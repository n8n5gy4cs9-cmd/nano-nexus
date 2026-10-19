//! Terminal view of the Activity Observatory (T19.16).
//!
//! cargo run -p nexus-observe --example top                 # NanoNexus is not running here: Claude Code + Codex
//! cargo run -p nexus-observe --example top -- --demo       # synthetic data
//! cargo run -p nexus-observe --example top -- --once       # print one frame and exit
//! cargo run -p nexus-observe --example top -- --claude-only | --codex-only
//!
//! Read-only: it only reads the agents' own transcript files and prints counts, names and times.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nexus_observe::observer::{default_claude_root, default_codex_root};
use nexus_observe::render::table;
use nexus_observe::{Observer, ObserverConfig};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let cfg = ObserverConfig {
        window_hours: 6,
        nanonexus: false,
        claude_root: (!has("--codex-only")).then(default_claude_root).flatten(),
        codex_root: (!has("--claude-only")).then(default_codex_root).flatten(),
        demo: has("--demo"),
    };
    let mut observer = Observer::new(cfg);
    loop {
        let (snap, _) = observer.poll(now_ms());
        if !has("--once") {
            print!("\x1b[2J\x1b[H");
        }
        println!("{}", table(&snap));
        if has("--once") {
            return;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}
