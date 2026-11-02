//! Spawning a worker child with a cleared environment and captured, line-split output.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use nexus_llm::LogRing;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

use crate::error::StudioError;

/// Called for every output line of the worker; `is_stderr` tells the streams apart.
pub type LineSink = Arc<dyn Fn(&str, bool) + Send + Sync>;

/// What to run.
pub struct Spawn {
    pub bin: PathBuf,
    pub args: Vec<String>,
    /// Added after the minimal base environment; use it for HOME/TMP/cache redirection (UP.022).
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    /// Bytes written to the child's stdin, which is then closed. `None` = no stdin.
    pub stdin: Option<Vec<u8>>,
}

/// A running worker.
pub struct Worker {
    child: Child,
    log: LogRing,
}

fn base_env_keys() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &["PATH", "SystemRoot", "TEMP", "TMP", "HOME"]
    }
    #[cfg(not(windows))]
    {
        &["PATH", "HOME", "TMPDIR"]
    }
}

/// Environment entries that let a worker find the shared libraries next to its executable.
pub fn library_env(bin: &Path) -> Vec<(String, String)> {
    let Some(dir) = bin.parent() else {
        return Vec::new();
    };
    let dir = dir.to_string_lossy();
    if cfg!(target_os = "macos") {
        vec![("DYLD_LIBRARY_PATH".into(), dir.into_owned())]
    } else if cfg!(target_os = "windows") {
        let path = std::env::var("PATH").unwrap_or_default();
        vec![("PATH".into(), format!("{dir};{path}"))]
    } else {
        vec![("LD_LIBRARY_PATH".into(), dir.into_owned())]
    }
}

impl Worker {
    pub async fn spawn(s: Spawn, sink: LineSink) -> Result<Self, StudioError> {
        if !s.bin.is_file() {
            return Err(StudioError::NotInstalled(s.bin.file_name().map_or_else(
                || "The engine".into(),
                |n| n.to_string_lossy().into_owned(),
            )));
        }
        let mut cmd = Command::new(&s.bin);
        cmd.args(&s.args)
            .env_clear()
            .stdin(if s.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for key in base_env_keys() {
            if let Some(v) = std::env::var_os(key) {
                cmd.env(key, v);
            }
        }
        for (k, v) in &s.env {
            cmd.env(k, v);
        }
        if let Some(cwd) = &s.cwd {
            cmd.current_dir(cwd);
        }
        #[cfg(unix)]
        cmd.process_group(0);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = cmd
            .spawn()
            .map_err(|e| StudioError::Start(format!("{}: {e}", s.bin.display())))?;
        if let (Some(bytes), Some(mut stdin)) = (s.stdin, child.stdin.take()) {
            tokio::spawn(async move {
                let _ = stdin.write_all(&bytes).await;
                let _ = stdin.shutdown().await;
            });
        }
        let log = LogRing::new(300);
        if let Some(out) = child.stdout.take() {
            tokio::spawn(pump(out, log.clone(), sink.clone(), false));
        }
        if let Some(err) = child.stderr.take() {
            tokio::spawn(pump(err, log.clone(), sink, true));
        }
        Ok(Self { child, log })
    }

    pub fn log_tail(&self, n: usize) -> Vec<String> {
        self.log.tail(n)
    }

    /// `Some(code)` once exited (`-1` when killed by a signal).
    pub fn exited(&mut self) -> Option<i32> {
        self.child
            .try_wait()
            .ok()
            .flatten()
            .map(|s| s.code().unwrap_or(-1))
    }

    pub async fn wait(&mut self) -> Result<i32, StudioError> {
        let s = self.child.wait().await?;
        Ok(s.code().unwrap_or(-1))
    }

    /// Kill the worker and its process group, then reap it.
    pub async fn kill(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.child.id() {
            // Signal the whole group so helper processes do not outlive the worker.
            let _ = Command::new("kill")
                .args(["-KILL", &format!("-{pid}")])
                .status()
                .await;
        }
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
    }
}

/// Split on both `\n` and `\r`: progress bars redraw one line with carriage returns.
async fn pump<R: AsyncRead + Unpin>(stream: R, ring: LogRing, sink: LineSink, is_stderr: bool) {
    let mut reader = BufReader::new(stream);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let text = String::from_utf8_lossy(&buf);
                for piece in text.split(['\n', '\r']) {
                    if piece.trim().is_empty() {
                        continue;
                    }
                    sink(piece, is_stderr);
                    ring.push(piece.to_owned());
                }
            }
        }
    }
}
