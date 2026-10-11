//! Transport-agnostic business API. Each Tauri command is a thin wrapper over a function here;
//! LAN Share will call the same functions with `Caller::Remote` over WebSocket (M14).

pub mod app;
pub mod assistants;
pub mod attachments;
pub mod chat;
pub mod coalesce;
pub mod conversations;
pub mod fs;
pub mod notifications;
pub mod permissions;
pub mod runtime;
pub mod settings;
pub mod sink;

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
