//! Schema-constrained JSON output with validation, one re-ask, and a deterministic fallback
//! (INFERENCE-CONTRACT §4.3, ADR-003).

use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::error::LlmError;
use crate::scheduler::{Priority, Scheduler};
use crate::types::{ChatMessage, ChatRequest, ResponseFormat};

/// Facts a validator needs that are not in the JSON itself (allowed ids, reserved names, ...).
#[derive(Clone, Debug, Default)]
pub struct ValidationCtx {
    /// Ids the model may refer to (e.g. the batch's symbol ids).
    pub allowed_ids: Vec<String>,
    /// Names the model must not produce (keywords, existing symbols).
    pub reserved_names: Vec<String>,
    /// Naming-convention regex, for validators that check it themselves.
    pub pattern: Option<String>,
}

/// Domain validation on top of schema conformance. Domain validators live in the crate that owns the
/// domain (e.g. rename rules in `nexus-analyze`). Return every problem found, in plain words: they
/// are shown to the model on the re-ask.
pub trait Validate {
    fn validate(&self, ctx: &ValidationCtx) -> Result<(), Vec<String>>;
}

/// Outcome of [`chat_json`]. `Fallback` means the caller must apply its deterministic default
/// (see the table in the contract); it is not an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Structured<T> {
    Valid(T),
    Fallback { reason: String },
}

/// What to ask for.
#[derive(Clone, Debug)]
pub struct StructuredSpec<'a> {
    /// Schema name sent as `json_schema.name`.
    pub name: &'a str,
    pub schema: &'a Value,
    pub temperature: f32,
    pub max_tokens: u32,
}

/// Drop documentation-only keys (`$comment`, `x-nexus-*`) before the schema goes to the server.
pub fn clean_schema(schema: &Value) -> Value {
    match schema {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, _)| *k != "$comment" && !k.starts_with("x-nexus-"))
                .map(|(k, v)| (k.clone(), clean_schema(v)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(clean_schema).collect()),
        other => other.clone(),
    }
}

fn build_request(messages: &[ChatMessage], spec: &StructuredSpec<'_>, temperature: f32) -> ChatRequest {
    let mut req = ChatRequest::new(messages.to_vec())
        .temperature(temperature)
        .max_tokens(spec.max_tokens)
        .response_format(ResponseFormat::json_schema(spec.name, clean_schema(spec.schema)));
    req.top_p = 0.9;
    req.top_k = None;
    req.repeat_penalty = None;
    req
}

/// Ask for JSON matching `spec.schema`, validate it, re-ask once with the problems listed (at a lower
/// temperature), and fall back if it is still invalid. Transport and cancellation errors propagate.
///
/// ```ignore
/// let out: Structured<Title> =
///     chat_json(&sched, Priority::Background, msgs, spec, &ValidationCtx::default(), cancel).await?;
/// let title = match out {
///     Structured::Valid(t) => t.title,
///     Structured::Fallback { .. } => first_six_words(&user_text),
/// };
/// ```
pub async fn chat_json<T: DeserializeOwned + Validate>(
    sched: &Scheduler,
    prio: Priority,
    mut messages: Vec<ChatMessage>,
    spec: StructuredSpec<'_>,
    vctx: &ValidationCtx,
    cancel: CancellationToken,
) -> Result<Structured<T>, LlmError> {
    let mut last_errs = Vec::new();
    for attempt in 0..2 {
        let temp = if attempt == 0 {
            spec.temperature
        } else {
            (spec.temperature * 0.5).min(0.2)
        };
        let raw = sched
            .complete_wait(prio, build_request(&messages, &spec, temp), cancel.clone())
            .await?;
        let checked = serde_json::from_str::<T>(raw.trim())
            .map_err(|e| vec![format!("not valid JSON for the schema: {e}")])
            .and_then(|v| v.validate(vctx).map(|()| v));
        match checked {
            Ok(v) => return Ok(Structured::Valid(v)),
            Err(errs) => {
                if attempt == 0 {
                    messages.push(ChatMessage::assistant(raw));
                    messages.push(ChatMessage::user(format!(
                        "Your JSON was invalid:\n- {}\nReturn corrected JSON only.",
                        errs.join("\n- ")
                    )));
                }
                last_errs = errs;
            }
        }
    }
    Ok(Structured::Fallback {
        reason: last_errs.join("; "),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn clean_schema_strips_doc_keys_recursively() {
        let s = json!({"$comment": "c", "x-nexus-patterns": {"a": "b"}, "type": "object",
            "properties": {"x": {"$comment": "n", "type": "string"}}, "anyOf": [{"x-nexus-y": 1, "type": "null"}]});
        assert_eq!(
            clean_schema(&s),
            json!({"type": "object", "properties": {"x": {"type": "string"}}, "anyOf": [{"type": "null"}]})
        );
    }
}
