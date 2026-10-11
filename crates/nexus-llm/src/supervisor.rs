//! The `llama-server` supervisor (INFERENCE-CONTRACT §2, §3).
//!
//! `start` spawns the child on a free port with a fresh token, waits for `/health`, proves
//! ownership via the authenticated `/v1/models`, and publishes [`RuntimeState`] on a watch channel.
//! A monitor task notices unexpected exits and auto-restarts once.

use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use nexus_core::events::RuntimeState;
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;

use crate::client::{Health, LlmClient};
use crate::config::RuntimeConfig;
use crate::error::LlmError;
use crate::port::find_free_port;
use crate::process::ServerProcess;
use crate::token::new_token;

const GRACE: Duration = Duration::from_secs(3);
const MONITOR_INTERVAL: Duration = Duration::from_millis(250);
const HEALTHY_RESET: Duration = Duration::from_secs(10 * 60);
const LOG_TAIL: usize = 20;
const FAILED_VISIBLE: Duration = Duration::from_millis(150);

struct Ctl {
    proc: Option<ServerProcess>,
    cfg: Option<RuntimeConfig>,
    client: Option<LlmClient>,
    generation: u64,
    /// Automatic restarts left (1 after every user-initiated start).
    budget: u8,
    ready_at: Option<Instant>,
}

struct Inner {
    ctl: Mutex<Ctl>,
    state: watch::Sender<RuntimeState>,
    cancel: StdMutex<CancellationToken>,
    log_file: Option<std::path::PathBuf>,
    server_json: Option<std::path::PathBuf>,
}

impl Inner {
    fn set_state(&self, s: RuntimeState) {
        self.state.send_replace(s);
    }

    /// Cancel whatever startup is running and hand out a fresh token for the next one.
    fn swap_cancel(&self) -> CancellationToken {
        let mut g = self.cancel.lock().unwrap_or_else(|e| e.into_inner());
        g.cancel();
        let fresh = CancellationToken::new();
        *g = fresh.clone();
        fresh
    }

    fn cancel_current(&self) {
        self.cancel.lock().unwrap_or_else(|e| e.into_inner()).cancel();
    }
}

/// Owns the `llama-server` child process. Cheap to clone.
///
/// ```ignore
/// let sup = Supervisor::new(Some(log_path));
/// sup.start(RuntimeConfig::new(bin, model)).await?;
/// let client = sup.client().ok_or(LlmError::RuntimeUnavailable)?;
/// sup.shutdown().await;
/// ```
#[derive(Clone)]
pub struct Supervisor {
    inner: Arc<Inner>,
}

impl Supervisor {
    /// `log_file`: where the child's output is also written (rotating, 5 MB x 3).
    pub fn new(log_file: Option<std::path::PathBuf>) -> Self {
        Self::with_files(log_file, None)
    }

    /// Like [`Supervisor::new`], plus `server_json` (`<data>/runtime/server.json`): written after a
    /// successful start, removed on clean stop, and used to reap an orphan from a previous run.
    pub fn with_files(log_file: Option<std::path::PathBuf>, server_json: Option<std::path::PathBuf>) -> Self {
        let (state, _) = watch::channel(RuntimeState::Stopped);
        Self {
            inner: Arc::new(Inner {
                ctl: Mutex::new(Ctl {
                    proc: None,
                    cfg: None,
                    client: None,
                    generation: 0,
                    budget: 1,
                    ready_at: None,
                }),
                state,
                cancel: StdMutex::new(CancellationToken::new()),
                log_file,
                server_json,
            }),
        }
    }

    /// Subscribe to state changes (this is what feeds the `runtime:status` event).
    pub fn subscribe(&self) -> watch::Receiver<RuntimeState> {
        self.inner.state.subscribe()
    }

    /// The current state.
    pub fn state(&self) -> RuntimeState {
        self.inner.state.borrow().clone()
    }

    /// A client for the running server, only while `Ready`.
    pub async fn client(&self) -> Option<LlmClient> {
        if !matches!(self.state(), RuntimeState::Ready { .. }) {
            return None;
        }
        self.inner.ctl.lock().await.client.clone()
    }

    /// Last lines of the server's output (for the runtime panel and `Failed.log_tail`).
    pub async fn log_tail(&self, n: usize) -> Vec<String> {
        self.inner
            .ctl
            .lock()
            .await
            .proc
            .as_ref()
            .map(|p| p.log_tail(n))
            .unwrap_or_default()
    }

    /// Start the server and wait until it is `Ready` (or fails). Any running server is stopped first.
    pub async fn start(&self, cfg: RuntimeConfig) -> Result<(), LlmError> {
        let cancel = self.inner.swap_cancel();
        let mut ctl = self.inner.ctl.lock().await;
        // Reap before stopping: `stop_locked` deletes server.json, which is what names the orphan.
        if let (Some(json), true) = (&self.inner.server_json, ctl.proc.is_none()) {
            crate::reaper::reap_stale(json, &cfg.server_bin);
        }
        stop_locked(&self.inner, &mut ctl).await;
        ctl.budget = 1;
        start_locked(&self.inner, &mut ctl, cfg, cancel).await
    }

    /// Stop, then start with `cfg` (model switch or settings change).
    pub async fn restart(&self, cfg: RuntimeConfig) -> Result<(), LlmError> {
        self.start(cfg).await
    }

    /// Stop the server (graceful, then forced). Idempotent.
    pub async fn stop(&self) {
        self.inner.cancel_current();
        let mut ctl = self.inner.ctl.lock().await;
        stop_locked(&self.inner, &mut ctl).await;
    }

    /// Stop for app exit. Call this from the Tauri exit handlers (`kill_on_drop` is not enough).
    pub async fn shutdown(&self) {
        self.stop().await;
    }
}

async fn stop_locked(inner: &Arc<Inner>, ctl: &mut Ctl) {
    ctl.generation += 1; // retire any monitor
    ctl.client = None;
    ctl.ready_at = None;
    if let Some(mut p) = ctl.proc.take() {
        p.stop(GRACE).await;
    }
    if let Some(json) = &inner.server_json {
        crate::reaper::ServerInfo::remove(json);
    }
    inner.set_state(RuntimeState::Stopped);
}

fn fail(inner: &Inner, reason: String, tail: Vec<String>) -> LlmError {
    inner.set_state(RuntimeState::Failed {
        reason: reason.clone(),
        log_tail: tail,
        can_retry: true,
    });
    LlmError::Transport(reason)
}

async fn start_locked(
    inner: &Arc<Inner>,
    ctl: &mut Ctl,
    cfg: RuntimeConfig,
    cancel: CancellationToken,
) -> Result<(), LlmError> {
    ctl.generation += 1;
    let generation = ctl.generation;
    ctl.cfg = Some(cfg.clone());
    let max = cfg.max_attempts.max(1);
    let mut last_reason = String::from("did not start");
    let mut last_tail = Vec::new();

    for attempt in 1..=max {
        if cancel.is_cancelled() {
            inner.set_state(RuntimeState::Stopped);
            return Err(LlmError::Cancelled);
        }
        inner.set_state(RuntimeState::Starting { attempt });
        let port = find_free_port().map_err(|e| fail(inner, format!("no free port: {e}"), vec![]))?;
        let token = new_token()?;
        let mut proc = match ServerProcess::spawn(&cfg, port, &token, inner.log_file.as_deref()).await {
            Ok(p) => p,
            Err(e) => return Err(fail(inner, e.to_string(), vec![])),
        };
        let client = LlmClient::new(port, token.clone())?;

        match wait_ready(inner, &mut proc, &client, &cancel, cfg.ready_timeout).await {
            Wait::Ready { model_id } => {
                inner.set_state(RuntimeState::Ready {
                    model_id,
                    model_path: cfg.model_path.to_string_lossy().into_owned(),
                    ctx_size: cfg.ctx_size,
                    gpu_layers: cfg.gpu_layers.clone(),
                });
                if let Some(json) = &inner.server_json {
                    let info = crate::reaper::ServerInfo::new(proc.pid(), port, cfg.server_bin.clone());
                    if let Err(e) = info.write(json) {
                        tracing::warn!("could not write server.json: {e}");
                    }
                }
                ctl.proc = Some(proc);
                ctl.client = Some(client);
                ctl.ready_at = Some(Instant::now());
                spawn_monitor(inner.clone(), generation);
                return Ok(());
            }
            Wait::Cancelled => {
                proc.stop(GRACE).await;
                inner.set_state(RuntimeState::Stopped);
                return Err(LlmError::Cancelled);
            }
            Wait::Retry(reason) => {
                last_tail = proc.log_tail(LOG_TAIL);
                proc.stop(GRACE).await;
                tracing::warn!(attempt, %reason, "llama-server start attempt failed, retrying on a new port");
                last_reason = reason;
            }
            Wait::Fatal(reason) => {
                let tail = proc.log_tail(LOG_TAIL);
                proc.stop(GRACE).await;
                return Err(fail(inner, reason, tail));
            }
        }
    }
    Err(fail(inner, last_reason, last_tail))
}

enum Wait {
    Ready {
        model_id: String,
    },
    /// Try again on a new port (bind error, or a foreign server answered).
    Retry(String),
    Fatal(String),
    Cancelled,
}

async fn wait_ready(
    inner: &Inner,
    proc: &mut ServerProcess,
    client: &LlmClient,
    cancel: &CancellationToken,
    timeout: Duration,
) -> Wait {
    let deadline = Instant::now() + timeout;
    let mut delay = Duration::from_millis(150);
    loop {
        if cancel.is_cancelled() {
            return Wait::Cancelled;
        }
        match proc.try_wait() {
            Ok(Some(status)) => {
                // Give the output pump a moment to deliver the last lines.
                tokio::time::sleep(Duration::from_millis(100)).await;
                let reason = format!("exited {status}");
                return if proc.saw_bind_error() {
                    Wait::Retry(reason)
                } else {
                    Wait::Fatal(reason)
                };
            }
            Ok(None) => {}
            Err(e) => return Wait::Fatal(e.to_string()),
        }
        match client.health().await {
            Ok(Health::Ok) => {
                return match client.models().await {
                    Ok(m) if m.data.len() == 1 => Wait::Ready {
                        model_id: m.data[0].id.clone(),
                    },
                    Ok(m) => Wait::Retry(format!(
                        "port answered but not our server ({} models)",
                        m.data.len()
                    )),
                    Err(_) => Wait::Retry("port answered but not our server".into()),
                };
            }
            Ok(Health::Loading) => inner.set_state(RuntimeState::Loading),
            Err(_) => {} // not listening yet
        }
        if Instant::now() >= deadline {
            return Wait::Fatal(format!("not ready after {}s", timeout.as_secs()));
        }
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = cancel.cancelled() => {}
        }
        delay = (delay + Duration::from_millis(50)).min(Duration::from_millis(500));
    }
}

fn spawn_monitor(inner: Arc<Inner>, generation: u64) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(MONITOR_INTERVAL).await;
            let mut ctl = inner.ctl.lock().await;
            if ctl.generation != generation {
                return;
            }
            let Some(proc) = ctl.proc.as_mut() else { return };
            let Ok(Some(status)) = proc.try_wait() else {
                if ctl.ready_at.is_some_and(|t| t.elapsed() >= HEALTHY_RESET) {
                    ctl.budget = 1;
                }
                continue;
            };
            let tail = proc.log_tail(LOG_TAIL);
            ctl.proc = None;
            ctl.client = None;
            let healthy = ctl.ready_at.is_some_and(|t| t.elapsed() >= HEALTHY_RESET);
            if healthy {
                ctl.budget = 1;
            }
            inner.set_state(RuntimeState::Failed {
                reason: format!("llama-server exited unexpectedly ({status})"),
                log_tail: tail,
                can_retry: true,
            });
            if ctl.budget > 0 {
                ctl.budget -= 1;
                if let Some(cfg) = ctl.cfg.clone() {
                    // Let subscribers observe `Failed` before `Starting` replaces it.
                    tokio::time::sleep(FAILED_VISIBLE).await;
                    let cancel = inner.swap_cancel();
                    tracing::warn!("llama-server died; restarting once");
                    let _ = start_locked(&inner, &mut ctl, cfg, cancel).await;
                }
            }
            return;
        }
    });
}
