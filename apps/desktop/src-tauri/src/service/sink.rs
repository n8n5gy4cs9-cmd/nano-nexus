//! Sinks for the desktop: a Tauri `Channel`. (`Sink`/`VecSink` live in `nexus-core`.)

use serde::Serialize;

pub use nexus_core::sink::{NullSink, Sink, VecSink};

/// Sink over a Tauri `Channel` (ordered delivery to the webview).
pub struct ChannelSink<E: Serialize + Send + Sync + 'static>(pub tauri::ipc::Channel<E>);

impl<E: Serialize + Send + Sync + 'static> Sink<E> for ChannelSink<E> {
    fn send(&self, event: E) -> bool {
        self.0.send(event).is_ok()
    }
}
