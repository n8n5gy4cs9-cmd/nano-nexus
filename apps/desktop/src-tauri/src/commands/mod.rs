//! Thin Tauri command wrappers. Each returns `Result<T, IpcError>` (DATA-AND-IPC §2).

pub mod app;
pub mod chat;
pub mod notifications;
pub mod runtime;
pub mod settings;

use nexus_core::error::IpcError;

/// Result type of every command.
pub type CmdResult<T> = Result<T, IpcError>;
