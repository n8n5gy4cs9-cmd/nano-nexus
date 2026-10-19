//! What the share server needs from the app. The app implements [`ShareApi`] by delegating to
//! `service::*` with `Caller::Remote`, so this crate never depends on Tauri.

use std::sync::Arc;

use async_trait::async_trait;
use nexus_core::error::{ErrorCode, IpcError};
use nexus_core::settings::SharePolicy;
use nexus_core::share::ShareDevice;
use nexus_core::sink::Sink;
use serde_json::Value;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// A global event pushed to subscribed clients.
#[derive(Clone, Debug)]
pub struct Push {
    pub event: String,
    pub data: Value,
}

/// The command surface offered to remote devices.
#[async_trait]
pub trait ShareApi: Send + Sync + 'static {
    /// Run `cmd` for `device`. Streamed events go to `sink`; `cancel` fires when the client
    /// sends a cancel frame or disconnects. The server has already applied the allow-list and policy.
    async fn call(
        &self,
        device: &ShareDevice,
        cmd: &str,
        args: Value,
        sink: Arc<dyn Sink<Value>>,
        cancel: CancellationToken,
    ) -> Result<Value, IpcError>;

    /// Global events (`runtime:status`, `conversation:updated`, ...).
    fn subscribe(&self) -> broadcast::Receiver<Push>;
}

/// Commands that stream events before their final result.
pub const STREAMING: &[&str] = &["chat_send", "chat_regenerate", "chat_edit_resend"];

/// Global events a client may subscribe to.
pub const PUSH_EVENTS: &[&str] = &[
    "runtime:status",
    "conversation:updated",
    "notification:new",
    "model:changed",
];

const BASE: &[&str] = &[
    "app_info",
    "runtime_status",
    "assistants_list",
    "assistant_get",
    "chat_send",
    "chat_stop",
    "chat_regenerate",
    "chat_edit_resend",
    "conversations_list",
    "conversation_get",
    "conversation_update",
    "conversation_export_md",
    "notifications_list",
    "notifications_unread_count",
    "notifications_mark_read",
];

const READ: &[&str] = &[
    "reports_list",
    "report_folder",
    "report_body",
    "models_list",
    "fs_grants_list",
];

const FULL: &[&str] = &["models_activate"];

fn denied(cmd: &str) -> IpcError {
    IpcError {
        code: ErrorCode::ForbiddenRemote,
        message: format!("`{cmd}` is not available to this device; use NanoNexus on your computer"),
        detail: None,
    }
}

/// Allow-list plus policy guard. Anything not listed is refused, whatever the policy
/// (apply / undo / grant commands are never on a list a `chat_*` policy reaches).
pub fn authorize(policy: SharePolicy, cmd: &str) -> Result<(), IpcError> {
    let allowed = BASE.contains(&cmd)
        || (policy != SharePolicy::ChatOnly && READ.contains(&cmd))
        || (policy == SharePolicy::Full && FULL.contains(&cmd));
    if allowed {
        return Ok(());
    }
    let known = BASE.iter().chain(READ).chain(FULL).any(|c| *c == cmd);
    if known {
        Err(denied(cmd))
    } else {
        Err(IpcError {
            code: ErrorCode::NotFound,
            message: format!("unknown command `{cmd}`"),
            detail: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_policies_cannot_apply_or_grant() {
        for cmd in [
            "changeset_apply",
            "changeset_undo",
            "fs_grant",
            "settings_set",
            "share_start",
        ] {
            for p in [SharePolicy::ChatOnly, SharePolicy::ChatRead] {
                assert_eq!(authorize(p, cmd).unwrap_err().code, ErrorCode::NotFound, "{cmd}");
            }
        }
    }

    #[test]
    fn read_commands_need_chat_read() {
        assert!(authorize(SharePolicy::ChatRead, "reports_list").is_ok());
        assert_eq!(
            authorize(SharePolicy::ChatOnly, "reports_list").unwrap_err().code,
            ErrorCode::ForbiddenRemote
        );
        assert!(authorize(SharePolicy::ChatOnly, "chat_send").is_ok());
    }

    #[test]
    fn activating_models_needs_full() {
        assert!(authorize(SharePolicy::ChatRead, "models_activate").is_err());
        assert!(authorize(SharePolicy::Full, "models_activate").is_ok());
    }

    #[test]
    fn activity_is_desktop_only() {
        // ADR-017: the Activity page reads other tools' transcripts; it is never reachable remotely.
        for cmd in [
            "observe_snapshot",
            "observe_replay",
            "observe_set_source",
            "observe_demo",
        ] {
            for p in [SharePolicy::ChatOnly, SharePolicy::ChatRead, SharePolicy::Full] {
                assert!(authorize(p, cmd).is_err(), "{cmd}");
            }
        }
        assert!(!PUSH_EVENTS.iter().any(|e| e.starts_with("observe")));
    }

    #[test]
    fn streaming_commands_are_allowed_for_chat() {
        for c in STREAMING {
            assert!(authorize(SharePolicy::ChatOnly, c).is_ok());
        }
    }
}
