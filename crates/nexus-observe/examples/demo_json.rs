//! Prints the demo `ObserveView` as JSON (for UI work and screenshots without the app).
//!
//! cargo run -q -p nexus-observe --example demo_json > demo.json

use nexus_observe::{Hub, Switches};

fn main() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    let sw = Switches {
        nanonexus: true,
        claude_code: false,
        codex: false,
        window_hours: 6,
        alerts: false,
    };
    let hub = Hub::default();
    hub.set_demo(&sw, true);
    // Poll once a second for 15 s so the per-agent rates have filled up, as they would in the app.
    let mut last = hub.poll(&sw, now).0;
    for i in 1..=15 {
        last = hub.poll(&sw, now + i * 1_000).0;
    }
    let v = last;
    match serde_json::to_string(&v) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("{e}"),
    }
}
