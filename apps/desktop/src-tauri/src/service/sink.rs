//! Where streamed events go: a Tauri `Channel` on desktop, a WebSocket for LAN remotes, a `Vec` in tests.

use std::sync::{Arc, Mutex};

use serde::Serialize;

/// Receives the events of a streaming service call, in order.
pub trait Sink<E>: Send + Sync + 'static {
    /// Deliver one event. `false` means the receiver is gone, so the producer should stop.
    fn send(&self, event: E) -> bool;
}

/// Sink over a Tauri `Channel` (ordered delivery to the webview).
pub struct ChannelSink<E: Serialize + Send + Sync + 'static>(pub tauri::ipc::Channel<E>);

impl<E: Serialize + Send + Sync + 'static> Sink<E> for ChannelSink<E> {
    fn send(&self, event: E) -> bool {
        self.0.send(event).is_ok()
    }
}

/// Collects events in memory (tests).
#[derive(Clone)]
pub struct VecSink<E>(pub Arc<Mutex<Vec<E>>>);

impl<E> Default for VecSink<E> {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Vec::new())))
    }
}

impl<E: Clone> VecSink<E> {
    /// Everything received so far.
    pub fn events(&self) -> Vec<E> {
        self.0.lock().map(|v| v.clone()).unwrap_or_default()
    }
}

impl<E: Send + Sync + 'static> Sink<E> for VecSink<E> {
    fn send(&self, event: E) -> bool {
        match self.0.lock() {
            Ok(mut v) => {
                v.push(event);
                true
            }
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec_sink_keeps_order() {
        let s = VecSink::<u32>::default();
        assert!(s.send(1) && s.send(2) && s.send(3));
        assert_eq!(s.events(), vec![1, 2, 3]);
    }
}
