#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support;

use std::time::{Duration, Instant};

use futures_util::StreamExt;
use nexus_llm::{
    ChatMessage, ChatRequest, LlmClient, LlmError, Priority, Scheduler, StreamEvent, Supervisor,
};
use secrecy::SecretString;
use support::mock_server::{MockConfig, spawn};
use tokio_util::sync::CancellationToken;

fn req(text: &str) -> ChatRequest {
    ChatRequest::new(vec![ChatMessage::user(text)])
}

async fn setup(cfg: MockConfig) -> (Scheduler, std::sync::Arc<support::mock_server::Shared>) {
    let token = cfg.token.clone();
    let (port, shared) = spawn(cfg).await;
    let client = LlmClient::new(port, SecretString::from(token)).unwrap();
    (Scheduler::with_client(client), shared)
}

fn order(shared: &support::mock_server::Shared) -> Vec<String> {
    shared
        .chat_requests()
        .iter()
        .map(|r| {
            r["messages"][0]["content"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

#[tokio::test]
async fn interactive_jumps_the_background_queue_but_does_not_preempt() {
    let (sched, shared) = setup(MockConfig::new("t")).await;
    let spawn_job = |prio, text: &'static str| {
        let s = sched.clone();
        tokio::spawn(async move { s.complete(prio, req(text), CancellationToken::new()).await })
    };
    let b1 = spawn_job(Priority::Background, "SLOW-b1");
    tokio::time::sleep(Duration::from_millis(100)).await; // b1 is in flight
    let b2 = spawn_job(Priority::Background, "SLOW-b2");
    let b3 = spawn_job(Priority::Background, "SLOW-b3");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let i = spawn_job(Priority::Interactive, "SLOW-chat");
    for h in [b1, b2, b3, i] {
        assert_eq!(h.await.unwrap().unwrap(), "ok");
    }
    assert_eq!(order(&shared), vec!["SLOW-b1", "SLOW-chat", "SLOW-b2", "SLOW-b3"]);
}

#[tokio::test]
async fn background_queue_is_bounded() {
    let (sched, _) = setup(MockConfig::new("t")).await;
    let mut handles = Vec::new();
    // 1 in flight + 8 queued fit.
    for n in 0..9 {
        let s = sched.clone();
        handles.push(tokio::spawn(async move {
            s.complete(
                Priority::Background,
                req(&format!("SLOW-{n}")),
                CancellationToken::new(),
            )
            .await
        }));
        if n == 0 {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    match sched
        .complete(
            Priority::Background,
            req("one too many"),
            CancellationToken::new(),
        )
        .await
    {
        Err(LlmError::Busy) => {}
        other => panic!("expected Busy, got {other:?}"),
    }
    // Interactive is unaffected by a full background queue.
    let started = Instant::now();
    let r = sched
        .complete(Priority::Interactive, req("hi"), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(r, "ok");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "interactive waited behind the whole background queue"
    );
    for h in handles {
        h.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn complete_wait_awaits_capacity() {
    let (sched, _) = setup(MockConfig::new("t")).await;
    let mut handles = Vec::new();
    for n in 0..12 {
        let s = sched.clone();
        handles.push(tokio::spawn(async move {
            s.complete_wait(
                Priority::Background,
                req(&format!("job-{n}")),
                CancellationToken::new(),
            )
            .await
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap().unwrap(), "ok");
    }
}

#[tokio::test]
async fn cancelling_a_queued_job_never_runs_it() {
    let (sched, shared) = setup(MockConfig::new("t")).await;
    let s = sched.clone();
    let first = tokio::spawn(async move {
        s.complete(Priority::Background, req("SLOW-first"), CancellationToken::new())
            .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let cancel = CancellationToken::new();
    let (s, c) = (sched.clone(), cancel.clone());
    let queued = tokio::spawn(async move { s.complete(Priority::Background, req("never-runs"), c).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    cancel.cancel();
    assert!(matches!(queued.await.unwrap(), Err(LlmError::Cancelled)));
    first.await.unwrap().unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(order(&shared), vec!["SLOW-first"]);
}

#[tokio::test]
async fn cancelling_an_inflight_job_frees_the_lane() {
    let (sched, _) = setup(MockConfig::new("t")).await;
    let cancel = CancellationToken::new();
    let (s, c) = (sched.clone(), cancel.clone());
    let h = tokio::spawn(async move { s.complete(Priority::Background, req("SLOW-x"), c).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let t = Instant::now();
    cancel.cancel();
    assert!(matches!(h.await.unwrap(), Err(LlmError::Cancelled)));
    let r = sched
        .complete(Priority::Interactive, req("next"), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(r, "ok");
    assert!(
        t.elapsed() < Duration::from_millis(250),
        "lane stayed busy after cancel"
    );
}

#[tokio::test]
async fn streams_events_and_releases_the_lane_on_drop() {
    let mut cfg = MockConfig::new("t");
    cfg.sse_chunks = vec![
        b"data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n".to_vec(),
        b"data: {\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\n".to_vec(),
        b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".to_vec(),
    ];
    let (sched, _) = setup(cfg).await;
    let mut s = sched
        .stream(Priority::Interactive, req("hi"), CancellationToken::new())
        .await
        .unwrap();
    let mut text = String::new();
    let mut done = false;
    while let Some(ev) = s.next().await {
        match ev.unwrap() {
            StreamEvent::Content(c) => text.push_str(&c),
            StreamEvent::Done { .. } => done = true,
            StreamEvent::Reasoning(_) => {}
        }
    }
    assert_eq!(text, "ab");
    assert!(done);

    // Dropping a stream early frees the lane.
    let s2 = sched
        .stream(Priority::Interactive, req("again"), CancellationToken::new())
        .await
        .unwrap();
    drop(s2);
    let r = tokio::time::timeout(
        Duration::from_secs(2),
        sched.complete(Priority::Interactive, req("after-drop"), CancellationToken::new()),
    )
    .await
    .expect("lane never freed")
    .unwrap();
    assert_eq!(r, "ok");
}

#[tokio::test]
async fn runtime_not_ready_fails_fast() {
    let sched = Scheduler::new(Supervisor::new(None));
    let t = Instant::now();
    match sched
        .complete(Priority::Interactive, req("hi"), CancellationToken::new())
        .await
    {
        Err(LlmError::RuntimeUnavailable) => {}
        other => panic!("{other:?}"),
    }
    assert!(
        sched
            .stream(Priority::Background, req("hi"), CancellationToken::new())
            .await
            .is_err()
    );
    assert!(t.elapsed() < Duration::from_millis(200));
}
