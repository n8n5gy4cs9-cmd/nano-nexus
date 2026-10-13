#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support;

use std::time::Duration;

use nexus_core::events::RuntimeState;
use nexus_llm::{LlmError, RuntimeConfig, Supervisor};

const BIN: &str = env!("CARGO_BIN_EXE_mock-llama");

fn cfg(mode: &str) -> RuntimeConfig {
    let mut c = RuntimeConfig::new(BIN, mode);
    c.ready_timeout = Duration::from_secs(20);
    c.max_attempts = 4;
    c
}

/// Record every distinct state kind the supervisor publishes.
fn record(sup: &Supervisor) -> std::sync::Arc<std::sync::Mutex<Vec<&'static str>>> {
    let hist = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (h, mut rx) = (hist.clone(), sup.subscribe());
    tokio::spawn(async move {
        while rx.changed().await.is_ok() {
            let kind = match &*rx.borrow() {
                RuntimeState::Stopped => "stopped",
                RuntimeState::Starting { .. } => "starting",
                RuntimeState::Loading => "loading",
                RuntimeState::Ready { .. } => "ready",
                RuntimeState::Failed { .. } => "failed",
            };
            let mut g = h.lock().unwrap();
            if g.last() != Some(&kind) {
                g.push(kind);
            }
        }
    });
    hist
}

async fn wait_state(sup: &Supervisor, pred: impl Fn(&RuntimeState) -> bool, what: &str) {
    for _ in 0..200 {
        if pred(&sup.state()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}; state = {:?}", sup.state());
}

#[tokio::test]
async fn happy_path_reaches_ready_and_serves() {
    let sup = Supervisor::new(None);
    assert_eq!(sup.state(), RuntimeState::Stopped);
    sup.start(cfg("ok")).await.unwrap();
    match sup.state() {
        RuntimeState::Ready {
            model_id, ctx_size, ..
        } => {
            assert_eq!(model_id, "mock.gguf");
            assert_eq!(ctx_size, 8192);
        }
        other => panic!("{other:?}"),
    }
    let client = sup.client().await.expect("client when ready");
    assert_eq!(client.models().await.unwrap().data.len(), 1);
    sup.shutdown().await;
    assert_eq!(sup.state(), RuntimeState::Stopped);
    assert!(sup.client().await.is_none());
}

#[tokio::test]
async fn slow_load_passes_through_loading() {
    let sup = Supervisor::new(None);
    let mut rx = sup.subscribe();
    let seen = tokio::spawn(async move {
        let mut saw_loading = false;
        while rx.changed().await.is_ok() {
            match &*rx.borrow() {
                RuntimeState::Loading => saw_loading = true,
                RuntimeState::Ready { .. } => break,
                _ => {}
            }
        }
        saw_loading
    });
    sup.start(cfg("slow_load")).await.unwrap();
    assert!(seen.await.unwrap(), "never saw Loading");
    sup.shutdown().await;
}

#[tokio::test]
async fn early_exit_fails_with_log_tail() {
    let sup = Supervisor::new(None);
    assert!(sup.start(cfg("exit_early")).await.is_err());
    match sup.state() {
        RuntimeState::Failed {
            reason,
            log_tail,
            can_retry,
        } => {
            assert!(reason.contains("exited"), "{reason}");
            assert!(
                log_tail.iter().any(|l| l.contains("simulated crash")),
                "{log_tail:?}"
            );
            assert!(can_retry);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn log_tail_still_works_after_a_failed_start() {
    let sup = Supervisor::new(None);
    assert!(sup.start(cfg("exit_early")).await.is_err());
    let lines = sup.log_tail(50).await;
    assert!(lines.iter().any(|l| l.contains("simulated crash")), "{lines:?}");
}

#[tokio::test]
async fn bind_failure_retries_on_a_new_port() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("marker");
    let sup = Supervisor::new(None);
    sup.start(cfg(&format!("bind_fail_once:{}", marker.display())))
        .await
        .unwrap();
    assert!(matches!(sup.state(), RuntimeState::Ready { .. }));
    sup.shutdown().await;
}

#[tokio::test]
async fn permanent_bind_failure_gives_up_after_max_attempts() {
    let sup = Supervisor::new(None);
    let mut c = cfg("bind_fail");
    c.max_attempts = 3;
    assert!(sup.start(c).await.is_err());
    assert!(matches!(sup.state(), RuntimeState::Failed { .. }));
}

#[tokio::test]
async fn foreign_server_is_rejected_as_not_ours() {
    let sup = Supervisor::new(None);
    let mut c = cfg("wrong_token");
    c.max_attempts = 2;
    match sup.start(c).await {
        Err(LlmError::Transport(m)) => assert!(m.contains("not our server"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(sup.state(), RuntimeState::Failed { .. }));
}

#[tokio::test]
async fn crash_auto_restarts_once() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("marker");
    let sup = Supervisor::new(None);
    let hist = record(&sup);
    sup.start(cfg(&format!("crash_once:{}", marker.display())))
        .await
        .unwrap();
    // The first instance dies after ~600 ms; the monitor restarts it, and the second one stays up.
    wait_state(
        &sup,
        |s| matches!(s, RuntimeState::Failed { .. }),
        "Failed after crash",
    )
    .await;
    wait_state(
        &sup,
        |s| matches!(s, RuntimeState::Ready { .. }),
        "Ready after auto-restart",
    )
    .await;
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert!(matches!(sup.state(), RuntimeState::Ready { .. }));
    assert!(sup.client().await.unwrap().health().await.is_ok());
    let h = hist.lock().unwrap().clone();
    assert!(h.windows(2).any(|w| w == ["failed", "starting"]), "{h:?}");
    sup.shutdown().await;
}

#[tokio::test]
async fn second_crash_stays_failed_until_manual_retry() {
    let sup = Supervisor::new(None);
    let hist = record(&sup);
    sup.start(cfg("always_crash")).await.unwrap();
    wait_state(&sup, |s| matches!(s, RuntimeState::Failed { .. }), "first Failed").await;
    // Auto-restart brings it back once, it crashes again and stays down.
    tokio::time::sleep(Duration::from_millis(3000)).await;
    assert!(
        matches!(sup.state(), RuntimeState::Failed { can_retry: true, .. }),
        "{:?}",
        sup.state()
    );
    let h = hist.lock().unwrap().clone();
    assert_eq!(h.iter().filter(|k| **k == "ready").count(), 2, "{h:?}");
    // Manual retry works.
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("m");
    sup.restart(cfg(&format!("crash_once:{}", marker.display())))
        .await
        .unwrap();
    assert!(matches!(sup.state(), RuntimeState::Ready { .. }));
    sup.shutdown().await;
}

#[tokio::test]
async fn shutdown_kills_the_process_and_stop_during_startup_cancels() {
    let sup = Supervisor::new(None);
    sup.start(cfg("ok")).await.unwrap();
    let port_client = sup.client().await.unwrap();
    sup.shutdown().await;
    assert!(
        port_client.health().await.is_err(),
        "server still answering after shutdown"
    );

    // Stop while a slow start is in flight.
    let sup2 = Supervisor::new(None);
    let mut c = cfg("slow_load");
    c.ready_timeout = Duration::from_secs(30);
    let s2 = sup2.clone();
    let starting = tokio::spawn(async move { s2.start(c).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    sup2.stop().await;
    let _ = starting.await.unwrap();
    assert_eq!(sup2.state(), RuntimeState::Stopped);
}

#[tokio::test]
async fn server_json_is_written_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let json = dir.path().join("runtime/server.json");
    let sup = Supervisor::with_files(None, Some(json.clone()));
    sup.start(cfg("ok")).await.unwrap();
    let info = nexus_llm::reaper::ServerInfo::read(&json).expect("server.json after start");
    assert_eq!(info.exe, std::path::PathBuf::from(BIN));
    assert!(info.pid > 0 && info.port > 0);
    sup.shutdown().await;
    assert!(!json.exists());
}

#[tokio::test]
async fn start_reaps_an_orphan_from_a_previous_run() {
    let dir = tempfile::tempdir().unwrap();
    let json = dir.path().join("server.json");
    // Simulate a crashed app: a live mock server recorded in server.json.
    let mut orphan = std::process::Command::new(BIN)
        .args(["-m", "ok", "--port", "0"])
        .env("LLAMA_API_KEY", "x")
        .spawn()
        .unwrap();
    // port 0 is rejected by the mock only if bind fails; give it a moment, then record it.
    tokio::time::sleep(Duration::from_millis(300)).await;
    nexus_llm::reaper::ServerInfo::new(orphan.id(), 0, std::path::PathBuf::from(BIN))
        .write(&json)
        .unwrap();
    let sup = Supervisor::with_files(None, Some(json.clone()));
    sup.start(cfg("ok")).await.unwrap();
    let status = tokio::task::spawn_blocking(move || orphan.wait().unwrap())
        .await
        .unwrap();
    assert!(!status.success(), "orphan should have been killed");
    sup.shutdown().await;
}
