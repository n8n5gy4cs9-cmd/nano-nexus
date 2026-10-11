//! Watch deltas: compare two folder reports and decide whether the change is worth a notification
//! (ANALYZER §4). Pure functions; the scheduler and notification live in the app.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::fmt;
use crate::report::{FileRef, FolderReport};

/// Size change that always notifies (500 MB).
pub const SIZE_DELTA_BYTES: u64 = 500 * 1024 * 1024;
/// Relative size change that notifies (10 %), once the folder is big enough to matter.
pub const SIZE_DELTA_PCT: f64 = 0.10;
/// Below this the percentage rule is ignored (a 1 MB folder doubling is not news).
pub const MIN_BYTES_FOR_PCT: u64 = 50 * 1024 * 1024;
/// New files of at least this size notify on their own (1 GB).
pub const BIG_FILE_BYTES: u64 = 1024 * 1024 * 1024;

/// What changed between two scans of the same folder.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ReportDelta {
    pub root: String,
    pub prev_at: i64,
    pub new_at: i64,
    pub prev_bytes: u64,
    pub new_bytes: u64,
    /// `new_bytes - prev_bytes` (negative when the folder shrank).
    pub bytes_delta: i64,
    pub files_delta: i64,
    /// Files among the largest that were not there before.
    pub new_big_files: Vec<FileRef>,
    /// Duplicate groups (by hash) that did not exist before.
    pub new_duplicate_groups: u32,
    /// Bytes wasted by those new groups.
    pub new_duplicate_bytes: u64,
}

impl ReportDelta {
    pub fn new_big_files_bytes(&self) -> u64 {
        self.new_big_files.iter().map(|f| f.bytes).sum()
    }
}

/// Compute the delta from `prev` to `new`.
pub fn diff_reports(prev: &FolderReport, new: &FolderReport) -> ReportDelta {
    let prev_paths: HashSet<&str> = prev.largest_files.iter().map(|f| f.rel_path.as_str()).collect();
    // `largest_files` is capped, so a file that merely slipped into the list is not "new": it must
    // also have been modified since the previous scan.
    let new_big_files: Vec<FileRef> = new
        .largest_files
        .iter()
        .filter(|f| !prev_paths.contains(f.rel_path.as_str()))
        .filter(|f| f.modified.is_none_or(|m| m >= prev.scanned_at))
        .cloned()
        .collect();

    let prev_hashes: HashSet<&str> = prev.duplicates.iter().map(|g| g.hash.as_str()).collect();
    let fresh: Vec<_> = new
        .duplicates
        .iter()
        .filter(|g| !prev_hashes.contains(g.hash.as_str()))
        .collect();

    ReportDelta {
        root: new.root.clone(),
        prev_at: prev.scanned_at,
        new_at: new.scanned_at,
        prev_bytes: prev.totals.bytes,
        new_bytes: new.totals.bytes,
        bytes_delta: new.totals.bytes as i64 - prev.totals.bytes as i64,
        files_delta: new.totals.files as i64 - prev.totals.files as i64,
        new_big_files,
        new_duplicate_groups: fresh.len() as u32,
        new_duplicate_bytes: fresh.iter().map(|g| g.wasted()).sum(),
    }
}

/// Does the delta cross a threshold (size ≥ 500 MB or ≥ 10 %, a new file ≥ 1 GB, new duplicates)?
pub fn is_significant(d: &ReportDelta) -> bool {
    let grew = d.bytes_delta.unsigned_abs();
    let by_size = grew >= SIZE_DELTA_BYTES;
    let by_pct = d.prev_bytes >= MIN_BYTES_FOR_PCT && grew as f64 >= d.prev_bytes as f64 * SIZE_DELTA_PCT;
    let by_file = d.new_big_files.iter().any(|f| f.bytes >= BIG_FILE_BYTES);
    by_size || by_pct || by_file || d.new_duplicate_groups > 0
}

fn folder_name(root: &str) -> &str {
    root.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(root)
}

/// Notification text, e.g. `("Downloads grew by 3.2 GB", "2 new big files (2.4 GB). 1 new duplicate group (120 MB).")`.
pub fn notification_text(d: &ReportDelta) -> (String, String) {
    let name = folder_name(&d.root);
    let verb = if d.bytes_delta >= 0 { "grew" } else { "shrank" };
    let title = if d.bytes_delta == 0 {
        format!("{name} changed")
    } else {
        format!("{name} {verb} by {}", fmt::bytes(d.bytes_delta.unsigned_abs()))
    };
    let mut parts = Vec::new();
    if !d.new_big_files.is_empty() {
        let n = d.new_big_files.len();
        parts.push(format!(
            "{n} new big file{} ({}).",
            if n == 1 { "" } else { "s" },
            fmt::bytes(d.new_big_files_bytes())
        ));
    }
    if d.new_duplicate_groups > 0 {
        let n = d.new_duplicate_groups;
        parts.push(format!(
            "{n} new duplicate group{} ({} wasted).",
            if n == 1 { "" } else { "s" },
            fmt::bytes(d.new_duplicate_bytes)
        ));
    }
    if parts.is_empty() {
        parts.push(format!("Since {}.", fmt::date(d.prev_at)));
    }
    (title, parts.join(" "))
}

/// Is a watch due? `daily` after 24 h, `weekly` after 7 days; `on_change` is driven by file events
/// and never by the clock.
pub fn is_due(schedule: &str, last_run_ms: Option<i64>, now_ms: i64) -> bool {
    const DAY: i64 = 86_400_000;
    let every = match schedule {
        "daily" => DAY,
        "weekly" => 7 * DAY,
        _ => return false,
    };
    last_run_ms.is_none_or(|t| now_ms - t >= every)
}

/// Collects file-change events and says when the quiet period (10 min) has passed.
#[derive(Clone, Debug, Default)]
pub struct Debounce {
    last_event_ms: Option<i64>,
}

impl Debounce {
    pub const QUIET_MS: i64 = 10 * 60 * 1000;

    pub fn event(&mut self, now_ms: i64) {
        self.last_event_ms = Some(now_ms);
    }

    /// `true` once, when events stopped for [`Self::QUIET_MS`]; resets the state.
    pub fn take_if_quiet(&mut self, now_ms: i64) -> bool {
        match self.last_event_ms {
            Some(t) if now_ms - t >= Self::QUIET_MS => {
                self.last_event_ms = None;
                true
            }
            _ => false,
        }
    }
}

/// A running file-system watcher; dropping it stops watching.
pub struct FolderWatcher {
    _inner: notify::RecommendedWatcher,
}

/// Watch `root` recursively and call `on_event` for every change under it (the caller debounces with
/// [`Debounce`]). Refuses the home folder or a root folder: watching those is far too noisy.
pub fn watch_folder(
    root: &std::path::Path,
    home: Option<&std::path::Path>,
    on_event: impl Fn() + Send + 'static,
) -> Result<FolderWatcher, String> {
    use notify::{RecursiveMode, Watcher};
    if root.parent().is_none() || home.is_some_and(|h| h == root) {
        return Err("Watching a whole home or root folder is too noisy; pick a subfolder.".into());
    }
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok_and(|e| !e.kind.is_access()) {
            on_event();
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;
    Ok(FolderWatcher { _inner: watcher })
}
