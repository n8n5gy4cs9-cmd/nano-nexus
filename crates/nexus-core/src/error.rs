//! Error codes and error types shared across crates and sent over IPC.

use serde::{Deserialize, Serialize};

/// Stable machine-readable error code (see DATA-AND-IPC §4). Serialized as snake_case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    RuntimeUnavailable,
    ModelMissing,
    PermissionDenied,
    OsPermissionDenied,
    PathDenied,
    NotFound,
    InvalidInput,
    LlmInvalidOutput,
    ContextOverflow,
    Cancelled,
    Conflict,
    ChecksumMismatch,
    InsufficientDisk,
    Network,
    Busy,
    PortInUse,
    Unauthorized,
    ForbiddenRemote,
    Internal,
}

/// Error type for non-IPC Rust code. Every variant maps to an [`ErrorCode`].
#[derive(Debug, thiserror::Error)]
pub enum NexusError {
    #[error("runtime unavailable: {0}")]
    RuntimeUnavailable(String),
    #[error("no model installed")]
    ModelMissing,
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("OS blocked access: {0}")]
    OsPermissionDenied(String),
    #[error("protected path: {0}")]
    PathDenied(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("unusable model output: {0}")]
    LlmInvalidOutput(String),
    #[error("context overflow: {0}")]
    ContextOverflow(String),
    #[error("cancelled")]
    Cancelled,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("checksum mismatch: {0}")]
    ChecksumMismatch(String),
    #[error("insufficient disk space: need {need} bytes, have {have}")]
    InsufficientDisk { need: u64, have: u64 },
    #[error("network error: {0}")]
    Network(String),
    #[error("busy: {0}")]
    Busy(String),
    #[error("port {0} is already in use")]
    PortInUse(u16),
    #[error("unauthorized")]
    Unauthorized,
    #[error("requires the desktop app")]
    ForbiddenRemote,
    #[error("internal error: {0}")]
    Internal(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl NexusError {
    /// The IPC-facing code for this error.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::RuntimeUnavailable(_) => ErrorCode::RuntimeUnavailable,
            Self::ModelMissing => ErrorCode::ModelMissing,
            Self::PermissionDenied(_) => ErrorCode::PermissionDenied,
            Self::OsPermissionDenied(_) => ErrorCode::OsPermissionDenied,
            Self::PathDenied(_) => ErrorCode::PathDenied,
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::InvalidInput(_) => ErrorCode::InvalidInput,
            Self::LlmInvalidOutput(_) => ErrorCode::LlmInvalidOutput,
            Self::ContextOverflow(_) => ErrorCode::ContextOverflow,
            Self::Cancelled => ErrorCode::Cancelled,
            Self::Conflict(_) => ErrorCode::Conflict,
            Self::ChecksumMismatch(_) => ErrorCode::ChecksumMismatch,
            Self::InsufficientDisk { .. } => ErrorCode::InsufficientDisk,
            Self::Network(_) => ErrorCode::Network,
            Self::Busy(_) => ErrorCode::Busy,
            Self::PortInUse(_) => ErrorCode::PortInUse,
            Self::Unauthorized => ErrorCode::Unauthorized,
            Self::ForbiddenRemote => ErrorCode::ForbiddenRemote,
            Self::Internal(_) => ErrorCode::Internal,
            Self::Io(e) => match e.kind() {
                std::io::ErrorKind::NotFound => ErrorCode::NotFound,
                std::io::ErrorKind::PermissionDenied => ErrorCode::OsPermissionDenied,
                _ => ErrorCode::Internal,
            },
        }
    }
}

/// Error sent to the frontend. `detail` holds the debug chain (shown only under "Details").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct IpcError {
    pub code: ErrorCode,
    pub message: String,
    pub detail: Option<String>,
}

impl From<NexusError> for IpcError {
    fn from(e: NexusError) -> Self {
        Self {
            code: e.code(),
            message: e.to_string(),
            detail: Some(format!("{e:?}")),
        }
    }
}

/// Convenience alias for library results.
pub type Result<T, E = NexusError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_serialize_snake_case() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::PermissionDenied).unwrap(),
            "\"permission_denied\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::LlmInvalidOutput).unwrap(),
            "\"llm_invalid_output\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::OsPermissionDenied).unwrap(),
            "\"os_permission_denied\""
        );
    }

    #[test]
    fn nexus_error_maps_to_code() {
        assert_eq!(NexusError::PortInUse(9889).code(), ErrorCode::PortInUse);
        assert_eq!(
            NexusError::InsufficientDisk { need: 2, have: 1 }.code(),
            ErrorCode::InsufficientDisk
        );
        let nf = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(NexusError::from(nf).code(), ErrorCode::NotFound);
    }

    #[test]
    fn ipc_error_round_trips() {
        let e: IpcError = NexusError::Busy("indexing".into()).into();
        assert_eq!(e.code, ErrorCode::Busy);
        assert!(e.message.contains("indexing"));
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["code"], "busy");
        let back: IpcError = serde_json::from_value(json).unwrap();
        assert_eq!(back, e);
    }
}
