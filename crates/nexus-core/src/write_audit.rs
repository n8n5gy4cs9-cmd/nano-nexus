//! Bounded record of application-controlled writes, kept on the same drive as the app.
//!
//! This lists operations the app chose to record, plus the destinations it configured for
//! workers and caches. It is not a filesystem monitor: OS swap, crash reports, security
//! databases and driver caches are not observed, and worker-internal writes are only
//! known as configured destinations. Contents, tokens and secrets are never recorded.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// In-memory ring size; the file keeps older entries until rotation.
const RING: usize = 500;
/// Rotate the JSONL file at this size and keep one previous generation.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_ERROR_CHARS: usize = 200;

/// One recorded write operation. Repeats of the same operation are merged into `count`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct WriteEvent {
    pub ts_ms: i64,
    pub subsystem: String,
    pub operation: String,
    /// Root-relative (slash separated) when inside the distribution, otherwise absolute.
    pub destination: String,
    pub outside_root: bool,
    pub bytes: Option<u64>,
    pub ok: bool,
    /// Short failure reason, truncated; never file contents.
    pub error: Option<String>,
    pub count: u32,
}

/// A destination the app configured for writes it does not itemise (worker caches, temp).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ConfiguredDestination {
    pub subsystem: String,
    pub path: String,
    pub outside_root: bool,
}

/// Everything the storage view shows, with the limits stated alongside.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct WriteReport {
    pub recorded: Vec<WriteEvent>,
    pub configured: Vec<ConfiguredDestination>,
    pub gaps: Vec<String>,
    pub log_path: Option<String>,
}

/// Plain statements of what this record cannot see.
pub fn coverage_gaps() -> Vec<String> {
    [
        "Only operations the app records are listed. This is not a system-wide file monitor.",
        "Configured destinations show where workers are told to write; their individual writes are not itemised.",
        "OS-managed writes (swap, crash reports, security scans, driver and shader caches outside configured paths) are not observed.",
        "Edits to your own documents are governed by the permission and undo journal and are recorded only when applied.",
    ]
    .map(String::from)
    .to_vec()
}

/// Append-only log with a memory ring. Failure to log never fails the audited operation.
pub struct WriteLog {
    file: PathBuf,
    root: Option<PathBuf>,
    ring: Mutex<VecDeque<WriteEvent>>,
}

impl WriteLog {
    pub fn new(file: PathBuf, root: Option<PathBuf>) -> Self {
        Self {
            file,
            root,
            ring: Mutex::new(VecDeque::new()),
        }
    }

    fn describe(&self, dest: &Path) -> (String, bool) {
        match self.root.as_deref().and_then(|r| dest.strip_prefix(r).ok()) {
            Some(rel) => (
                rel.components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/"),
                false,
            ),
            None => (dest.display().to_string(), self.root.is_some()),
        }
    }

    /// Record one operation. Writes to the audit log itself are ignored (no self-audit).
    pub fn record(
        &self,
        subsystem: &str,
        operation: &str,
        dest: &Path,
        bytes: Option<u64>,
        result: Result<(), &str>,
    ) {
        if dest.starts_with(&self.file) || dest == self.file.with_extension("jsonl.1") {
            return;
        }
        let (destination, outside_root) = self.describe(dest);
        let error = result
            .err()
            .map(|e| e.chars().take(MAX_ERROR_CHARS).collect::<String>());
        let mut event = WriteEvent {
            ts_ms: now_ms(),
            subsystem: subsystem.into(),
            operation: operation.into(),
            destination,
            outside_root,
            bytes,
            ok: error.is_none(),
            error,
            count: 1,
        };
        let Ok(mut ring) = self.ring.lock() else {
            return;
        };
        if let Some(last) = ring.back_mut()
            && last.subsystem == event.subsystem
            && last.operation == event.operation
            && last.destination == event.destination
            && last.ok == event.ok
            && last.error == event.error
        {
            last.count = last.count.saturating_add(1);
            last.bytes = match (last.bytes, event.bytes) {
                (Some(a), Some(b)) => Some(a.saturating_add(b)),
                _ => None,
            };
            last.ts_ms = event.ts_ms;
            event = last.clone();
        } else {
            ring.push_back(event.clone());
            if ring.len() > RING {
                ring.pop_front();
            }
        }
        drop(ring);
        self.append(&event);
    }

    fn append(&self, event: &WriteEvent) {
        let Ok(line) = serde_json::to_string(event) else {
            return;
        };
        if std::fs::metadata(&self.file).is_ok_and(|m| m.len() >= MAX_FILE_BYTES) {
            let _ = std::fs::rename(&self.file, self.file.with_extension("jsonl.1"));
        }
        if let Some(parent) = self.file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)
        {
            let _ = writeln!(f, "{line}");
        }
    }

    /// Newest first.
    pub fn recent(&self, limit: usize) -> Vec<WriteEvent> {
        self.ring
            .lock()
            .map(|r| r.iter().rev().take(limit).cloned().collect())
            .unwrap_or_default()
    }

    /// Classify configured destinations against the distribution root.
    pub fn configured(&self, entries: Vec<(String, PathBuf)>) -> Vec<ConfiguredDestination> {
        entries
            .into_iter()
            .map(|(subsystem, path)| {
                let (path, outside_root) = self.describe(&path);
                ConfiguredDestination {
                    subsystem,
                    path,
                    outside_root,
                }
            })
            .collect()
    }

    pub fn file(&self) -> &Path {
        &self.file
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

static LOG: OnceLock<WriteLog> = OnceLock::new();

/// Install the process-wide log once at startup; later calls are ignored.
pub fn init(file: PathBuf, root: Option<PathBuf>) {
    let _ = LOG.set(WriteLog::new(file, root));
}

/// The process-wide log, absent before `init` (unit tests, tools).
pub fn global() -> Option<&'static WriteLog> {
    LOG.get()
}

/// Record against the global log; a no-op when none is installed.
pub fn record(subsystem: &str, operation: &str, dest: &Path, bytes: Option<u64>, result: Result<(), &str>) {
    if let Some(log) = LOG.get() {
        log.record(subsystem, operation, dest, bytes, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_repeats_redacts_nothing_but_truncates_and_flags_outside() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path().join("dist");
        let log = WriteLog::new(root.join("logs/write-audit.jsonl"), Some(root.clone()));
        let out = root.join("outputs/a.png");
        log.record("media", "save", &out, Some(10), Ok(()));
        log.record("media", "save", &out, Some(5), Ok(()));
        log.record(
            "media",
            "save",
            Path::new("/elsewhere/x"),
            None,
            Err(&"e".repeat(500)),
        );
        let recent = log.recent(10);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[1].count, 2);
        assert_eq!(recent[1].bytes, Some(15));
        assert_eq!(recent[1].destination, "outputs/a.png");
        assert!(!recent[1].outside_root);
        assert!(recent[0].outside_root);
        assert_eq!(recent[0].error.as_deref().map(str::len), Some(MAX_ERROR_CHARS));
        assert!(!recent[0].ok);
    }

    #[test]
    fn never_records_its_own_file_and_rotates() {
        let dir = tempfile::tempdir().expect("tmp");
        let file = dir.path().join("w.jsonl");
        let log = WriteLog::new(file.clone(), None);
        log.record("audit", "append", &file, None, Ok(()));
        assert!(log.recent(5).is_empty());
        std::fs::write(&file, vec![b'x'; MAX_FILE_BYTES as usize]).expect("fill");
        log.record("s", "o", Path::new("/a"), None, Ok(()));
        assert!(dir.path().join("w.jsonl.1").exists());
        assert!(std::fs::metadata(&file).expect("new").len() < MAX_FILE_BYTES);
    }

    #[test]
    fn ring_is_bounded() {
        let dir = tempfile::tempdir().expect("tmp");
        let log = WriteLog::new(dir.path().join("w.jsonl"), None);
        for i in 0..(RING + 20) {
            log.record("s", "o", Path::new(&format!("/p/{i}")), None, Ok(()));
        }
        assert_eq!(log.recent(usize::MAX).len(), RING);
    }
}
