//! Index job body (pure, synchronous; the app runs it on the blocking pool as a Background job).
//! Incremental by content hash, cancellable, with progress callbacks and size/time limits.

use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use nexus_store::Store;
use tokio_util::sync::CancellationToken;

use crate::chunk::{ChunkOpts, chunk_document};
use crate::extract::{extract_file, list_files};
use crate::repo::{self, NewDocument};
use crate::types::KbError;

/// Limits for one run (defaults from M17: 2 GB/library, 50 MB/file).
#[derive(Clone, Debug)]
pub struct IndexOpts {
    pub max_file_bytes: u64,
    pub max_library_bytes: u64,
    /// Extraction + chunking slower than this marks the file `skipped` (checked after extraction;
    /// parsers cannot be interrupted).
    pub max_file_time: Duration,
    pub chunk: ChunkOpts,
}

impl Default for IndexOpts {
    fn default() -> Self {
        Self {
            max_file_bytes: 50 * 1024 * 1024,
            max_library_bytes: 2 * 1024 * 1024 * 1024,
            max_file_time: Duration::from_secs(120),
            chunk: ChunkOpts::default(),
        }
    }
}

/// Progress snapshot sent after every file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexProgress {
    pub files_done: usize,
    pub files_total: usize,
    pub current: String,
    pub chunks: usize,
    pub skipped: usize,
}

/// What a run did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexSummary {
    pub indexed: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub failed: usize,
    pub removed: usize,
    pub chunks: usize,
    /// `true` when the library size cap stopped the run early.
    pub truncated: bool,
}

fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex().to_string())
}

/// Index (or incrementally re-index) the library's folder. Files whose hash is unchanged are left alone;
/// files that vanished are removed. Returns [`KbError::Cancelled`] if `cancel` fires (work already
/// committed stays committed, so the next run resumes cheaply).
pub fn index_library(
    store: &Store,
    library_id: &str,
    opts: &IndexOpts,
    cancel: &CancellationToken,
    mut progress: impl FnMut(&IndexProgress),
) -> Result<IndexSummary, KbError> {
    let lib = repo::get_library(store, library_id)?;
    let root = Path::new(&lib.path);
    if !root.is_dir() {
        return Err(KbError::Invalid(format!(
            "library folder `{}` does not exist",
            lib.path
        )));
    }
    let files = list_files(root);
    let existing = repo::list_documents(store, library_id)?;
    let mut sum = IndexSummary::default();
    let mut prog = IndexProgress {
        files_total: files.len(),
        ..Default::default()
    };
    let mut total_bytes: u64 = existing
        .iter()
        .filter(|d| !files.iter().any(|(_, rel)| rel == &d.path))
        .map(|d| d.bytes.max(0) as u64)
        .sum();

    for (path, rel) in &files {
        if cancel.is_cancelled() {
            repo::refresh_counts(store, library_id)?;
            return Err(KbError::Cancelled);
        }
        prog.current = rel.clone();
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let hash = match hash_file(path) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(file = %rel, error = %e, "knowledge: cannot read file");
                sum.failed += 1;
                prog.files_done += 1;
                progress(&prog);
                continue;
            }
        };
        let prev = existing.iter().find(|d| &d.path == rel);
        if prev.is_some_and(|d| d.hash == hash) {
            sum.unchanged += 1;
            total_bytes += size;
            prog.files_done += 1;
            progress(&prog);
            continue;
        }
        let stub = |status: &str, kind: &str, note: String| NewDocument {
            path: rel.clone(),
            title: None,
            hash: hash.clone(),
            kind: kind.to_owned(),
            pages: 0,
            bytes: size as i64,
            status: status.to_owned(),
            note: Some(note),
        };
        if total_bytes + size > opts.max_library_bytes {
            sum.truncated = true;
            let doc = stub(
                "skipped",
                "unknown",
                "library size limit reached; file not indexed".into(),
            );
            repo::replace_document(store, library_id, &doc, &[])?;
            sum.skipped += 1;
            prog.skipped += 1;
            prog.files_done += 1;
            progress(&prog);
            continue;
        }
        if size > opts.max_file_bytes {
            let note = format!("larger than {} MB; skipped", opts.max_file_bytes / 1024 / 1024);
            repo::replace_document(store, library_id, &stub("skipped", "unknown", note), &[])?;
            sum.skipped += 1;
            prog.skipped += 1;
            prog.files_done += 1;
            progress(&prog);
            continue;
        }
        let started = Instant::now();
        match extract_file(path) {
            Ok(x) if started.elapsed() > opts.max_file_time => {
                let note = format!(
                    "took longer than {} s to read; skipped",
                    opts.max_file_time.as_secs()
                );
                repo::replace_document(store, library_id, &stub("skipped", &x.kind, note), &[])?;
                sum.skipped += 1;
                prog.skipped += 1;
            }
            Ok(x) => {
                let title = x.title.clone().unwrap_or_else(|| {
                    Path::new(rel)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| rel.clone())
                });
                let chunks = chunk_document(&title, &x.pages, &opts.chunk);
                let mut notes = x.notes.clone();
                if chunks.is_empty() {
                    notes.push("no readable text".into());
                }
                let stored = NewDocument {
                    path: rel.clone(),
                    title: Some(title),
                    hash: hash.clone(),
                    kind: x.kind.clone(),
                    pages: x.pages.iter().filter_map(|p| p.number).max().unwrap_or(0) as i64,
                    bytes: size as i64,
                    status: if chunks.is_empty() { "skipped" } else { "indexed" }.into(),
                    note: (!notes.is_empty()).then(|| notes.join("; ")),
                };
                repo::replace_document(store, library_id, &stored, &chunks)?;
                total_bytes += size;
                prog.chunks += chunks.len();
                sum.chunks += chunks.len();
                if chunks.is_empty() {
                    sum.skipped += 1;
                    prog.skipped += 1;
                } else {
                    sum.indexed += 1;
                }
            }
            Err(e) => {
                repo::replace_document(store, library_id, &stub("failed", "unknown", e.to_string()), &[])?;
                sum.failed += 1;
            }
        }
        prog.files_done += 1;
        progress(&prog);
    }

    for d in &existing {
        if !files.iter().any(|(_, rel)| rel == &d.path) {
            repo::remove_document(store, library_id, &d.path)?;
            sum.removed += 1;
        }
    }
    repo::refresh_counts(store, library_id)?;
    Ok(sum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LibrarySource;

    fn setup(files: &[(&str, &str)]) -> (tempfile::TempDir, Store, String) {
        let dir = tempfile::tempdir().unwrap();
        for (n, t) in files {
            let p = dir.path().join(n);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(p, t).unwrap();
        }
        let s = Store::open_in_memory().unwrap();
        let l = repo::create_library(
            &s,
            "L",
            LibrarySource::User,
            &dir.path().to_string_lossy(),
            "reference",
        )
        .unwrap();
        (dir, s, l.id)
    }

    fn run(s: &Store, id: &str) -> IndexSummary {
        index_library(s, id, &IndexOpts::default(), &CancellationToken::new(), |_| {}).unwrap()
    }

    #[test]
    fn indexes_then_is_incremental() {
        let (dir, s, id) = setup(&[("a.md", "# A\n\nalpha text"), ("sub/b.txt", "beta text")]);
        let first = run(&s, &id);
        assert_eq!((first.indexed, first.unchanged), (2, 0));
        let second = run(&s, &id);
        assert_eq!((second.indexed, second.unchanged), (0, 2));
        std::fs::write(dir.path().join("a.md"), "# A\n\nchanged text").unwrap();
        std::fs::remove_file(dir.path().join("sub/b.txt")).unwrap();
        let third = run(&s, &id);
        assert_eq!((third.indexed, third.unchanged, third.removed), (1, 0, 1));
        let lib = repo::get_library(&s, &id).unwrap();
        assert_eq!(lib.doc_count, 1);
        assert!(lib.chunk_count >= 1);
    }

    #[test]
    fn oversize_and_broken_files_are_noted_not_fatal() {
        let (_d, s, id) = setup(&[
            ("big.md", &"x ".repeat(100)),
            ("bad.json", "{nope"),
            ("ok.md", "fine"),
        ]);
        let opts = IndexOpts {
            max_file_bytes: 50,
            ..Default::default()
        };
        let sum = index_library(&s, &id, &opts, &CancellationToken::new(), |_| {}).unwrap();
        assert_eq!((sum.indexed, sum.skipped, sum.failed), (1, 1, 1));
        let docs = repo::list_documents(&s, &id).unwrap();
        assert!(
            docs.iter()
                .any(|d| d.path == "big.md" && d.status == "skipped" && d.note.is_some())
        );
        assert!(docs.iter().any(|d| d.path == "bad.json" && d.status == "failed"));
    }

    #[test]
    fn cancel_stops_and_progress_reports() {
        let (_d, s, id) = setup(&[("a.md", "one"), ("b.md", "two")]);
        let cancel = CancellationToken::new();
        let mut seen = 0;
        let r = index_library(&s, &id, &IndexOpts::default(), &cancel, |p| {
            seen = p.files_done;
            cancel.cancel();
        });
        assert!(matches!(r, Err(KbError::Cancelled)));
        assert_eq!(seen, 1);
        // The committed file stays; a later run finishes the rest.
        let sum = run(&s, &id);
        assert_eq!((sum.indexed, sum.unchanged), (1, 1));
    }

    #[test]
    fn library_size_cap_truncates() {
        let (_d, s, id) = setup(&[("a.md", "aaaaaaaaaa"), ("b.md", "bbbbbbbbbb")]);
        let opts = IndexOpts {
            max_library_bytes: 12,
            ..Default::default()
        };
        let sum = index_library(&s, &id, &opts, &CancellationToken::new(), |_| {}).unwrap();
        assert!(sum.truncated);
        assert_eq!((sum.indexed, sum.skipped), (1, 1));
    }
}
