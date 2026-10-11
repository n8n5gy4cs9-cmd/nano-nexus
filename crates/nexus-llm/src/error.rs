//! LLM error type (INFERENCE-CONTRACT §7) and its mapping onto `NexusError`.

use nexus_core::error::NexusError;

/// Everything that can go wrong talking to the inference server.
#[derive(thiserror::Error, Debug)]
pub enum LlmError {
    #[error("runtime unavailable")]
    RuntimeUnavailable,
    #[error("busy")]
    Busy,
    #[error("cancelled")]
    Cancelled,
    #[error("http {status}: {body}")]
    Http { status: u16, body: String },
    #[error("transport: {0}")]
    Transport(String),
    #[error("context overflow: need {need}, have {have}")]
    ContextOverflow { need: u32, have: u32 },
    #[error("protocol: {0}")]
    Protocol(String),
}

impl From<reqwest::Error> for LlmError {
    fn from(e: reqwest::Error) -> Self {
        LlmError::Transport(e.without_url().to_string())
    }
}

impl From<LlmError> for NexusError {
    fn from(e: LlmError) -> Self {
        match e {
            LlmError::RuntimeUnavailable => NexusError::RuntimeUnavailable("model is not ready".into()),
            LlmError::Busy => NexusError::Busy("the model is busy".into()),
            LlmError::Cancelled => NexusError::Cancelled,
            LlmError::ContextOverflow { need, have } => {
                NexusError::ContextOverflow(format!("need {need} tokens, have {have}"))
            }
            LlmError::Protocol(m) => NexusError::LlmInvalidOutput(m),
            LlmError::Http { status, body } => {
                NexusError::Internal(format!("llama-server http {status}: {body}"))
            }
            LlmError::Transport(m) => NexusError::RuntimeUnavailable(m),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::error::ErrorCode;

    #[test]
    fn maps_to_error_codes() {
        let code = |e: LlmError| NexusError::from(e).code();
        assert_eq!(code(LlmError::Busy), ErrorCode::Busy);
        assert_eq!(code(LlmError::Cancelled), ErrorCode::Cancelled);
        assert_eq!(code(LlmError::RuntimeUnavailable), ErrorCode::RuntimeUnavailable);
        assert_eq!(
            code(LlmError::ContextOverflow { need: 2, have: 1 }),
            ErrorCode::ContextOverflow
        );
        assert_eq!(code(LlmError::Protocol("x".into())), ErrorCode::LlmInvalidOutput);
        assert_eq!(
            code(LlmError::Http {
                status: 500,
                body: String::new()
            }),
            ErrorCode::Internal
        );
    }
}
