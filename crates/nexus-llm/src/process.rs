//! Spawning and stopping the `llama-server` child (INFERENCE-CONTRACT §1, §3).

use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};

use crate::config::RuntimeConfig;
use crate::error::LlmError;
use crate::logring::{LogRing, RotatingLog};
use crate::port::is_bind_error;

const LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;
const LOG_KEEP: u32 = 3;

/// A running `llama-server` child with its output captured.
pub struct ServerProcess {
    child: Child,
    pid: u32,
    port: u16,
    log: LogRing,
    /// Windows: dropping the job (app exit for any reason) kills the server.
    #[cfg(windows)]
    _job: Option<win32job::Job>,
}

impl ServerProcess {
    /// Spawn the server on `port`. The environment is cleared and rebuilt minimally so stray
    /// `LLAMA_ARG_*` variables can't change behaviour; the token travels in `LLAMA_API_KEY`, never argv.
    /// Output goes to the returned ring and, if `log_file` is set, to a rotating file.
    pub async fn spawn(
        cfg: &RuntimeConfig,
        port: u16,
        token: &SecretString,
        log_file: Option<&Path>,
    ) -> Result<Self, LlmError> {
        let mut cmd = Command::new(&cfg.server_bin);
        cmd.args(cfg.to_args(port))
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for key in base_env_keys() {
            if let Some(v) = std::env::var_os(key) {
                cmd.env(key, v);
            }
        }
        cmd.env("LLAMA_API_KEY", token.expose_secret());
        platform_setup(&mut cmd);

        let mut child = cmd
            .spawn()
            .map_err(|e| LlmError::Transport(format!("could not start {}: {e}", cfg.server_bin.display())))?;
        let pid = child
            .id()
            .ok_or_else(|| LlmError::Transport("child exited immediately".into()))?;
        let log = LogRing::default();
        let file = match log_file {
            Some(p) => RotatingLog::open(p, LOG_MAX_BYTES, LOG_KEEP).await.ok(),
            None => None,
        };
        let file = std::sync::Arc::new(tokio::sync::Mutex::new(file));
        if let Some(out) = child.stdout.take() {
            tokio::spawn(pump(out, log.clone(), file.clone()));
        }
        if let Some(err) = child.stderr.take() {
            tokio::spawn(pump(err, log.clone(), file.clone()));
        }
        #[cfg(windows)]
        let job = assign_kill_on_close_job(&child);
        tracing::info!(pid, port, "llama-server started");
        Ok(Self {
            child,
            pid,
            port,
            log,
            #[cfg(windows)]
            _job: job,
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The captured output ring.
    pub fn log(&self) -> &LogRing {
        &self.log
    }

    /// Last `n` lines of output.
    pub fn log_tail(&self, n: usize) -> Vec<String> {
        self.log.tail(n)
    }

    /// Did the output say the port could not be bound?
    pub fn saw_bind_error(&self) -> bool {
        self.log.any(is_bind_error)
    }

    /// `Some(status)` once the child has exited.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, LlmError> {
        self.child
            .try_wait()
            .map_err(|e| LlmError::Transport(e.to_string()))
    }

    /// Wait for the child to exit.
    pub async fn wait(&mut self) -> Result<ExitStatus, LlmError> {
        self.child
            .wait()
            .await
            .map_err(|e| LlmError::Transport(e.to_string()))
    }

    /// Graceful stop: SIGTERM to the process group, wait up to `grace`, then SIGKILL. On Windows the
    /// child is simply killed. Safe to call on an already-exited child.
    pub async fn stop(&mut self, grace: Duration) {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return;
        }
        // Windows has no SIGTERM and llama-server holds no state to flush: kill straight away.
        #[cfg(windows)]
        let grace = {
            let _ = self.child.start_kill();
            grace
        };
        terminate(self.pid);
        if tokio::time::timeout(grace, self.child.wait()).await.is_err() {
            kill_group(self.pid);
            let _ = self.child.start_kill();
            let _ = self.child.wait().await;
        }
        tracing::info!(pid = self.pid, "llama-server stopped");
    }
}

fn base_env_keys() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &["PATH", "HOME", "TMPDIR", "SystemRoot", "TEMP", "TMP"]
    }
    #[cfg(not(windows))]
    {
        &["PATH", "HOME", "TMPDIR"]
    }
}

#[cfg(unix)]
fn platform_setup(cmd: &mut Command) {
    // Own process group so a kill can take the whole tree.
    cmd.process_group(0);
}

/// `CREATE_NO_WINDOW`: no console window flashes when the server starts.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
fn platform_setup(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(any(unix, windows)))]
fn platform_setup(_cmd: &mut Command) {}

/// Put the child in a Job Object with `KILL_ON_JOB_CLOSE`, so Windows kills the server when the app
/// dies for any reason. Failure is logged and non-fatal (the explicit stop path still works).
#[cfg(windows)]
fn assign_kill_on_close_job(child: &Child) -> Option<win32job::Job> {
    let attempt = || -> Result<win32job::Job, win32job::JobError> {
        let job = win32job::Job::create()?;
        let mut info = job.query_extended_limit_info()?;
        info.limit_kill_on_job_close();
        job.set_extended_limit_info(&info)?;
        if let Some(handle) = child.raw_handle() {
            job.assign_process(handle as isize)?;
        }
        Ok(job)
    };
    match attempt() {
        Ok(job) => Some(job),
        Err(e) => {
            tracing::warn!("could not create kill-on-close job object: {e}");
            None
        }
    }
}

#[cfg(unix)]
fn signal_group(pid: u32, sig: nix::sys::signal::Signal) {
    if let Ok(raw) = i32::try_from(pid) {
        let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(raw), sig);
    }
}

#[cfg(unix)]
fn terminate(pid: u32) {
    signal_group(pid, nix::sys::signal::Signal::SIGTERM);
}

#[cfg(unix)]
fn kill_group(pid: u32) {
    signal_group(pid, nix::sys::signal::Signal::SIGKILL);
}

#[cfg(not(unix))]
fn terminate(_pid: u32) {}

#[cfg(not(unix))]
fn kill_group(_pid: u32) {}

type SharedFile = std::sync::Arc<tokio::sync::Mutex<Option<RotatingLog>>>;

async fn pump<R: AsyncRead + Unpin>(stream: R, ring: LogRing, file: SharedFile) {
    let mut reader = BufReader::new(stream);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let line = String::from_utf8_lossy(&buf)
                    .trim_end_matches(['\n', '\r'])
                    .to_owned();
                if let Some(f) = file.lock().await.as_mut() {
                    let _ = f.write_line(&line).await;
                }
                ring.push(line);
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use secrecy::SecretString;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, body: &str) -> std::path::PathBuf {
        let p = dir.join("fake-server.sh");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    async fn wait_for(p: &ServerProcess, pred: impl Fn(&str) -> bool) {
        for _ in 0..100 {
            if p.log().any(&pred) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("log never matched; tail = {:?}", p.log_tail(20));
    }

    #[tokio::test]
    async fn captures_output_clears_env_and_passes_token_via_env_only() {
        let dir = tempfile::tempdir().unwrap();
        let bin = script(
            dir.path(),
            "echo \"args:$*\"; echo \"key:$LLAMA_API_KEY\" >&2; env | sed 's/^/env:/'; sleep 30",
        );
        let cfg = RuntimeConfig::new(bin, "/m/model.gguf");
        let token = SecretString::from("sekrit-token".to_owned());
        let log_file = dir.path().join("logs/llama-server.log");
        let mut p = ServerProcess::spawn(&cfg, 4242, &token, Some(&log_file))
            .await
            .unwrap();
        wait_for(&p, |l| l.starts_with("key:")).await;
        wait_for(&p, |l| l.starts_with("env:PATH=")).await;
        let tail = p.log_tail(200);
        assert!(
            tail.iter()
                .any(|l| l.starts_with("args:") && l.contains("--port 4242"))
        );
        assert!(
            !tail
                .iter()
                .any(|l| l.starts_with("args:") && l.contains("sekrit-token"))
        );
        assert!(tail.iter().any(|l| l == "key:sekrit-token"));
        assert!(
            !tail.iter().any(|l| l.starts_with("env:CARGO")),
            "environment was not cleared"
        );
        p.stop(Duration::from_secs(2)).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let on_disk = std::fs::read_to_string(&log_file).unwrap();
        assert!(on_disk.contains("--port 4242"));
    }

    #[tokio::test]
    async fn stop_terminates_the_whole_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let bin = script(dir.path(), "sleep 300 & echo \"child:$!\"; wait");
        let cfg = RuntimeConfig::new(bin, "/m");
        let mut p = ServerProcess::spawn(&cfg, 1, &SecretString::from("t".to_owned()), None)
            .await
            .unwrap();
        wait_for(&p, |l| l.starts_with("child:")).await;
        let grandchild: i32 = p
            .log_tail(5)
            .iter()
            .find_map(|l| l.strip_prefix("child:")?.parse().ok())
            .unwrap();
        p.stop(Duration::from_secs(2)).await;
        assert!(p.try_wait().unwrap().is_some());
        let pid = nix::unistd::Pid::from_raw(grandchild);
        for _ in 0..50 {
            if nix::sys::signal::kill(pid, None).is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("grandchild {grandchild} survived stop()");
    }

    #[tokio::test]
    async fn stop_escalates_to_sigkill_when_sigterm_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let bin = script(
            dir.path(),
            "trap '' TERM; echo ready; while true; do sleep 1; done",
        );
        let cfg = RuntimeConfig::new(bin, "/m");
        let mut p = ServerProcess::spawn(&cfg, 1, &SecretString::from("t".to_owned()), None)
            .await
            .unwrap();
        wait_for(&p, |l| l == "ready").await;
        let started = std::time::Instant::now();
        p.stop(Duration::from_millis(300)).await;
        assert!(p.try_wait().unwrap().is_some());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn detects_early_exit_and_bind_error() {
        let dir = tempfile::tempdir().unwrap();
        let bin = script(
            dir.path(),
            "echo \"couldn't bind HTTP server socket\" >&2; exit 1",
        );
        let cfg = RuntimeConfig::new(bin, "/m");
        let mut p = ServerProcess::spawn(&cfg, 1, &SecretString::from("t".to_owned()), None)
            .await
            .unwrap();
        let status = p.wait().await.unwrap();
        assert!(!status.success());
        wait_for(&p, is_bind_error).await;
        assert!(p.saw_bind_error());
    }

    #[tokio::test]
    async fn missing_binary_is_a_transport_error() {
        let cfg = RuntimeConfig::new("/definitely/not/here", "/m");
        let r = ServerProcess::spawn(&cfg, 1, &SecretString::from("t".to_owned()), None).await;
        assert!(matches!(r, Err(LlmError::Transport(_))));
    }
}
