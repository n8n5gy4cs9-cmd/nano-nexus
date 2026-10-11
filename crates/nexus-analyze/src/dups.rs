//! Duplicate detection in three stages (ANALYZER §1): size, then BLAKE3 of the first 64 KiB, then BLAKE3 of
//! the whole file. Hashing runs on the rayon pool; every stage checks the cancel token.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::report::{DupGroup, FileEntry};

const HEAD: usize = 64 * 1024;

/// Files sharing a size and a first-64-KiB hash.
type HeadGroup<'a> = ((u64, [u8; 32]), Vec<&'a FileEntry>);
/// Files above this are hashed with the memory-mapped, multi-threaded path.
const MMAP_ABOVE: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DupMode {
    Off,
    /// Same size and same first 64 KiB: quick, but not proof.
    Fast,
    /// Same BLAKE3 of the whole file.
    Full,
}

fn hash_head(path: &Path) -> Option<[u8; 32]> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; HEAD];
    let mut n = 0;
    while n < HEAD {
        match f.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(_) => return None,
        }
    }
    Some(*blake3::hash(&buf[..n]).as_bytes())
}

fn hash_full(path: &Path, size: u64) -> Option<[u8; 32]> {
    let mut h = blake3::Hasher::new();
    if size >= MMAP_ABOVE {
        h.update_mmap_rayon(path).ok()?;
    } else {
        h.update_reader(std::fs::File::open(path).ok()?).ok()?;
    }
    Some(*h.finalize().as_bytes())
}

fn hex(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Group identical files under `root`. Returns `None` if cancelled. Groups are sorted by wasted bytes.
pub fn find_duplicates(
    root: &Path,
    files: &[FileEntry],
    mode: DupMode,
    min_size: u64,
    cancel: &CancellationToken,
) -> Option<Vec<DupGroup>> {
    if mode == DupMode::Off {
        return Some(Vec::new());
    }
    // Stage 1: size.
    let mut by_size: HashMap<u64, Vec<&FileEntry>> = HashMap::new();
    for f in files.iter().filter(|f| f.bytes >= min_size.max(1)) {
        by_size.entry(f.bytes).or_default().push(f);
    }
    let candidates: Vec<&FileEntry> = by_size.into_values().filter(|g| g.len() > 1).flatten().collect();

    // Stage 2: first 64 KiB.
    let heads: Vec<(&FileEntry, Option<[u8; 32]>)> = candidates
        .par_iter()
        .map(|f| {
            if cancel.is_cancelled() {
                (*f, None)
            } else {
                (*f, hash_head(&root.join(&f.rel)))
            }
        })
        .collect();
    if cancel.is_cancelled() {
        return None;
    }
    let mut by_head: HashMap<(u64, [u8; 32]), Vec<&FileEntry>> = HashMap::new();
    for (f, h) in heads {
        if let Some(h) = h {
            by_head.entry((f.bytes, h)).or_default().push(f);
        }
    }
    let mut groups: Vec<DupGroup> = Vec::new();
    let head_groups: Vec<HeadGroup<'_>> = by_head.into_iter().filter(|(_, g)| g.len() > 1).collect();
    if mode == DupMode::Fast {
        for ((size, h), g) in head_groups {
            let mut paths: Vec<String> = g.iter().map(|f| f.rel.clone()).collect();
            paths.sort();
            groups.push(DupGroup {
                bytes_each: size,
                hash: hex(&h),
                paths,
                confirmed: false,
            });
        }
    } else {
        // Stage 3: the whole file.
        let full: Vec<(u64, Option<[u8; 32]>, &FileEntry)> = head_groups
            .iter()
            .flat_map(|((size, _), g)| g.iter().map(move |f| (*size, *f)))
            .collect::<Vec<_>>()
            .par_iter()
            .map(|(size, f)| {
                if cancel.is_cancelled() {
                    (*size, None, *f)
                } else {
                    (*size, hash_full(&root.join(&f.rel), *size), *f)
                }
            })
            .collect();
        if cancel.is_cancelled() {
            return None;
        }
        let mut by_full: HashMap<(u64, [u8; 32]), Vec<String>> = HashMap::new();
        for (size, h, f) in full {
            if let Some(h) = h {
                by_full.entry((size, h)).or_default().push(f.rel.clone());
            }
        }
        for ((size, h), mut paths) in by_full {
            if paths.len() > 1 {
                paths.sort();
                groups.push(DupGroup {
                    bytes_each: size,
                    hash: hex(&h),
                    paths,
                    confirmed: true,
                });
            }
        }
    }
    groups.sort_by(|a, b| b.wasted().cmp(&a.wasted()).then_with(|| a.paths.cmp(&b.paths)));
    Some(groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::categories::Category;

    fn entry(rel: &str, bytes: u64) -> FileEntry {
        FileEntry {
            rel: rel.into(),
            bytes,
            modified: None,
            category: Category::Other,
            ext: String::new(),
            hidden: false,
        }
    }

    fn write(dir: &Path, rel: &str, data: &[u8]) -> FileEntry {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, data).unwrap();
        entry(rel, data.len() as u64)
    }

    #[test]
    fn finds_identical_files_and_ignores_look_alikes() {
        let d = tempfile::tempdir().unwrap();
        let same = vec![7u8; 10_000];
        let mut tail_differs = same.clone();
        *tail_differs.last_mut().unwrap() = 8;
        let mut head_differs = same.clone();
        head_differs[0] = 9;
        let files = vec![
            write(d.path(), "a/one.bin", &same),
            write(d.path(), "b/two.bin", &same),
            write(d.path(), "c/three.bin", &same),
            write(d.path(), "c/tail.bin", &tail_differs),
            write(d.path(), "c/head.bin", &head_differs),
            write(d.path(), "c/small.bin", b"x"),
            write(d.path(), "c/small2.bin", b"x"),
        ];
        let g = find_duplicates(d.path(), &files, DupMode::Full, 4096, &CancellationToken::new()).unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].paths, ["a/one.bin", "b/two.bin", "c/three.bin"]);
        assert_eq!((g[0].wasted(), g[0].confirmed), (20_000, true));
        // With a tiny minimum the 1-byte pair shows up too.
        let g = find_duplicates(d.path(), &files, DupMode::Full, 1, &CancellationToken::new()).unwrap();
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn fast_mode_trusts_the_head_but_full_mode_does_not() {
        let d = tempfile::tempdir().unwrap();
        let mut a = vec![1u8; 200_000];
        let b = a.clone();
        *a.last_mut().unwrap() = 2; // differs after the first 64 KiB
        let files = vec![write(d.path(), "a.bin", &a), write(d.path(), "b.bin", &b)];
        let fast = find_duplicates(d.path(), &files, DupMode::Fast, 4096, &CancellationToken::new()).unwrap();
        assert_eq!(fast.len(), 1);
        assert!(!fast[0].confirmed);
        let full = find_duplicates(d.path(), &files, DupMode::Full, 4096, &CancellationToken::new()).unwrap();
        assert!(full.is_empty());
        assert!(
            find_duplicates(d.path(), &files, DupMode::Off, 4096, &CancellationToken::new())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn groups_are_sorted_by_wasted_bytes_and_cancel_returns_none() {
        let d = tempfile::tempdir().unwrap();
        let big = vec![3u8; 50_000];
        let small = vec![4u8; 5_000];
        let files = vec![
            write(d.path(), "s1", &small),
            write(d.path(), "s2", &small),
            write(d.path(), "b1", &big),
            write(d.path(), "b2", &big),
        ];
        let g = find_duplicates(d.path(), &files, DupMode::Full, 4096, &CancellationToken::new()).unwrap();
        assert_eq!(g[0].bytes_each, 50_000);
        let c = CancellationToken::new();
        c.cancel();
        assert!(find_duplicates(d.path(), &files, DupMode::Full, 4096, &c).is_none());
    }

    #[test]
    fn unreadable_files_are_skipped() {
        let d = tempfile::tempdir().unwrap();
        let data = vec![5u8; 8000];
        let mut files = vec![write(d.path(), "a", &data), write(d.path(), "b", &data)];
        files.push(entry("missing", 8000));
        let g = find_duplicates(d.path(), &files, DupMode::Full, 4096, &CancellationToken::new()).unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].paths.len(), 2);
    }
}
