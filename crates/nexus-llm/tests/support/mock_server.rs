//! A small axum imitation of llama-server for tests (INFERENCE-CONTRACT §8.1).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use tokio::net::TcpListener;

/// Behaviour knobs.
#[derive(Clone)]
pub struct MockConfig {
    pub token: String,
    pub model_id: String,
    /// `/health` answers 503 this many times before 200.
    pub loading_polls: u32,
    /// Raw byte chunks of the SSE body for streaming requests.
    pub sse_chunks: Vec<Vec<u8>>,
    /// Scripted `message.content` values for non-streaming requests (last one repeats).
    pub replies: Vec<String>,
}

impl MockConfig {
    pub fn new(token: &str) -> Self {
        Self {
            token: token.to_owned(),
            model_id: "mock.gguf".into(),
            loading_polls: 0,
            sse_chunks: Vec::new(),
            replies: vec!["ok".into()],
        }
    }
}

pub struct Shared {
    cfg: MockConfig,
    health_calls: AtomicU32,
    replies: Mutex<VecDeque<String>>,
    pub requests: Mutex<Vec<Value>>,
}

impl Shared {
    pub fn chat_requests(&self) -> Vec<Value> {
        self.requests.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

fn authorized(s: &Shared, h: &HeaderMap) -> bool {
    h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {}", s.cfg.token))
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": {"code": 401, "message": "Invalid API Key"}})),
    )
        .into_response()
}

async fn health(State(s): State<Arc<Shared>>) -> Response {
    let n = s.health_calls.fetch_add(1, Ordering::SeqCst);
    if n < s.cfg.loading_polls {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": {"code": 503, "message": "Loading model"}})),
        )
            .into_response()
    } else {
        Json(json!({"status": "ok"})).into_response()
    }
}

async fn models(State(s): State<Arc<Shared>>, h: HeaderMap) -> Response {
    if !authorized(&s, &h) {
        return unauthorized();
    }
    Json(json!({"data": [{"id": s.cfg.model_id, "meta": {"n_ctx_train": 32768}}]})).into_response()
}

async fn tokenize(State(s): State<Arc<Shared>>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if !authorized(&s, &h) {
        return unauthorized();
    }
    let n = b["content"]
        .as_str()
        .map(|c| c.split_whitespace().count())
        .unwrap_or(0);
    Json(json!({"tokens": (0..n).collect::<Vec<_>>()})).into_response()
}

async fn chat(State(s): State<Arc<Shared>>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if !authorized(&s, &h) {
        return unauthorized();
    }
    if let Ok(mut r) = s.requests.lock() {
        r.push(b.clone());
    }
    if b.to_string().contains("SLOW") {
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    if b.to_string().contains("OVERFLOW") {
        let body = json!({"error": {"code": 400, "type": "exceed_context_size_error",
            "message": "the request exceeds the available context size, try increasing it",
            "n_prompt_tokens": 9000, "n_ctx": 8192}});
        return (StatusCode::BAD_REQUEST, Json(body)).into_response();
    }
    if b["stream"] == true {
        let chunks = s.cfg.sse_chunks.clone();
        let stream = futures_util::stream::iter(chunks).then(|c| async move {
            tokio::time::sleep(Duration::from_millis(2)).await;
            Ok::<_, std::convert::Infallible>(bytes::Bytes::from(c))
        });
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(stream))
            .unwrap_or_default();
    }
    let content = {
        let mut q = s.replies.lock().unwrap_or_else(|e| e.into_inner());
        if q.len() > 1 {
            q.pop_front().unwrap_or_default()
        } else {
            q.front().cloned().unwrap_or_default()
        }
    };
    Json(json!({
        "choices": [{"message": {"role": "assistant", "content": content}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    }))
    .into_response()
}

use futures_util::StreamExt;

pub fn router(cfg: MockConfig) -> (Router, Arc<Shared>) {
    let shared = Arc::new(Shared {
        replies: Mutex::new(cfg.replies.iter().cloned().collect()),
        cfg,
        health_calls: AtomicU32::new(0),
        requests: Mutex::new(Vec::new()),
    });
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/models", get(models))
        .route("/tokenize", post(tokenize))
        .route("/v1/chat/completions", post(chat))
        .with_state(shared.clone());
    (app, shared)
}

/// Bind `127.0.0.1:0`, serve in the background, return the port.
pub async fn spawn(cfg: MockConfig) -> (u16, Arc<Shared>) {
    let (app, shared) = router(cfg);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (port, shared)
}
