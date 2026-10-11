//! Payload types shared by the app, the LLM supervisor and the frontend.

use serde::{Deserialize, Serialize};

use crate::ids::{JobId, NotificationId};

/// State of the llama-server supervisor (published as `runtime:status`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RuntimeState {
    Stopped,
    Starting {
        attempt: u8,
    },
    Loading,
    Ready {
        model_id: String,
        model_path: String,
        ctx_size: u32,
        gpu_layers: String,
    },
    Failed {
        reason: String,
        log_tail: Vec<String>,
        can_retry: bool,
    },
}

/// Lifecycle of a background job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

/// Payload of `job:update`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct JobSummary {
    pub id: JobId,
    pub kind: String,
    pub title: String,
    pub status: JobStatus,
    /// 0.0..=1.0 when known.
    pub progress: Option<f32>,
    pub detail: Option<String>,
    pub result_ref: Option<String>,
    pub error: Option<String>,
}

/// Severity of a notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum NotificationLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// A click action attached to a notification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct NotificationAction {
    pub label: String,
    /// In-app route, query string included (e.g. `/models?focus=abc`).
    pub route: String,
}

/// Payload of `notification:new`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct NotificationPayload {
    pub id: NotificationId,
    pub level: NotificationLevel,
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub action: Option<NotificationAction>,
    pub read: bool,
    /// Unix milliseconds (UTC).
    pub created_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn runtime_state_is_internally_tagged() {
        assert_eq!(
            serde_json::to_value(RuntimeState::Stopped).unwrap(),
            json!({"state": "stopped"})
        );
        assert_eq!(
            serde_json::to_value(RuntimeState::Starting { attempt: 2 }).unwrap(),
            json!({"state": "starting", "attempt": 2})
        );
        let failed = RuntimeState::Failed {
            reason: "x".into(),
            log_tail: vec![],
            can_retry: true,
        };
        let back: RuntimeState = serde_json::from_value(serde_json::to_value(&failed).unwrap()).unwrap();
        assert_eq!(back, failed);
    }

    #[test]
    fn job_and_notification_round_trip() {
        let job = JobSummary {
            id: JobId::from("j1"),
            kind: "scan".into(),
            title: "Scan".into(),
            status: JobStatus::Running,
            progress: Some(0.5),
            detail: None,
            result_ref: None,
            error: None,
        };
        let v = serde_json::to_value(&job).unwrap();
        assert_eq!(v["status"], "running");
        assert_eq!(serde_json::from_value::<JobSummary>(v).unwrap(), job);

        let n = NotificationPayload {
            id: NotificationId::new(),
            level: NotificationLevel::Warning,
            kind: "download".into(),
            title: "t".into(),
            body: None,
            action: Some(NotificationAction {
                label: "Open".into(),
                route: "/models".into(),
            }),
            read: false,
            created_at: 1,
        };
        let v = serde_json::to_value(&n).unwrap();
        assert_eq!(v["level"], "warning");
        assert_eq!(serde_json::from_value::<NotificationPayload>(v).unwrap(), n);
    }
}
