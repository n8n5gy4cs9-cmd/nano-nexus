//! Thin Tauri command wrappers. Each returns `Result<T, IpcError>` (DATA-AND-IPC §2).

pub mod app;
pub mod assistants;
pub mod attachments;
pub mod chat;
pub mod conversations;
pub mod fs;
pub mod models;
pub mod notifications;
pub mod permissions;
pub mod runtime;
pub mod settings;

use nexus_core::error::IpcError;

/// Result type of every command.
pub type CmdResult<T> = Result<T, IpcError>;
