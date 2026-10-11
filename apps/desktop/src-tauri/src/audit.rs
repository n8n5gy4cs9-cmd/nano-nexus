//! Store-backed audit sink: decisions are queued without blocking the caller and written in batches
//! (every 500 ms or 100 entries, whichever comes first).

use std::sync::Arc;
use std::time::Duration;

use nexus_fs::{AuditEntry, AuditSink};
use nexus_store::{NewAudit, Store};
use tokio::sync::{mpsc, oneshot};

const FLUSH_EVERY: Duration = Duration::from_millis(500);
const FLUSH_AT: usize = 100;

enum Msg {
    Entry(AuditEntry),
    Flush(oneshot::Sender<()>),
}

/// Handle given to the permission engine. Cheap to clone via `Arc`.
pub struct AuditWriter {
    tx: mpsc::UnboundedSender<Msg>,
}

impl AuditWriter {
    /// Start the writer task (needs a tokio runtime).
    pub fn start(store: Arc<Store>) -> Arc<Self> {
        Self::start_with(store, FLUSH_EVERY, FLUSH_AT)
    }

    pub fn start_with(store: Arc<Store>, every: Duration, at: usize) -> Arc<Self> {
        let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
        tokio::spawn(async move {
            let mut batch: Vec<NewAudit> = Vec::new();
            let mut tick = tokio::time::interval(every);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    msg = rx.recv() => match msg {
                        Some(Msg::Entry(e)) => {
                            batch.push(NewAudit { ts: e.ts, actor: e.actor, action: e.action, path: e.path, allowed: e.allowed, detail: e.detail });
                            if batch.len() >= at {
                                write(&store, &mut batch).await;
                            }
                        }
                        Some(Msg::Flush(done)) => {
                            write(&store, &mut batch).await;
                            let _ = done.send(());
                        }
                        None => {
                            write(&store, &mut batch).await;
                            break;
                        }
                    },
                    _ = tick.tick() => write(&store, &mut batch).await,
                }
            }
        });
        Arc::new(Self { tx })
    }

    /// Wait until everything queued so far is on disk (call on shutdown and in tests).
    pub async fn flush(&self) {
        let (done, wait) = oneshot::channel();
        if self.tx.send(Msg::Flush(done)).is_ok() {
            let _ = wait.await;
        }
    }
}

async fn write(store: &Arc<Store>, batch: &mut Vec<NewAudit>) {
    if batch.is_empty() {
        return;
    }
    let rows = std::mem::take(batch);
    let store = Arc::clone(store);
    let res = tokio::task::spawn_blocking(move || store.insert_audit_batch(&rows)).await;
    match res {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!("could not write audit entries: {e}"),
        Err(e) => tracing::warn!("audit writer task failed: {e}"),
    }
}

impl AuditSink for AuditWriter {
    fn record(&self, entry: AuditEntry) {
        let _ = self.tx.send(Msg::Entry(entry));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(i: i64) -> AuditEntry {
        AuditEntry {
            ts: i,
            actor: "t".into(),
            action: "read".into(),
            path: format!("/p/{i}"),
            allowed: i % 2 == 0,
            detail: None,
        }
    }

    #[tokio::test]
    async fn entries_are_batched_flushed_and_ordered() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let w = AuditWriter::start_with(store.clone(), Duration::from_secs(60), 100);
        for i in 0..250 {
            w.record(entry(i));
        }
        w.flush().await;
        let rows = store.list_audit(1000, None).unwrap();
        assert_eq!(rows.len(), 250);
        // Newest first; ids follow arrival order.
        assert_eq!(rows[0].path, "/p/249");
        assert_eq!(rows[249].path, "/p/0");
        assert!(rows[0].allowed != rows[1].allowed);
    }

    #[tokio::test]
    async fn the_timer_flushes_small_batches_and_record_never_blocks() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let w = AuditWriter::start_with(store.clone(), Duration::from_millis(30), 100);
        let t = std::time::Instant::now();
        w.record(entry(1));
        assert!(t.elapsed() < Duration::from_millis(20));
        assert!(store.list_audit(10, None).unwrap().is_empty() || t.elapsed() > Duration::from_millis(30));
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(store.list_audit(10, None).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn dropping_the_writer_flushes_the_rest() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        {
            let w = AuditWriter::start_with(store.clone(), Duration::from_secs(60), 100);
            for i in 0..3 {
                w.record(entry(i));
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(store.list_audit(10, None).unwrap().len(), 3);
    }

    #[tokio::test]
    async fn works_as_the_engines_audit_sink() {
        use nexus_core::fs::Access;
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let store = Arc::new(Store::open_in_memory().unwrap());
        let w = AuditWriter::start_with(store.clone(), Duration::from_secs(60), 100);
        let engine = nexus_fs::PermissionEngine::new(nexus_fs::DenyList::default(), w.clone());
        let _ = engine.check("folder-scout", &root, Access::Read);
        w.flush().await;
        let rows = store.list_audit(10, None).unwrap();
        assert_eq!((rows[0].actor.as_str(), rows[0].allowed), ("folder-scout", false));
    }
}
