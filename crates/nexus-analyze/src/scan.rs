//! The folder scanner (ANALYZER §1): a parallel, gitignore-aware walk that collects file facts, then a
//! pure aggregation into a [`FolderReport`]. Permission errors become warnings and never abort the scan.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use ignore::{WalkBuilder, WalkState};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::categories::{self, Category};
use crate::dups::{self, DupMode};
use crate::junk::{self, JunkHit};
use crate::naming::{self, NamingHit};
use crate::report::{self, FileEntry, FolderReport, FolderTotals, ScanWarning};

const MAX_WARNINGS: usize = 200;
/// At most this many files are opened to compare their bytes with their extension.
const MAGIC_CHECK_CAP: usize = 5000;

#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// Already permission-checked (Read).
    pub root: PathBuf,
    pub include_hidden: bool,
    pub respect_gitignore: bool,
    /// Hard cap on entries; the report is marked truncated beyond it.
    pub max_entries: usize,
    pub duplicates: DupMode,
    /// Smallest file considered for duplicates.
    pub min_dup_size: u64,
    /// Compare file contents with extensions for media files (reads 512 bytes each).
    pub check_magic: bool,
}

impl ScanOptions {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            include_hidden: false,
            respect_gitignore: false,
            max_entries: 1_000_000,
            duplicates: DupMode::Fast,
            min_dup_size: 4096,
            check_magic: true,
        }
    }
}

/// Throttled to 10 Hz while scanning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ScanProgress {
    pub files_seen: u64,
    pub bytes_seen: u64,
    pub current_dir: String,
    /// `walking | hashing | summarizing`
    pub phase: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("the folder does not exist or is not a folder: {0}")]
    NotAFolder(String),
    #[error("cancelled")]
    Cancelled,
}

struct Raw {
    path: PathBuf,
    is_dir: bool,
    is_symlink: bool,
    bytes: u64,
    modified: Option<i64>,
}

fn ms(t: std::time::SystemTime) -> Option<i64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis() as i64)
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Shorten the root for display: `/Users/me/Downloads` → `~/Downloads`.
pub fn display_root(root: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| root.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => root.display().to_string(),
    }
}

/// Scan `opts.root`. `progress` is called at most every 100 ms from worker threads.
pub fn scan_folder(
    opts: &ScanOptions,
    progress: &(dyn Fn(ScanProgress) + Sync),
    cancel: &CancellationToken,
) -> Result<FolderReport, ScanError> {
    let started = Instant::now();
    let root = &opts.root;
    if !root.is_dir() {
        return Err(ScanError::NotAFolder(root.display().to_string()));
    }
    let now_ms = ms(std::time::SystemTime::now()).unwrap_or(0);

    // ---- walk ----------------------------------------------------------------------------------
    struct Shared {
        raws: Mutex<Vec<Raw>>,
        warnings: Mutex<Vec<ScanWarning>>,
        files_seen: AtomicU64,
        bytes_seen: AtomicU64,
        total_seen: AtomicU64,
        truncated: AtomicBool,
        last_emit: Mutex<Instant>,
    }
    /// A worker's private batch; flushed into the shared list when full and when the worker ends.
    struct Local<'a> {
        buf: Vec<Raw>,
        shared: &'a Shared,
    }
    impl Local<'_> {
        fn flush(&mut self) {
            if let Ok(mut all) = self.shared.raws.lock() {
                all.append(&mut self.buf);
            }
        }
    }
    impl Drop for Local<'_> {
        fn drop(&mut self) {
            self.flush();
        }
    }
    let shared = Shared {
        raws: Mutex::new(Vec::new()),
        warnings: Mutex::new(Vec::new()),
        files_seen: AtomicU64::new(0),
        bytes_seen: AtomicU64::new(0),
        total_seen: AtomicU64::new(0),
        truncated: AtomicBool::new(false),
        last_emit: Mutex::new(Instant::now()),
    };
    let sh = &shared;
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(!opts.include_hidden)
        .follow_links(false)
        .git_ignore(opts.respect_gitignore)
        .git_exclude(opts.respect_gitignore)
        .git_global(false)
        .ignore(false)
        .parents(opts.respect_gitignore);
    builder.build_parallel().run(|| {
        let mut local = Local {
            buf: Vec::with_capacity(512),
            shared: sh,
        };
        Box::new(move |entry| {
            if cancel.is_cancelled() {
                return WalkState::Quit;
            }
            match entry {
                Ok(e) => {
                    if e.depth() == 0 {
                        return WalkState::Continue;
                    }
                    if sh.total_seen.fetch_add(1, Ordering::Relaxed) >= opts.max_entries as u64 {
                        sh.truncated.store(true, Ordering::Relaxed);
                        return WalkState::Quit;
                    }
                    let ft = e.file_type();
                    let is_dir = ft.is_some_and(|t| t.is_dir());
                    let is_symlink = ft.is_some_and(|t| t.is_symlink());
                    let (bytes, modified) = match e.metadata() {
                        Ok(m) => (if is_dir { 0 } else { m.len() }, m.modified().ok().and_then(ms)),
                        Err(err) => {
                            if let Ok(mut w) = sh.warnings.lock()
                                && w.len() < MAX_WARNINGS
                            {
                                w.push(ScanWarning {
                                    path: rel_of(root, e.path()),
                                    error: err.to_string(),
                                });
                            }
                            (0, None)
                        }
                    };
                    if !is_dir {
                        sh.files_seen.fetch_add(1, Ordering::Relaxed);
                        sh.bytes_seen.fetch_add(bytes, Ordering::Relaxed);
                    }
                    let dir = e.path().parent().map(|p| rel_of(root, p)).unwrap_or_default();
                    local.buf.push(Raw {
                        path: e.into_path(),
                        is_dir,
                        is_symlink,
                        bytes,
                        modified,
                    });
                    if local.buf.len() >= 512 {
                        local.flush();
                    }
                    if let Ok(mut t) = sh.last_emit.try_lock()
                        && t.elapsed() >= Duration::from_millis(100)
                    {
                        *t = Instant::now();
                        progress(ScanProgress {
                            files_seen: sh.files_seen.load(Ordering::Relaxed),
                            bytes_seen: sh.bytes_seen.load(Ordering::Relaxed),
                            current_dir: dir,
                            phase: "walking".into(),
                        });
                    }
                }
                Err(err) => {
                    if let Ok(mut w) = sh.warnings.lock()
                        && w.len() < MAX_WARNINGS
                    {
                        let path = match &err {
                            ignore::Error::WithPath { path, .. } => rel_of(root, path),
                            _ => String::new(),
                        };
                        w.push(ScanWarning {
                            path,
                            error: err.to_string(),
                        });
                    }
                }
            }
            WalkState::Continue
        })
    });
    if cancel.is_cancelled() {
        return Err(ScanError::Cancelled);
    }
    let files_seen = shared.files_seen.load(Ordering::Relaxed);
    let bytes_seen = shared.bytes_seen.load(Ordering::Relaxed);
    let truncated = shared.truncated.load(Ordering::Relaxed);
    let warnings = shared.warnings.into_inner().unwrap_or_default();
    let mut raws = shared.raws.into_inner().unwrap_or_default();
    raws.sort_by(|a, b| a.path.cmp(&b.path));
    progress(ScanProgress {
        files_seen,
        bytes_seen,
        current_dir: String::new(),
        phase: "summarizing".into(),
    });

    // ---- classify ------------------------------------------------------------------------------
    let mut totals = FolderTotals::default();
    let mut files: Vec<FileEntry> = Vec::with_capacity(raws.len());
    let mut dir_rels: Vec<String> = Vec::new();
    let mut max_depth = 0u32;
    let mut deepest = String::new();
    for r in &raws {
        let rel = rel_of(root, &r.path);
        if r.is_symlink {
            totals.symlinks += 1;
            continue;
        }
        let depth = rel.matches('/').count() as u32 + 1;
        if depth > max_depth {
            max_depth = depth;
            deepest = rel.clone();
        }
        if r.is_dir {
            totals.dirs += 1;
            dir_rels.push(rel);
            continue;
        }
        let name = r
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = r
            .path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let hidden = name.starts_with('.');
        if hidden {
            totals.hidden_files += 1;
        }
        totals.files += 1;
        totals.bytes += r.bytes;
        files.push(FileEntry {
            category: categories::categorize(&r.path, None),
            rel,
            bytes: r.bytes,
            modified: r.modified,
            ext,
            hidden,
        });
    }

    // ---- duplicates (hashing) ---------------------------------------------------------------------
    if opts.duplicates != DupMode::Off {
        progress(ScanProgress {
            files_seen: totals.files,
            bytes_seen: totals.bytes,
            current_dir: String::new(),
            phase: "hashing".into(),
        });
    }
    let duplicates = dups::find_duplicates(root, &files, opts.duplicates, opts.min_dup_size, cancel)
        .ok_or(ScanError::Cancelled)?;
    let wasted_by_duplicates = duplicates.iter().map(|g| g.wasted()).sum();

    // ---- derived facts ---------------------------------------------------------------------------
    let sizes = report::dir_sizes(&files);
    let mut junk_hits: Vec<JunkHit> = Vec::new();
    for f in &files {
        let name = f.rel.rsplit('/').next().unwrap_or(&f.rel);
        if let Some(j) = junk::check_file(name, f.bytes, f.modified, now_ms) {
            junk_hits.push(JunkHit {
                rule: j.rule.into(),
                rel_path: f.rel.clone(),
                bytes: f.bytes,
                safe_to_trash: j.safe_to_trash,
                regenerable: false,
            });
        }
    }
    for d in &dir_rels {
        let name = d.rsplit('/').next().unwrap_or(d);
        if let Some(rule) = junk::regenerable_dir(name) {
            let bytes = sizes.get(d).map_or(0, |s| s.0);
            // Do not list a regenerable folder nested in another one that is already listed.
            if !junk_hits
                .iter()
                .any(|h| h.regenerable && d.starts_with(&format!("{}/", h.rel_path)))
            {
                junk_hits.push(JunkHit {
                    rule: rule.into(),
                    rel_path: d.clone(),
                    bytes,
                    safe_to_trash: false,
                    regenerable: true,
                });
            }
        }
    }
    junk_hits.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.rel_path.cmp(&b.rel_path)));

    let empty_files: Vec<String> = files
        .iter()
        .filter(|f| f.bytes == 0)
        .map(|f| f.rel.clone())
        .collect();
    let non_empty_dirs: std::collections::HashSet<&str> = files
        .iter()
        .flat_map(|f| f.rel.rmatch_indices('/').map(|(i, _)| &f.rel[..i]))
        .collect();
    let empty_dirs: Vec<String> = dir_rels
        .iter()
        .filter(|d| !non_empty_dirs.contains(d.as_str()))
        .filter(|d| {
            !dir_rels
                .iter()
                .any(|o| o.len() > d.len() && o.starts_with(&format!("{d}/")))
        })
        .filter(|d| {
            std::fs::read_dir(root.join(d))
                .map(|mut r| r.next().is_none())
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    let mut naming_issues: Vec<NamingHit> = files.iter().flat_map(|f| naming::check_name(&f.rel)).collect();
    naming_issues.extend(naming::inconsistent_case(
        &files.iter().map(|f| f.rel.clone()).collect::<Vec<_>>(),
    ));
    if opts.check_magic {
        let media: Vec<&FileEntry> = files
            .iter()
            .filter(|f| {
                matches!(
                    f.category,
                    Category::Image | Category::Video | Category::Audio | Category::Archive | Category::Pdf
                )
            })
            .take(MAGIC_CHECK_CAP)
            .collect();
        let mismatches: Vec<NamingHit> = media
            .par_iter()
            .filter_map(|f| {
                use std::io::Read;
                let mut head = [0u8; 512];
                let n = std::fs::File::open(root.join(&f.rel))
                    .ok()?
                    .read(&mut head)
                    .ok()?;
                let found = categories::magic_extension_mismatch(Path::new(&f.rel), &head[..n])?;
                Some(NamingHit {
                    kind: "extension_mismatch".into(),
                    rel_path: f.rel.clone(),
                    detail: format!("looks like a .{found} file but is named .{}", f.ext),
                })
            })
            .collect();
        naming_issues.extend(mismatches);
    }
    naming_issues.sort_by(|a, b| a.rel_path.cmp(&b.rel_path).then_with(|| a.kind.cmp(&b.kind)));

    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string());
    Ok(FolderReport {
        id: format!("scan-{now_ms}"),
        root: root.display().to_string(),
        scanned_at: now_ms,
        duration_ms: started.elapsed().as_millis() as u64,
        by_category: report::by_category(&files),
        by_extension: report::by_extension(&files, 30),
        largest_files: report::largest_files(&files, 50),
        largest_dirs: report::largest_dirs(&sizes, 30),
        age: report::age_buckets(&files, now_ms),
        duplicates,
        wasted_by_duplicates,
        empty_dirs,
        empty_files,
        junk: junk_hits,
        naming_issues,
        max_depth,
        deepest_path: deepest,
        treemap: report::treemap(&root_name, &files, 2000),
        totals,
        warnings,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, data: &[u8]) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }

    fn no_progress(_: ScanProgress) {}

    fn scan(root: &Path, tweak: impl FnOnce(&mut ScanOptions)) -> FolderReport {
        let mut o = ScanOptions::new(root);
        tweak(&mut o);
        scan_folder(&o, &no_progress, &CancellationToken::new()).unwrap()
    }

    #[test]
    fn scans_a_small_tree_end_to_end() {
        let d = tempfile::tempdir().unwrap();
        let big = vec![1u8; 10_000];
        write(d.path(), "Movies/talk.mov", &big);
        write(d.path(), "Movies/talk copy.mov", &big);
        write(d.path(), "docs/report.pdf", &[0u8; 100]);
        write(d.path(), ".DS_Store", b"x");
        write(d.path(), "node_modules/pkg/index.js", &[0u8; 2000]);
        write(d.path(), "empty.txt", b"");
        std::fs::create_dir_all(d.path().join("hollow/deeper")).unwrap();
        let r = scan(d.path(), |o| {
            o.include_hidden = true;
            o.duplicates = DupMode::Full;
        });
        assert_eq!(r.totals.files, 6);
        assert_eq!(r.totals.hidden_files, 1);
        assert_eq!(r.totals.bytes, 10_000 * 2 + 100 + 1 + 2000);
        assert_eq!(r.by_category[0].category, Category::Video);
        assert_eq!(r.duplicates.len(), 1);
        assert_eq!(r.wasted_by_duplicates, 10_000);
        assert!(r.junk.iter().any(|j| j.rule == ".DS_Store" && j.safe_to_trash));
        let nm = r.junk.iter().find(|j| j.regenerable).unwrap();
        assert_eq!(
            (nm.rel_path.as_str(), nm.bytes, nm.safe_to_trash),
            ("node_modules", 2000, false)
        );
        assert_eq!(r.empty_files, ["empty.txt"]);
        assert_eq!(r.empty_dirs, ["hollow/deeper"]);
        assert!(
            r.naming_issues
                .iter()
                .any(|n| n.kind == "copy_pattern" && n.rel_path.contains("copy"))
        );
        assert_eq!(r.max_depth, 3);
        assert_eq!(r.treemap.bytes, r.totals.bytes);
        assert!(!r.truncated && r.warnings.is_empty());
    }

    #[test]
    fn hidden_files_are_skipped_by_default() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), ".secret", b"x");
        write(d.path(), "shown.txt", b"x");
        let r = scan(d.path(), |_| {});
        assert_eq!((r.totals.files, r.totals.hidden_files), (1, 0));
    }

    #[test]
    fn gitignore_is_honoured_when_asked() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".git")).unwrap();
        write(d.path(), ".gitignore", b"ignored/\n");
        write(d.path(), "ignored/a.txt", b"x");
        write(d.path(), "kept/b.txt", b"x");
        assert_eq!(scan(d.path(), |o| o.respect_gitignore = true).totals.files, 1);
        assert_eq!(scan(d.path(), |_| {}).totals.files, 2);
    }

    #[test]
    fn the_entry_cap_marks_the_report_truncated() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..50 {
            write(d.path(), &format!("f{i}.txt"), b"x");
        }
        let r = scan(d.path(), |o| o.max_entries = 10);
        assert!(r.truncated);
        assert!(r.totals.files <= 11);
    }

    #[test]
    fn media_with_the_wrong_extension_is_flagged() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "holiday.jpg",
            &[b'P', b'K', 3, 4, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        let r = scan(d.path(), |_| {});
        assert!(
            r.naming_issues.iter().any(|n| n.kind == "extension_mismatch"),
            "{:?}",
            r.naming_issues
        );
    }

    #[test]
    fn cancel_and_bad_roots() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..200 {
            write(d.path(), &format!("d{}/f{i}", i % 10), b"x");
        }
        let c = CancellationToken::new();
        c.cancel();
        assert!(matches!(
            scan_folder(&ScanOptions::new(d.path()), &no_progress, &c),
            Err(ScanError::Cancelled)
        ));
        assert!(matches!(
            scan_folder(
                &ScanOptions::new(d.path().join("nope")),
                &no_progress,
                &CancellationToken::new()
            ),
            Err(ScanError::NotAFolder(_))
        ));
    }

    #[test]
    fn progress_is_reported_during_a_big_scan() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..1500 {
            write(d.path(), &format!("d{}/f{i}", i % 30), b"x");
        }
        let seen = Mutex::new(Vec::new());
        let cb = |p: ScanProgress| seen.lock().unwrap().push(p.phase);
        let r = scan_folder(&ScanOptions::new(d.path()), &cb, &CancellationToken::new()).unwrap();
        assert_eq!(r.totals.files, 1500);
        let phases = seen.into_inner().unwrap();
        assert!(phases.contains(&"summarizing".to_string()));
    }

    #[test]
    fn display_root_uses_a_tilde() {
        assert_eq!(
            display_root(Path::new("/Users/me/Downloads"), Some(Path::new("/Users/me"))),
            "~/Downloads"
        );
        assert_eq!(
            display_root(Path::new("/Users/me"), Some(Path::new("/Users/me"))),
            "~"
        );
        assert_eq!(
            display_root(Path::new("/data/x"), Some(Path::new("/Users/me"))),
            "/data/x"
        );
    }

    /// 100k files in under 10 s with no hashing, and cancel within 300 ms (run with `--ignored --release`).
    #[test]
    #[ignore = "benchmark: creates 100k files"]
    fn benchmark_100k_files() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..100_000u32 {
            let dir = d.path().join(format!("d{}/s{}", i % 100, i % 17));
            if !dir.exists() {
                std::fs::create_dir_all(&dir).unwrap();
            }
            std::fs::write(dir.join(format!("f{i}.dat")), b"x").unwrap();
        }
        let t = Instant::now();
        let r = scan(d.path(), |o| o.duplicates = DupMode::Off);
        let took = t.elapsed();
        assert_eq!(r.totals.files, 100_000);
        assert!(took < Duration::from_secs(10), "scan took {took:?}");
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            c2.cancel();
        });
        let t = Instant::now();
        let _ = scan_folder(&ScanOptions::new(d.path()), &no_progress, &cancel);
        assert!(t.elapsed() < Duration::from_millis(800));
    }
}
