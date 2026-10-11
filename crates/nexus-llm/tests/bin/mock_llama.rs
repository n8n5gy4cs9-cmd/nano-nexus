//! Fake `llama-server` for supervisor tests. The behaviour is chosen by the `-m` argument (the
//! supervisor clears the environment, so env vars can't carry a mode):
//!
//! `ok` · `slow_load` · `exit_early` · `bind_fail` · `wrong_token` · `always_crash` ·
//! `bind_fail_once:<marker>` · `crash_once:<marker>`
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "../support/mock_server.rs"]
mod mock_server;

use std::path::Path;
use std::time::Duration;

use mock_server::MockConfig;

fn arg(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// True the first time it is called for `marker` (creates the file).
fn first_time(marker: &str) -> bool {
    if Path::new(marker).exists() {
        return false;
    }
    std::fs::write(marker, "x").unwrap();
    true
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let port: u16 = arg(&args, "--port").and_then(|p| p.parse().ok()).expect("--port");
    let mode = arg(&args, "-m").unwrap_or_else(|| "ok".into());
    let token = std::env::var("LLAMA_API_KEY").unwrap_or_default();

    let mut cfg = MockConfig::new(&token);
    let mut crash_after: Option<Duration> = None;
    match mode.as_str() {
        "exit_early" => {
            eprintln!("fatal: simulated crash during load");
            std::process::exit(1);
        }
        "bind_fail" => {
            eprintln!("main: couldn't bind HTTP server socket, hostname: 127.0.0.1, port: {port}");
            std::process::exit(1);
        }
        "slow_load" => cfg.loading_polls = 4,
        "wrong_token" => cfg.token = "not-the-token".into(),
        "always_crash" => crash_after = Some(Duration::from_millis(600)),
        m if m.starts_with("bind_fail_once:") => {
            if first_time(&m["bind_fail_once:".len()..]) {
                eprintln!("main: couldn't bind HTTP server socket, hostname: 127.0.0.1, port: {port}");
                std::process::exit(1);
            }
        }
        m if m.starts_with("crash_once:") && first_time(&m["crash_once:".len()..]) => {
            crash_after = Some(Duration::from_millis(600));
        }
        _ => {}
    }

    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("main: couldn't bind HTTP server socket: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("mock-llama listening on {port} (mode {mode})");
    if let Some(d) = crash_after {
        tokio::spawn(async move {
            tokio::time::sleep(d).await;
            eprintln!("fatal: simulated crash");
            std::process::exit(1);
        });
    }
    let (app, _) = mock_server::router(cfg);
    axum::serve(listener, app).await.expect("serve");
}
