//! Incremental, read-only reader of transcript files (OBSERVE.md §4).
//!
//! Files are only ever opened for reading. Directory and file symlinks are not followed, and the roots
//! are canonicalised, so nothing outside the two agent folders is read.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::claude::ClaudeParser;
use crate::codex::CodexParser;
use crate::model::{Drift, LineParser, Obs, Source};

/// On first sight of a file only its last 2 MB are read, so a restart is not blind but never slow.
const INITIAL_TAIL: u64 = 2 * 1024 * 1024;
/// Upper bound of bytes read from one file in one poll (the rest comes next poll).
const MAX_PER_POLL: u64 = 8 * 1024 * 1024;
/// A line longer than this is skipped unparsed.
const MAX_LINE: usize = 1024 * 1024;
const MAX_FILES: usize = 400;
const MAX_VISITED: usize = 20_000;
const DISCOVER_EVERY_MS: i64 = 5_000;

/// One folder to watch.
#[derive(Clone, Debug)]
pub struct Root {
    pub source: Source,
    /// `~/.claude/projects` or `~/.codex/sessions`.
    pub dir: PathBuf,
}

struct FileState {
    source: Source,
    parser: Box<dyn LineParser>,
    offset: u64,
    partial: Vec<u8>,
    /// Discard bytes up to the next newline (we started mid-line, or a line was too long).
    skipping: bool,
}

/// Follows every `.jsonl` under the roots whose mtime is inside the window.
pub struct Tailer {
    roots: Vec<Root>,
    window_ms: i64,
    files: HashMap<PathBuf, FileState>,
    last_discovery_ms: i64,
    pub drift: Drift,
}

/// `(path, mtime_ms, len)` of a candidate file.
type Found = (PathBuf, i64, u64);

fn walk(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<Found>, budget: &mut usize) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        // `DirEntry::file_type` does not follow symlinks: links are skipped, never traversed.
        let Ok(ft) = e.file_type() else { continue };
        let path = e.path();
        if ft.is_dir() {
            if depth < max_depth {
                walk(&path, depth + 1, max_depth, out, budget);
            }
        } else if ft.is_file()
            && path.extension().is_some_and(|x| x == "jsonl")
            && let Ok(md) = e.metadata()
        {
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as i64);
            out.push((path, mtime, md.len()));
        }
    }
}

fn fallback_session(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
    // rollout-2026-10-19T19-28-50-<uuid>: the uuid is the last 36 characters.
    if stem.len() > 36 && stem.is_char_boundary(stem.len() - 36) {
        stem[stem.len() - 36..].to_string()
    } else {
        stem.to_string()
    }
}

impl FileState {
    fn new(source: Source, path: &Path, len: u64) -> Self {
        let parser: Box<dyn LineParser> = match source {
            Source::Codex => Box::new(CodexParser::new(&fallback_session(path))),
            _ => Box::new(ClaudeParser::new()),
        };
        let start = len.saturating_sub(INITIAL_TAIL);
        Self {
            source,
            parser,
            offset: start,
            partial: Vec::new(),
            skipping: start > 0,
        }
    }

    fn reset(&mut self, path: &Path) {
        *self = FileState::new(self.source, path, 0);
    }

    fn feed(&mut self, bytes: &[u8], now_ms: i64, out: &mut Vec<Obs>, drift: &mut Drift) {
        for chunk in bytes.split_inclusive(|b| *b == b'\n') {
            if chunk.last() == Some(&b'\n') {
                if self.skipping {
                    self.skipping = false;
                    self.partial.clear();
                    continue;
                }
                self.partial.extend_from_slice(&chunk[..chunk.len() - 1]);
                let line = std::mem::take(&mut self.partial);
                if line.len() <= MAX_LINE && !line.iter().all(u8::is_ascii_whitespace) {
                    let text = String::from_utf8_lossy(&line);
                    let parsed = self.parser.parse_line(&text, now_ms);
                    drift.lines += 1;
                    if parsed.bad {
                        drift.bad += 1;
                    }
                    out.extend(parsed.obs);
                }
            } else if !self.skipping {
                self.partial.extend_from_slice(chunk);
                if self.partial.len() > MAX_LINE {
                    self.partial.clear();
                    self.skipping = true;
                }
            }
        }
    }
}

impl Tailer {
    pub fn new(roots: Vec<Root>, window_ms: i64) -> Self {
        Self {
            roots,
            window_ms,
            files: HashMap::new(),
            last_discovery_ms: i64::MIN / 2,
            drift: Drift::default(),
        }
    }

    pub fn set_window_ms(&mut self, window_ms: i64) {
        self.window_ms = window_ms;
    }

    /// Replace the watched roots (a source was switched on or off). State of files that are no longer
    /// under a root is dropped on the next discovery.
    pub fn set_roots(&mut self, roots: Vec<Root>) {
        self.roots = roots;
        self.last_discovery_ms = i64::MIN / 2;
    }

    pub fn is_idle(&self) -> bool {
        self.roots.is_empty()
    }

    fn discover(&mut self, now_ms: i64) {
        let mut found: Vec<(Source, Found)> = Vec::new();
        let mut budget = MAX_VISITED;
        for r in &self.roots {
            // Canonicalise each time: the folder may appear later, or be a link the owner set up.
            let Ok(dir) = r.dir.canonicalize() else { continue };
            let mut v = Vec::new();
            walk(&dir, 0, 4, &mut v, &mut budget);
            found.extend(v.into_iter().map(|f| (r.source, f)));
        }
        let cutoff = now_ms - self.window_ms;
        found.retain(|(_, (_, mtime, _))| *mtime >= cutoff);
        found.sort_by_key(|(_, (_, mtime, _))| std::cmp::Reverse(*mtime));
        found.truncate(MAX_FILES);
        let keep: std::collections::HashSet<&PathBuf> = found.iter().map(|(_, (p, _, _))| p).collect();
        self.files.retain(|p, _| keep.contains(p));
        for (source, (path, _, len)) in found {
            self.files
                .entry(path.clone())
                .or_insert_with(|| FileState::new(source, &path, len));
        }
    }

    /// Read what was appended since the last call. Never blocks on a missing or locked file.
    pub fn poll(&mut self, now_ms: i64) -> Vec<Obs> {
        if self.roots.is_empty() {
            self.files.clear();
            return Vec::new();
        }
        if now_ms - self.last_discovery_ms >= DISCOVER_EVERY_MS {
            self.discover(now_ms);
            self.last_discovery_ms = now_ms;
        }
        let mut out = Vec::new();
        let mut gone = Vec::new();
        for (path, st) in &mut self.files {
            let Ok(mut f) = File::open(path) else {
                gone.push(path.clone());
                continue;
            };
            let Ok(md) = f.metadata() else { continue };
            if !md.is_file() {
                gone.push(path.clone());
                continue;
            }
            let len = md.len();
            if len < st.offset {
                st.reset(path);
            }
            if len == st.offset {
                continue;
            }
            if f.seek(SeekFrom::Start(st.offset)).is_err() {
                continue;
            }
            let mut buf = Vec::new();
            let want = (len - st.offset).min(MAX_PER_POLL);
            if f.take(want).read_to_end(&mut buf).is_err() {
                continue;
            }
            st.offset += buf.len() as u64;
            st.feed(&buf, now_ms, &mut out, &mut self.drift);
        }
        for p in gone {
            self.files.remove(&p);
        }
        out
    }

    pub fn tracked_files(&self) -> usize {
        self.files.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ObsKind;
    use std::io::Write;

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64)
    }

    fn line(i: u32) -> String {
        format!(
            "{{\"type\":\"user\",\"sessionId\":\"s1\",\"cwd\":\"/p\",\"message\":{{\"content\":\"msg {i}\"}}}}\n"
        )
    }

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let proj = d.path().join("projects/-p");
        fs::create_dir_all(&proj).unwrap();
        (d, proj)
    }

    fn tailer(d: &tempfile::TempDir) -> Tailer {
        Tailer::new(
            vec![Root {
                source: Source::ClaudeCode,
                dir: d.path().join("projects"),
            }],
            6 * 3_600_000,
        )
    }

    #[test]
    fn reads_only_what_was_appended_and_waits_for_the_newline() {
        let (d, proj) = setup();
        let file = proj.join("s1.jsonl");
        fs::write(&file, line(1)).unwrap();
        let mut t = tailer(&d);
        let first = t.poll(now());
        assert_eq!(first.len(), 1);
        assert!(t.poll(now()).is_empty(), "nothing new");
        let mut f = fs::OpenOptions::new().append(true).open(&file).unwrap();
        let l2 = line(2);
        f.write_all(&l2.as_bytes()[..20]).unwrap();
        assert!(t.poll(now()).is_empty(), "half a line is held back");
        f.write_all(&l2.as_bytes()[20..]).unwrap();
        let more = t.poll(now());
        assert_eq!(more.len(), 1);
        assert!(matches!(more[0].kind, ObsKind::Message(_)));
        assert_eq!(t.drift.bad, 0);
    }

    #[test]
    fn truncation_starts_over() {
        let (d, proj) = setup();
        let file = proj.join("s1.jsonl");
        fs::write(&file, line(1) + &line(2)).unwrap();
        let mut t = tailer(&d);
        assert_eq!(t.poll(now()).len(), 2);
        fs::write(&file, line(3)).unwrap();
        assert_eq!(t.poll(now()).len(), 1);
    }

    #[test]
    fn big_old_files_are_tailed_from_the_end_without_a_torn_first_line() {
        let (d, proj) = setup();
        let file = proj.join("big.jsonl");
        let mut s = String::new();
        let one = line(0).len();
        let n = (INITIAL_TAIL as usize / one) + 5000;
        for i in 0..n {
            s.push_str(&line(i as u32));
        }
        fs::write(&file, &s).unwrap();
        let mut t = tailer(&d);
        let got = t.poll(now());
        assert!(got.len() < n && got.len() > 1000, "tail only: {}", got.len());
        assert_eq!(t.drift.bad, 0, "no torn line was parsed");
    }

    #[test]
    fn bad_lines_count_as_drift_and_never_stop_the_reader() {
        let (d, proj) = setup();
        fs::write(proj.join("s1.jsonl"), format!("garbage\n{}", line(1))).unwrap();
        let mut t = tailer(&d);
        assert_eq!(t.poll(now()).len(), 1);
        assert_eq!(t.drift, Drift { lines: 2, bad: 1 });
    }

    #[test]
    fn old_files_and_non_jsonl_are_ignored() {
        let (d, proj) = setup();
        fs::write(proj.join("notes.txt"), "x\n").unwrap();
        fs::write(proj.join("s1.jsonl"), line(1)).unwrap();
        let mut t = tailer(&d);
        // A clock far in the future makes the file older than the window.
        assert!(t.poll(now() + 30 * 24 * 3_600_000).is_empty());
        assert_eq!(t.tracked_files(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        let (d, proj) = setup();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.jsonl"), line(9)).unwrap();
        std::os::unix::fs::symlink(outside.path(), proj.join("escape")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.jsonl"), proj.join("link.jsonl")).unwrap();
        let mut t = tailer(&d);
        assert!(t.poll(now()).is_empty());
        assert_eq!(t.tracked_files(), 0);
    }

    #[test]
    fn codex_files_use_the_uuid_from_the_name_until_session_meta() {
        let d = tempfile::tempdir().unwrap();
        let day = d.path().join("sessions/2026/10/19");
        fs::create_dir_all(&day).unwrap();
        let uuid = "0199aaaa-bbbb-cccc-dddd-eeeeffff0000";
        fs::write(
            day.join(format!("rollout-2026-10-19T19-28-50-{uuid}.jsonl")),
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\"}}\n",
        )
        .unwrap();
        let mut t = Tailer::new(
            vec![Root {
                source: Source::Codex,
                dir: d.path().join("sessions"),
            }],
            6 * 3_600_000,
        );
        let got = t.poll(now());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].session, uuid);
        assert_eq!(got[0].source, Source::Codex);
    }

    #[test]
    fn no_roots_opens_nothing() {
        let mut t = Tailer::new(vec![], 1000);
        assert!(t.is_idle());
        assert!(t.poll(now()).is_empty());
    }
}
