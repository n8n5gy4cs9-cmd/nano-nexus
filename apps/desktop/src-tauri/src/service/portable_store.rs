//! Exclusive lifetime ownership for shared-model mutations across both model APIs.
//!
//! A conservative single writer also covers digest aliases, companions and partial cleanup.
//! Cloned leases keep detached blocking work owned after its requesting future is dropped.

use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use nexus_core::error::NexusError;

#[derive(Default)]
struct State {
    active: Weak<Lease>,
    closing: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    active: Weak::new(),
    closing: false,
});

/// Held until publication, verification and cancellation cleanup have all finished.
pub(super) struct Lease;

/// Reject overlapping mutations instead of letting two APIs share a partial file.
pub(super) fn acquire() -> Result<Arc<Lease>, NexusError> {
    let mut state = STATE
        .lock()
        .map_err(|_| NexusError::Internal("shared model ownership lock poisoned".into()))?;
    if state.closing {
        return Err(NexusError::Busy("NanoNexus is shutting down".into()));
    }
    if state.active.upgrade().is_some() {
        return Err(NexusError::Busy(
            "Another shared-model operation is running. Wait for it to finish or cancel it, then retry."
                .into(),
        ));
    }
    let lease = Arc::new(Lease);
    state.active = Arc::downgrade(&lease);
    Ok(lease)
}

/// Stop admission before cancelling existing work, including the legacy queue pump.
pub(crate) fn close() {
    if let Ok(mut state) = STATE.lock() {
        state.closing = true;
    }
}

/// Whether shutdown has started. Poisoned ownership state fails closed.
pub(super) fn closing() -> bool {
    STATE.lock().map_or(true, |state| state.closing)
}

/// Wait a bounded time for owned blocking work to observe cancellation and finish.
pub(crate) async fn drain() -> bool {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let idle = STATE.lock().is_ok_and(|state| state.active.upgrade().is_none());
            if idle {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .is_ok()
}
