//! Where streamed events go. The desktop app wraps a Tauri `Channel`, LAN Share wraps a WebSocket,
//! tests collect into a `Vec`. Library crates only see this trait.

use std::sync::{Arc, Mutex};

/// Receives the events of a streaming call, in order.
pub trait Sink<E>: Send + Sync {
    /// Deliver one event. `false` means the receiver is gone, so the producer should stop.
    fn send(&self, event: E) -> bool;
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

impl<E: Send> Sink<E> for VecSink<E> {
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

/// Drops everything.
pub struct NullSink;

impl<E> Sink<E> for NullSink {
    fn send(&self, _event: E) -> bool {
        true
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
        assert!(NullSink.send(1u8));
    }
}
