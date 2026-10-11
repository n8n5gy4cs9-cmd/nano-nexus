//! Chat data types shared by the store, the agent, and the IPC layer (DATA-AND-IPC §1, §2).

use serde::{Deserialize, Serialize};

use crate::error::ErrorCode;
use crate::ids::{AssistantId, ConversationId, MessageId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
    Event,
}

impl MessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
            Self::Event => "event",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "system" => Self::System,
            "user" => Self::User,
            "assistant" => Self::Assistant,
            "tool" => Self::Tool,
            "event" => Self::Event,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    Streaming,
    Done,
    Stopped,
    Error,
}

impl MessageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Streaming => "streaming",
            Self::Done => "done",
            Self::Stopped => "stopped",
            Self::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "streaming" => Self::Streaming,
            "done" => Self::Done,
            "stopped" => Self::Stopped,
            "error" => Self::Error,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    File,
    Folder,
    Text,
    Selection,
}

impl AttachmentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Folder => "folder",
            Self::Text => "text",
            Self::Selection => "selection",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "file" => Self::File,
            "folder" => Self::Folder,
            "text" => Self::Text,
            "selection" => Self::Selection,
            _ => return None,
        })
    }
}

/// An attachment as sent from the UI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct AttachmentInput {
    pub kind: AttachmentKind,
    pub path: Option<String>,
    pub label: String,
    /// The text to send to the model (already read and truncated by the UI/backend).
    pub text: Option<String>,
}

/// A stored attachment. `snapshot` is the text that was actually sent (truncated).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Attachment {
    pub id: String,
    pub kind: AttachmentKind,
    pub path: Option<String>,
    pub label: String,
    pub tokens: Option<u32>,
    pub snapshot: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Message {
    pub id: MessageId,
    pub conversation_id: ConversationId,
    pub role: MessageRole,
    pub content: String,
    /// Thoughts (never re-sent to the model).
    pub reasoning: Option<String>,
    pub assistant_id: Option<AssistantId>,
    pub model_id: Option<String>,
    /// JSON array of `{tool, args, summary, ok}`.
    pub tool_trace: Option<String>,
    pub status: MessageStatus,
    pub error: Option<String>,
    pub tokens_prompt: Option<u32>,
    pub tokens_out: Option<u32>,
    pub tps: Option<f32>,
    pub ttft_ms: Option<u32>,
    pub created_at: i64,
    pub attachments: Vec<Attachment>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Conversation {
    pub id: ConversationId,
    pub title: String,
    pub assistant_id: AssistantId,
    pub pinned: bool,
    pub summary: Option<String>,
    /// Last message id covered by `summary`.
    pub summary_upto: Option<MessageId>,
    pub created_at: i64,
    pub updated_at: i64,
    pub archived: bool,
}

/// One row of the conversation list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ConversationSummary {
    pub id: ConversationId,
    pub title: String,
    pub assistant_id: AssistantId,
    pub pinned: bool,
    pub updated_at: i64,
    /// For search results: the matching text around the hit.
    pub snippet: Option<String>,
}

/// Streamed to the UI while a turn runs (a `Channel<ChatEvent>`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
    Started {
        assistant_message_id: MessageId,
    },
    /// "Reading ~/Downloads…"
    Status {
        text: String,
    },
    /// `args_json` is the tool arguments as a JSON string (`serde_json::Value` cannot be exported).
    ToolCall {
        tool: String,
        args_json: String,
    },
    ToolResult {
        tool: String,
        summary: String,
        ok: bool,
    },
    PermissionNeeded {
        request_id: String,
        path: String,
        needed: String,
    },
    Reasoning {
        delta: String,
    },
    Content {
        delta: String,
    },
    Done {
        finish_reason: String,
        tokens_out: u32,
        tps: f32,
        ttft_ms: u32,
    },
    Error {
        code: ErrorCode,
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn enums_round_trip_their_string_forms() {
        for r in [
            MessageRole::System,
            MessageRole::User,
            MessageRole::Assistant,
            MessageRole::Tool,
            MessageRole::Event,
        ] {
            assert_eq!(MessageRole::parse(r.as_str()), Some(r));
        }
        for s in [
            MessageStatus::Streaming,
            MessageStatus::Done,
            MessageStatus::Stopped,
            MessageStatus::Error,
        ] {
            assert_eq!(MessageStatus::parse(s.as_str()), Some(s));
        }
        for k in [
            AttachmentKind::File,
            AttachmentKind::Folder,
            AttachmentKind::Text,
            AttachmentKind::Selection,
        ] {
            assert_eq!(AttachmentKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(MessageRole::parse("bogus"), None);
    }

    #[test]
    fn chat_event_is_internally_tagged() {
        let v = serde_json::to_value(ChatEvent::Content { delta: "hi".into() }).unwrap();
        assert_eq!(v, json!({"type": "content", "delta": "hi"}));
        let v = serde_json::to_value(ChatEvent::Error {
            code: ErrorCode::Busy,
            message: "x".into(),
        })
        .unwrap();
        assert_eq!(v, json!({"type": "error", "code": "busy", "message": "x"}));
        let done = ChatEvent::Done {
            finish_reason: "stop".into(),
            tokens_out: 3,
            tps: 50.0,
            ttft_ms: 120,
        };
        let back: ChatEvent = serde_json::from_value(serde_json::to_value(&done).unwrap()).unwrap();
        assert_eq!(back, done);
    }
}
