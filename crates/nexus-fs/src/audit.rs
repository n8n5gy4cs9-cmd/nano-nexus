//! Audit trail of every permission decision.

use std::sync::Mutex;

/// One decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditEntry {
    /// Unix milliseconds (UTC).
    pub ts: i64,
    /// Assistant id, `"editor"`, or `"user"`.
    pub actor: String,
    /// `read | write | create | move | trash`.
    pub action: String,
    pub path: String,
    pub allowed: bool,
    pub detail: Option<String>,
}

/// Where audit entries go. Implementations must not block (the store-backed one batches writes).
pub trait AuditSink: Send + Sync {
    fn record(&self, entry: AuditEntry);
}

/// Drops everything.
pub struct NullAudit;

impl AuditSink for NullAudit {
    fn record(&self, _entry: AuditEntry) {}
}

/// Collects entries in memory (tests).
#[derive(Default)]
pub struct MemoryAudit(pub Mutex<Vec<AuditEntry>>);

impl MemoryAudit {
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.0.lock().map(|v| v.clone()).unwrap_or_default()
    }
}

impl AuditSink for MemoryAudit {
    fn record(&self, entry: AuditEntry) {
        if let Ok(mut v) = self.0.lock() {
            v.push(entry);
        }
    }
}
