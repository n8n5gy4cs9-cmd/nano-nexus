//! Stream one answer from the local model.
//!
//! `cargo run -p nexus-llm --example chat -- "Explain Rust ownership in 3 bullets"`
//! (needs `scripts/setup-runtime.sh` and `node scripts/fetch-model.mjs` first; or set
//! `NEXUS_LLAMA_BIN` / `NEXUS_MODEL`).

mod common;

use std::io::Write;

use futures_util::StreamExt;
use nexus_llm::{ChatMessage, ChatRequest, Priority, Scheduler, StreamEvent};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Say hi in 5 words.".into());
    let sup = common::start().await?;
    let sched = Scheduler::new(sup.clone());

    let req = ChatRequest::new(vec![ChatMessage::user(prompt)]).streaming();
    let mut events = sched
        .stream(Priority::Interactive, req, CancellationToken::new())
        .await?;
    let mut out = std::io::stdout();
    while let Some(ev) = events.next().await {
        match ev? {
            StreamEvent::Content(t) => {
                write!(out, "{t}")?;
                out.flush()?;
            }
            StreamEvent::Reasoning(_) => {}
            StreamEvent::Done { timings, .. } => {
                writeln!(out)?;
                if let Some(t) = timings {
                    eprintln!("[{} tokens, {:.1} tok/s]", t.predicted_n, t.predicted_per_second);
                }
            }
        }
    }
    sup.shutdown().await;
    Ok(())
}
