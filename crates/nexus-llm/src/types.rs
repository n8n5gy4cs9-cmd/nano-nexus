//! Wire and domain types for the OpenAI-compatible `llama-server` API (INFERENCE-CONTRACT §4).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Author of a chat message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// One message in a chat request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::new(Role::System, content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(Role::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(Role::Assistant, content)
    }
}

/// `response_format` for structured output (ADR-003). We send the OpenAI shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    JsonSchema { json_schema: JsonSchemaSpec },
}

/// The named schema inside a [`ResponseFormat::JsonSchema`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JsonSchemaSpec {
    pub name: String,
    pub strict: bool,
    pub schema: Value,
}

impl ResponseFormat {
    /// A strict JSON-schema format with the given name.
    pub fn json_schema(name: impl Into<String>, schema: Value) -> Self {
        Self::JsonSchema {
            json_schema: JsonSchemaSpec {
                name: name.into(),
                strict: true,
                schema,
            },
        }
    }
}

/// `stream_options`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamOptions {
    pub include_usage: bool,
}

/// Body of `POST /v1/chat/completions`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    pub temperature: f32,
    pub top_p: f32,
    pub min_p: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_penalty: Option<f32>,
    pub max_tokens: u32,
    pub cache_prompt: bool,
    pub chat_template_kwargs: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
}

impl ChatRequest {
    /// Chat defaults from the contract: temperature 0.7, top_p 0.95, min_p 0 (MiniCPM5 repeats with
    /// llama.cpp's 0.05 default), 1024 answer tokens, thinking off, prompt cache on.
    pub fn new(messages: Vec<ChatMessage>) -> Self {
        Self {
            messages,
            stream: false,
            temperature: 0.7,
            top_p: 0.95,
            min_p: 0.0,
            top_k: Some(40),
            repeat_penalty: Some(1.0),
            max_tokens: 1024,
            cache_prompt: true,
            chat_template_kwargs: json!({ "enable_thinking": false }),
            stream_options: None,
            response_format: None,
        }
    }

    /// Switch to streaming (and ask for usage in the final chunk).
    pub fn streaming(mut self) -> Self {
        self.stream = true;
        self.stream_options = Some(StreamOptions { include_usage: true });
        self
    }

    /// Turn the model's thinking mode on or off.
    pub fn thinking(mut self, on: bool) -> Self {
        self.chat_template_kwargs = json!({ "enable_thinking": on });
        self
    }

    pub fn temperature(mut self, t: f32) -> Self {
        self.temperature = t;
        self
    }

    pub fn max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = n;
        self
    }

    pub fn response_format(mut self, f: ResponseFormat) -> Self {
        self.response_format = Some(f);
        self
    }
}

/// Token accounting from the final chunk / response.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
}

/// llama.cpp `timings` extension.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Timings {
    #[serde(default)]
    pub prompt_n: u32,
    #[serde(default)]
    pub prompt_ms: f64,
    #[serde(default)]
    pub predicted_n: u32,
    #[serde(default)]
    pub predicted_ms: f64,
    #[serde(default)]
    pub predicted_per_second: f64,
}

/// Event handed to callers of a streaming chat.
#[derive(Clone, Debug, PartialEq)]
pub enum StreamEvent {
    /// Thinking text (`reasoning_content`, or a `<think>` span routed here).
    Reasoning(String),
    /// Answer text.
    Content(String),
    /// End of stream.
    Done {
        finish_reason: Option<String>,
        usage: Option<Usage>,
        timings: Option<Timings>,
    },
}

/// `delta` of a streamed choice.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Delta {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
}

/// A streamed choice.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ChunkChoice {
    #[serde(default)]
    pub delta: Delta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// One parsed SSE `data:` payload.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ChatChunk {
    #[serde(default)]
    pub choices: Vec<ChunkChoice>,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(default)]
    pub timings: Option<Timings>,
}

/// Message of a non-streaming choice.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ResponseMessage {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
}

/// A non-streaming choice.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ResponseChoice {
    #[serde(default)]
    pub message: ResponseMessage,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// Non-streaming `chat/completions` response.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ChatResponse {
    #[serde(default)]
    pub choices: Vec<ResponseChoice>,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(default)]
    pub timings: Option<Timings>,
}

impl ChatResponse {
    /// Text of the first choice (empty if there is none).
    pub fn text(&self) -> String {
        self.choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default()
    }
}

/// One entry of `GET /v1/models`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default)]
    pub meta: Option<Value>,
}

/// `GET /v1/models` body.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ModelsResponse {
    #[serde(default)]
    pub data: Vec<ModelInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes_per_contract() {
        let req = ChatRequest::new(vec![ChatMessage::system("s"), ChatMessage::user("u")]).streaming();
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["messages"][0]["role"], "system");
        assert_eq!(v["stream"], true);
        assert_eq!(v["min_p"], 0.0);
        assert_eq!(v["cache_prompt"], true);
        assert_eq!(v["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(v["stream_options"]["include_usage"], true);
        assert!(v.get("response_format").is_none());
    }

    #[test]
    fn response_format_uses_openai_shape() {
        let req = ChatRequest::new(vec![ChatMessage::user("u")])
            .response_format(ResponseFormat::json_schema("title", json!({"type": "object"})));
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["response_format"]["type"], "json_schema");
        assert_eq!(v["response_format"]["json_schema"]["name"], "title");
        assert_eq!(v["response_format"]["json_schema"]["strict"], true);
        assert_eq!(v["response_format"]["json_schema"]["schema"]["type"], "object");
    }

    #[test]
    fn thinking_toggle() {
        let v = serde_json::to_value(ChatRequest::new(vec![]).thinking(true)).unwrap();
        assert_eq!(v["chat_template_kwargs"]["enable_thinking"], true);
    }

    #[test]
    fn chunk_parses_content_reasoning_and_final() {
        let c: ChatChunk =
            serde_json::from_str(r#"{"choices":[{"delta":{"content":"Hel"},"finish_reason":null}]}"#)
                .unwrap();
        assert_eq!(c.choices[0].delta.content.as_deref(), Some("Hel"));
        let c: ChatChunk =
            serde_json::from_str(r#"{"choices":[{"delta":{"reasoning_content":"hm"}}]}"#).unwrap();
        assert_eq!(c.choices[0].delta.reasoning_content.as_deref(), Some("hm"));
        let c: ChatChunk = serde_json::from_str(
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7},"timings":{"prompt_n":3,"predicted_per_second":55.5}}"#,
        )
        .unwrap();
        assert_eq!(c.choices[0].finish_reason.as_deref(), Some("stop"));
        assert_eq!(c.usage.unwrap().total_tokens, 7);
        assert_eq!(c.timings.unwrap().predicted_per_second, 55.5);
    }

    #[test]
    fn response_and_models_parse() {
        let r: ChatResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"content":"{\"a\":1}"},"finish_reason":"stop"}]}"#,
        )
        .unwrap();
        assert_eq!(r.text(), "{\"a\":1}");
        let m: ModelsResponse =
            serde_json::from_str(r#"{"data":[{"id":"m.gguf","meta":{"n_ctx_train":32768}}]}"#).unwrap();
        assert_eq!(m.data[0].id, "m.gguf");
    }
}
