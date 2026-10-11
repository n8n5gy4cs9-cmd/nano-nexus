#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use nexus_agent::{SummaryTurn, generate_title, summarize_history};
use nexus_llm::{LlmClient, Scheduler};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

type Replies = Arc<Mutex<Vec<String>>>;
type Seen = Arc<Mutex<Vec<Value>>>;

async fn serve(replies: Vec<&str>) -> (Scheduler, Seen) {
    let replies: Replies = Arc::new(Mutex::new(replies.into_iter().rev().map(String::from).collect()));
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    async fn chat(State((r, s)): State<(Replies, Seen)>, Json(body): Json<Value>) -> Json<Value> {
        s.lock().unwrap().push(body);
        let content = r.lock().unwrap().pop().unwrap_or_else(|| "{}".into());
        Json(json!({"choices": [{"message": {"content": content}, "finish_reason": "stop"}]}))
    }
    let app = Router::new()
        .route("/v1/chat/completions", post(chat))
        .with_state((replies, seen.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = LlmClient::with_base(
        format!("http://127.0.0.1:{port}"),
        SecretString::from("t".to_owned()),
    )
    .unwrap();
    (Scheduler::with_client(client), seen)
}

#[tokio::test]
async fn title_from_the_model() {
    let (sched, seen) = serve(vec![r#"{"title":"Godot Signal Debugging"}"#]).await;
    let t = generate_title(
        &sched,
        "my signal never fires",
        "Check the connection.",
        CancellationToken::new(),
    )
    .await;
    assert_eq!(t, "Godot Signal Debugging");
    let body = &seen.lock().unwrap()[0];
    assert_eq!(body["response_format"]["json_schema"]["name"], "title");
    assert_eq!(body["max_tokens"], 40);
    assert!(
        body["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("<user>my signal never fires</user>")
    );
}

#[tokio::test]
async fn invalid_titles_fall_back_to_the_first_words() {
    let (sched, seen) = serve(vec![r#"{"title":"x"}"#, r#"{"title":"y"}"#]).await;
    let t = generate_title(
        &sched,
        "how do I rotate a quaternion in godot",
        "…",
        CancellationToken::new(),
    )
    .await;
    assert_eq!(t, "How Do I Rotate A Quaternion");
    assert_eq!(seen.lock().unwrap().len(), 2, "one re-ask, then the fallback");
}

#[tokio::test]
async fn a_dead_runtime_still_gives_a_title() {
    let sched = Scheduler::new(nexus_llm::Supervisor::new(None));
    let t = generate_title(&sched, "plan my week", "ok", CancellationToken::new()).await;
    assert_eq!(t, "Plan My Week");
}

#[tokio::test]
async fn summary_uses_the_previous_summary_and_trims() {
    let (sched, seen) = serve(vec!["  The user is learning Godot signals.  "]).await;
    let turns = vec![
        SummaryTurn {
            role: "user".into(),
            content: "what is a signal".into(),
        },
        SummaryTurn {
            role: "assistant".into(),
            content: "an event".into(),
        },
    ];
    let s = summarize_history(
        &sched,
        Some("Earlier: they installed Godot."),
        &turns,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(s, "The user is learning Godot signals.");
    let user = seen.lock().unwrap()[0]["messages"][1]["content"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(user.contains("<previous_summary>Earlier: they installed Godot.</previous_summary>"));
    assert!(user.contains("[user] what is a signal"));
}

#[tokio::test]
async fn an_empty_summary_is_an_error() {
    let (sched, _) = serve(vec!["   "]).await;
    assert!(
        summarize_history(&sched, None, &[], CancellationToken::new())
            .await
            .is_err()
    );
}
