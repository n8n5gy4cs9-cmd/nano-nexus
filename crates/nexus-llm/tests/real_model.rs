//! Real-model smoke tests. They need `vendor/` from M01 and are run on the Mac with
//! `cargo test -p nexus-llm -- --ignored` (timings go in docs/SPIKES.md).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use nexus_core::events::RuntimeState;
use nexus_llm::{
    ChatMessage, ChatRequest, Priority, RuntimeConfig, Scheduler, StreamEvent, Structured, StructuredSpec,
    Supervisor, Validate, ValidationCtx, chat_json,
};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bin() -> PathBuf {
    std::env::var_os("NEXUS_LLAMA_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("vendor/runtime/macos-arm64/llama-server-aarch64-apple-darwin"))
}

fn model() -> PathBuf {
    if let Some(p) = std::env::var_os("NEXUS_MODEL") {
        return PathBuf::from(p);
    }
    let mut stack = vec![root().join("vendor/models")];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "gguf") {
                return p;
            }
        }
    }
    panic!("no model under vendor/models; run `node scripts/fetch-model.mjs` or set NEXUS_MODEL");
}

async fn started() -> (Supervisor, Duration) {
    let sup = Supervisor::new(None);
    let t = Instant::now();
    sup.start(RuntimeConfig::new(bin(), model())).await.unwrap();
    (sup, t.elapsed())
}

#[tokio::test]
#[ignore = "needs the real runtime and model (run on the Mac)"]
async fn becomes_ready_in_under_10_seconds() {
    let (sup, took) = started().await;
    println!("ready in {took:?}");
    assert!(matches!(sup.state(), RuntimeState::Ready { .. }));
    assert!(took < Duration::from_secs(10), "took {took:?}");
    sup.shutdown().await;
}

#[tokio::test]
#[ignore = "needs the real runtime and model (run on the Mac)"]
async fn streams_a_reply() {
    let (sup, _) = started().await;
    let sched = Scheduler::new(sup.clone());
    let req = ChatRequest::new(vec![ChatMessage::user("Say hi in 5 words.")]).streaming();
    let mut s = sched
        .stream(Priority::Interactive, req, CancellationToken::new())
        .await
        .unwrap();
    let (mut text, mut done) = (String::new(), false);
    let t = Instant::now();
    let mut first = None;
    while let Some(ev) = s.next().await {
        match ev.unwrap() {
            StreamEvent::Content(c) => {
                first.get_or_insert_with(|| t.elapsed());
                text.push_str(&c);
            }
            StreamEvent::Done { timings, .. } => {
                done = true;
                println!("timings: {timings:?}");
            }
            StreamEvent::Reasoning(_) => {}
        }
    }
    println!("ttft {first:?}, text {text:?}");
    assert!(done && !text.trim().is_empty());
    sup.shutdown().await;
}

#[derive(Debug, Deserialize)]
struct Title {
    title: String,
}

impl Validate for Title {
    fn validate(&self, _: &ValidationCtx) -> Result<(), Vec<String>> {
        if (3..=48).contains(&self.title.chars().count()) {
            Ok(())
        } else {
            Err(vec!["bad length".into()])
        }
    }
}

#[tokio::test]
#[ignore = "needs the real runtime and model (run on the Mac)"]
async fn returns_schema_valid_json() {
    let (sup, _) = started().await;
    let sched = Scheduler::new(sup.clone());
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../schemas/llm/title.schema.json")).unwrap();
    let spec = StructuredSpec {
        name: "title",
        schema: &schema,
        temperature: 0.3,
        max_tokens: 64,
    };
    let out: Structured<Title> = chat_json(
        &sched,
        Priority::Interactive,
        vec![ChatMessage::user(
            "Give a title for a chat about baking sourdough bread.",
        )],
        spec,
        &ValidationCtx::default(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    match out {
        Structured::Valid(t) => println!("title: {}", t.title),
        Structured::Fallback { reason } => panic!("fell back: {reason}"),
    }
    sup.shutdown().await;
}
