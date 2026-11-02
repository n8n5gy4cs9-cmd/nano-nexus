//! Transport-agnostic business API. Each Tauri command is a thin wrapper over a function here;
//! LAN Share will call the same functions with `Caller::Remote` over WebSocket (M14).

pub mod analyzer;
pub mod app;
pub mod assistants;
pub mod attachments;
pub mod chat;
pub mod coalesce;
mod consultations;
pub mod conversations;
pub mod diagnostics;
pub mod editor;
pub mod fs;
pub mod jobs;
pub mod knowledge;
mod knowledge_paths;
pub mod media;
pub mod models;
pub mod notifications;
pub mod observe;
pub mod permissions;
pub mod pipelines;
pub(crate) mod portable_store;
pub mod rename;
pub mod runtime;
pub mod settings;
pub mod share;
pub mod sink;
pub mod studio;
pub mod watches;

use nexus_core::settings::SharePolicy;

pub use sink::{ChannelSink, NullSink, Sink, VecSink};

/// Who is asking. Remotes get fewer powers (checked in the services that need it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caller {
    /// The desktop UI.
    Desktop,
    /// A paired LAN device.
    Remote { device_id: String, policy: SharePolicy },
}

impl Caller {
    pub fn is_desktop(&self) -> bool {
        matches!(self, Caller::Desktop)
    }
}
