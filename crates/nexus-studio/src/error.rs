//! Errors from the studio workers. Messages are shown to the user, so they say what to do.

use nexus_core::error::NexusError;

#[derive(Debug, thiserror::Error)]
pub enum StudioError {
    #[error("{0} is not installed in this distribution.")]
    NotInstalled(String),
    #[error("Could not start the engine: {0}")]
    Start(String),
    #[error("The engine did not become ready: {0}")]
    NotReady(String),
    #[error("{0}")]
    Failed(String),
    #[error("Cancelled.")]
    Cancelled,
    #[error("{0}")]
    Invalid(String),
    #[error("File error: {0}")]
    Io(String),
}

impl From<std::io::Error> for StudioError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<StudioError> for NexusError {
    fn from(e: StudioError) -> Self {
        match e {
            StudioError::NotInstalled(m) => Self::RuntimeUnavailable(m),
            StudioError::Start(m) | StudioError::NotReady(m) => Self::RuntimeUnavailable(m),
            StudioError::Cancelled => Self::Cancelled,
            StudioError::Invalid(m) => Self::InvalidInput(m),
            StudioError::Failed(m) | StudioError::Io(m) => Self::Internal(m),
        }
    }
}
