//! In-memory ring of the server's last output lines plus a size-rotated log file.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::io::AsyncWriteExt;

/// Last `cap` lines of child output, shared between the pump tasks and readers.
#[derive(Clone, Debug)]
pub struct LogRing {
    inner: Arc<Mutex<VecDeque<String>>>,
    cap: usize,
}

impl LogRing {
    pub fn new(cap: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(cap))),
            cap,
        }
    }

    /// Append a line, evicting the oldest when full.
    pub fn push(&self, line: String) {
        let mut q = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if q.len() == self.cap {
            q.pop_front();
        }
        q.push_back(line);
    }

    /// The last `n` lines, oldest first.
    pub fn tail(&self, n: usize) -> Vec<String> {
        let q = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        q.iter().skip(q.len().saturating_sub(n)).cloned().collect()
    }

    /// Does any buffered line satisfy `pred`?
    pub fn any(&self, pred: impl Fn(&str) -> bool) -> bool {
        let q = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        q.iter().any(|l| pred(l))
    }
}

impl Default for LogRing {
    fn default() -> Self {
        Self::new(200)
    }
}

/// Append-only log file that rotates `x.log -> x.log.1 -> x.log.2` past `max_bytes`.
pub struct RotatingLog {
    path: PathBuf,
    file: tokio::fs::File,
    written: u64,
    max_bytes: u64,
    keep: u32,
}

impl RotatingLog {
    /// Open (rotating first if the existing file is already too big).
    pub async fn open(path: &Path, max_bytes: u64, keep: u32) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let existing = tokio::fs::metadata(path).await.map(|m| m.len()).unwrap_or(0);
        if existing >= max_bytes {
            rotate(path, keep).await?;
        }
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        let written = tokio::fs::metadata(path).await.map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            path: path.to_owned(),
            file,
            written,
            max_bytes,
            keep,
        })
    }

    /// Write one line (a newline is added).
    pub async fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        if self.written >= self.max_bytes {
            self.file.flush().await?;
            rotate(&self.path, self.keep).await?;
            self.file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .await?;
            self.written = 0;
        }
        self.file.write_all(line.as_bytes()).await?;
        self.file.write_all(b"\n").await?;
        self.written += line.len() as u64 + 1;
        Ok(())
    }
}

async fn rotate(path: &Path, keep: u32) -> std::io::Result<()> {
    let numbered = |i: u32| PathBuf::from(format!("{}.{i}", path.display()));
    let _ = tokio::fs::remove_file(numbered(keep.saturating_sub(1))).await;
    for i in (1..keep.saturating_sub(1)).rev() {
        let _ = tokio::fs::rename(numbered(i), numbered(i + 1)).await;
    }
    if keep > 1 {
        let _ = tokio::fs::rename(path, numbered(1)).await;
    } else {
        let _ = tokio::fs::remove_file(path).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_evicts_oldest() {
        let r = LogRing::new(3);
        for i in 0..5 {
            r.push(format!("l{i}"));
        }
        assert_eq!(r.tail(10), vec!["l2", "l3", "l4"]);
        assert_eq!(r.tail(2), vec!["l3", "l4"]);
        assert!(r.any(|l| l == "l4"));
    }

    #[tokio::test]
    async fn file_rotates_and_keeps_three() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("llama-server.log");
        let mut log = RotatingLog::open(&p, 20, 3).await.unwrap();
        for i in 0..12 {
            log.write_line(&format!("line number {i}")).await.unwrap();
        }
        log.file.flush().await.unwrap();
        assert!(p.exists());
        assert!(dir.path().join("llama-server.log.1").exists());
        assert!(dir.path().join("llama-server.log.2").exists());
        assert!(!dir.path().join("llama-server.log.3").exists());
    }
}
