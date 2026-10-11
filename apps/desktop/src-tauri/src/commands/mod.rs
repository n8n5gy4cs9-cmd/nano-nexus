//! Thin Tauri command wrappers. Each returns `Result<T, IpcError>` (DATA-AND-IPC §2).

pub mod app;
pub mod assistants;
pub mod attachments;
pub mod analyzer;
pub mod chat;
pub mod conversations;
pub mod editor;
pub mod fs;
pub mod jobs;
pub mod knowledge;
pub mod models;
pub mod notifications;
pub mod permissions;
pub mod rename;
pub mod runtime;
pub mod settings;
pub mod share;

use nexus_core::error::IpcError;

/// Result type of every command.
pub type CmdResult<T> = Result<T, IpcError>;
