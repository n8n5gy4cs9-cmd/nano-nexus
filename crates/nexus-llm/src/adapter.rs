//! Streaming tool-call adapters (UP.082).
//!
//! Pure decoding of streamed tool calls for two wire formats, plus argument validation and result
//! bounding. No network, no tool execution. A model or endpoint that cannot do structured tool calls
//! produces an explicit [`AdapterError`]; nothing is guessed from prose and no format is silently swapped.

use std::collections::BTreeMap;

use serde_json::Value;

/// Most tool calls accepted from one model turn.
pub const MAX_CALLS: usize = 16;
/// Largest argument text for one call.
pub const MAX_ARGS_BYTES: usize = 64 * 1024;

/// Wire format of a streamed answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    /// `choices[].delta.tool_calls[]` (llama.cpp server and OpenAI-compatible endpoints).
    OpenAiChat,
    /// `content_block_start` / `input_json_delta` events.
    AnthropicMessages,
}

/// Type of one argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgType {
    String,
    Integer,
    Number,
    Bool,
    Array,
    Object,
}

/// What a tool accepts. Unknown argument names are rejected rather than ignored.
#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: String,
    pub args: Vec<(String, ArgType, bool)>, // name, type, required
}

/// A finished, validated call.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: serde_json::Map<String, Value>,
}

/// Why a stream could not become tool calls.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AdapterError {
    #[error("the endpoint sent no structured tool calls; this model or profile cannot run coding tools")]
    NoToolSupport,
    #[error("the model asked for an unknown tool `{0}`")]
    UnknownTool(String),
    #[error("tool `{tool}` arguments are not a JSON object: {reason}")]
    BadJson { tool: String, reason: String },
    #[error("tool `{tool}`: {reason}")]
    BadArguments { tool: String, reason: String },
    #[error("the model sent more than {MAX_CALLS} tool calls in one turn")]
    TooManyCalls,
    #[error("tool `{0}` arguments exceed the size limit")]
    ArgsTooLarge(String),
    #[error("the stream ended while tool `{0}` was still being written")]
    Truncated(String),
}

#[derive(Debug, Default)]
struct Partial {
    id: String,
    name: String,
    args: String,
}

/// Collects streamed fragments. Feed every decoded JSON event, then call [`Self::finish`].
#[derive(Debug)]
pub struct CallAccumulator {
    protocol: Protocol,
    parts: BTreeMap<u64, Partial>,
    oversize: Option<String>,
}

impl CallAccumulator {
    pub fn new(protocol: Protocol) -> Self {
        Self {
            protocol,
            parts: BTreeMap::new(),
            oversize: None,
        }
    }

    /// Feed one decoded event (the JSON of one SSE `data:` payload).
    pub fn feed(&mut self, event: &Value) {
        match self.protocol {
            Protocol::OpenAiChat => {
                let calls = event["choices"][0]["delta"]["tool_calls"].as_array();
                for c in calls.into_iter().flatten() {
                    let index = c["index"].as_u64().unwrap_or(0);
                    let p = self.parts.entry(index).or_default();
                    if let Some(id) = c["id"].as_str() {
                        p.id = id.to_owned();
                    }
                    if let Some(name) = c["function"]["name"].as_str() {
                        p.name.push_str(name);
                    }
                    if let Some(a) = c["function"]["arguments"].as_str() {
                        p.args.push_str(a);
                    }
                }
            }
            Protocol::AnthropicMessages => {
                let index = event["index"].as_u64().unwrap_or(0);
                match event["type"].as_str() {
                    Some("content_block_start") if event["content_block"]["type"] == "tool_use" => {
                        let p = self.parts.entry(index).or_default();
                        p.id = event["content_block"]["id"].as_str().unwrap_or("").to_owned();
                        p.name = event["content_block"]["name"].as_str().unwrap_or("").to_owned();
                    }
                    Some("content_block_delta") if event["delta"]["type"] == "input_json_delta" => {
                        if let Some(p) = self.parts.get_mut(&index) {
                            p.args
                                .push_str(event["delta"]["partial_json"].as_str().unwrap_or(""));
                        }
                    }
                    _ => {}
                }
            }
        }
        for p in self.parts.values() {
            if p.args.len() > MAX_ARGS_BYTES && self.oversize.is_none() {
                self.oversize = Some(p.name.clone());
            }
        }
    }

    /// True once any tool fragment has been seen.
    pub fn has_calls(&self) -> bool {
        !self.parts.is_empty()
    }

    /// Validate every collected call against `tools`. `stream_complete` must be false when the stream was
    /// cut off (cancel, network loss): a half-written call is reported, never executed.
    pub fn finish(self, tools: &[ToolSpec], stream_complete: bool) -> Result<Vec<ToolCall>, AdapterError> {
        if self.parts.is_empty() {
            return Err(AdapterError::NoToolSupport);
        }
        if self.parts.len() > MAX_CALLS {
            return Err(AdapterError::TooManyCalls);
        }
        if let Some(name) = self.oversize {
            return Err(AdapterError::ArgsTooLarge(name));
        }
        let mut out = Vec::new();
        for (i, p) in self.parts {
            let spec = tools
                .iter()
                .find(|t| t.name == p.name)
                .ok_or_else(|| AdapterError::UnknownTool(p.name.clone()))?;
            let text = if p.args.trim().is_empty() {
                "{}"
            } else {
                p.args.as_str()
            };
            let parsed: Result<Value, _> = serde_json::from_str(text);
            let value = match parsed {
                Ok(v) => v,
                Err(_) if !stream_complete => return Err(AdapterError::Truncated(p.name)),
                Err(e) => {
                    return Err(AdapterError::BadJson {
                        tool: p.name,
                        reason: e.to_string(),
                    });
                }
            };
            let Value::Object(args) = value else {
                return Err(AdapterError::BadJson {
                    tool: p.name,
                    reason: "expected an object".into(),
                });
            };
            validate_args(spec, &args)?;
            let id = if p.id.is_empty() {
                format!("call-{i}")
            } else {
                p.id
            };
            out.push(ToolCall {
                id,
                name: p.name,
                args,
            });
        }
        Ok(out)
    }
}

fn validate_args(spec: &ToolSpec, args: &serde_json::Map<String, Value>) -> Result<(), AdapterError> {
    let bad = |reason: String| AdapterError::BadArguments {
        tool: spec.name.clone(),
        reason,
    };
    for key in args.keys() {
        if !spec.args.iter().any(|(n, _, _)| n == key) {
            return Err(bad(format!("unknown argument `{key}`")));
        }
    }
    for (name, ty, required) in &spec.args {
        match args.get(name) {
            None | Some(Value::Null) if *required => {
                return Err(bad(format!("missing required argument `{name}`")));
            }
            None | Some(Value::Null) => {}
            Some(v) => {
                let ok = match ty {
                    ArgType::String => v.is_string(),
                    ArgType::Integer => v.is_i64() || v.is_u64(),
                    ArgType::Number => v.is_number(),
                    ArgType::Bool => v.is_boolean(),
                    ArgType::Array => v.is_array(),
                    ArgType::Object => v.is_object(),
                };
                if !ok {
                    return Err(bad(format!("argument `{name}` has the wrong type")));
                }
            }
        }
    }
    Ok(())
}

/// Keep a tool result within `max_bytes` for the model, cutting on a character boundary and saying so.
pub fn bound_result(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[result cut: {} of {} bytes shown]",
        &text[..end],
        end,
        text.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn read_file() -> ToolSpec {
        ToolSpec {
            name: "read_file".into(),
            args: vec![
                ("path".into(), ArgType::String, true),
                ("limit".into(), ArgType::Integer, false),
            ],
        }
    }

    fn openai(index: u64, id: Option<&str>, name: Option<&str>, args: &str) -> Value {
        let mut f = json!({ "arguments": args });
        if let Some(n) = name {
            f["name"] = json!(n);
        }
        let mut c = json!({ "index": index, "function": f });
        if let Some(i) = id {
            c["id"] = json!(i);
        }
        json!({"choices":[{"delta":{"tool_calls":[c]}}]})
    }

    #[test]
    fn openai_fragments_join_into_one_validated_call() {
        let mut acc = CallAccumulator::new(Protocol::OpenAiChat);
        acc.feed(&openai(0, Some("c1"), Some("read_file"), "{\"pa"));
        acc.feed(&openai(0, None, None, "th\":\"a.rs\"}"));
        let calls = acc.finish(&[read_file()], true).unwrap();
        assert_eq!(calls[0].id, "c1");
        assert_eq!(calls[0].args["path"], "a.rs");
    }

    #[test]
    fn anthropic_input_json_deltas_join() {
        let mut acc = CallAccumulator::new(Protocol::AnthropicMessages);
        acc.feed(&json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t1","name":"read_file"}}));
        acc.feed(&json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}));
        acc.feed(&json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"b.rs\"}"}}));
        assert_eq!(acc.finish(&[read_file()], true).unwrap()[0].args["path"], "b.rs");
    }

    #[test]
    fn plain_prose_is_an_explicit_compatibility_error() {
        let mut acc = CallAccumulator::new(Protocol::OpenAiChat);
        acc.feed(&json!({"choices":[{"delta":{"content":"I will read it"}}]}));
        assert_eq!(acc.finish(&[read_file()], true), Err(AdapterError::NoToolSupport));
    }

    #[test]
    fn invalid_calls_are_rejected_with_a_reason() {
        let run = |name: &str, args: &str, complete: bool| {
            let mut acc = CallAccumulator::new(Protocol::OpenAiChat);
            acc.feed(&openai(0, Some("c"), Some(name), args));
            acc.finish(&[read_file()], complete)
        };
        assert!(matches!(
            run("rm_rf", "{}", true),
            Err(AdapterError::UnknownTool(_))
        ));
        assert!(matches!(
            run("read_file", "{}", true),
            Err(AdapterError::BadArguments { .. })
        ));
        assert!(matches!(
            run("read_file", "{\"path\":1}", true),
            Err(AdapterError::BadArguments { .. })
        ));
        assert!(matches!(
            run("read_file", "{\"path\":\"a\",\"x\":1}", true),
            Err(AdapterError::BadArguments { .. })
        ));
        assert!(matches!(
            run("read_file", "{\"path\":\"a", false),
            Err(AdapterError::Truncated(_))
        ));
        assert!(matches!(
            run("read_file", "{\"path\":\"a", true),
            Err(AdapterError::BadJson { .. })
        ));
    }

    #[test]
    fn results_are_cut_on_character_boundaries() {
        let s = "ééééé";
        let cut = bound_result(s, 3);
        assert!(cut.starts_with('é') && cut.contains("[result cut"));
        assert_eq!(bound_result("ok", 10), "ok");
    }
}
