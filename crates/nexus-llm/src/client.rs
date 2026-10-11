//! HTTP client for `llama-server` (INFERENCE-CONTRACT §4).

use std::collections::VecDeque;
use std::pin::Pin;
use std::time::Duration;

use futures_util::stream::{self, Stream, StreamExt};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};

use crate::error::LlmError;
use crate::sse::{SseDecoder, SseItem};
use crate::think::{Segment, ThinkSplitter};
use crate::types::{ChatChunk, ChatRequest, ChatResponse, ModelsResponse, StreamEvent, Timings, Usage};

/// A boxed stream of chat events.
pub type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, LlmError>> + Send>>;

/// Result of `GET /health`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Ok,
    /// 503: the model is still loading.
    Loading,
}

/// Talks to one `llama-server` instance on loopback.
#[derive(Clone)]
pub struct LlmClient {
    http: reqwest::Client,
    base: String,
    token: SecretString,
}

impl std::fmt::Debug for LlmClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmClient")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl LlmClient {
    /// Client for `http://127.0.0.1:<port>` authenticating with `token`.
    pub fn new(port: u16, token: SecretString) -> Result<Self, LlmError> {
        Self::with_base(format!("http://127.0.0.1:{port}"), token)
    }

    /// Client for an explicit base URL (tests).
    pub fn with_base(base: String, token: SecretString) -> Result<Self, LlmError> {
        // Loopback must never go through a proxy; no overall timeout so streams can run long.
        let http = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(2))
            .pool_idle_timeout(Duration::from_secs(30))
            .build()
            .map_err(LlmError::from)?;
        Ok(Self { http, base, token })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        rb.bearer_auth(self.token.expose_secret())
    }

    /// `GET /health` (unauthenticated): `Ok` on 200, `Loading` on 503.
    pub async fn health(&self) -> Result<Health, LlmError> {
        let resp = self
            .http
            .get(self.url("/health"))
            .timeout(Duration::from_secs(5))
            .send()
            .await?;
        match resp.status().as_u16() {
            200 => Ok(Health::Ok),
            503 => Ok(Health::Loading),
            status => Err(LlmError::Http {
                status,
                body: resp.text().await.unwrap_or_default(),
            }),
        }
    }

    /// `GET /v1/models` with the bearer token. A 401 means the port is not our server.
    pub async fn models(&self) -> Result<ModelsResponse, LlmError> {
        let resp = self
            .auth(self.http.get(self.url("/v1/models")))
            .timeout(Duration::from_secs(5))
            .send()
            .await?;
        let resp = check(resp).await?;
        resp.json()
            .await
            .map_err(|e| LlmError::Protocol(format!("bad /v1/models body: {e}")))
    }

    /// `POST /tokenize`: token ids for `text` (no special tokens).
    pub async fn tokenize(&self, text: &str) -> Result<Vec<u32>, LlmError> {
        let resp = self
            .auth(self.http.post(self.url("/tokenize")))
            .timeout(Duration::from_secs(10))
            .json(&json!({ "content": text, "add_special": false }))
            .send()
            .await?;
        let v: Value = check(resp)
            .await?
            .json()
            .await
            .map_err(|e| LlmError::Protocol(e.to_string()))?;
        v.get("tokens")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|t| t.as_u64().map(|n| n as u32)).collect())
            .ok_or_else(|| LlmError::Protocol("tokenize response has no `tokens`".into()))
    }

    /// Non-streaming chat completion.
    pub async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let mut req = req.clone();
        req.stream = false;
        req.stream_options = None;
        let resp = self
            .auth(self.http.post(self.url("/v1/chat/completions")))
            .json(&req)
            .send()
            .await?;
        check(resp)
            .await?
            .json()
            .await
            .map_err(|e| LlmError::Protocol(format!("bad chat body: {e}")))
    }

    /// Streaming chat completion. Thoughts arrive as `Reasoning`, answer text as `Content`; the
    /// stream ends with exactly one `Done`.
    pub async fn chat_stream(&self, req: &ChatRequest) -> Result<EventStream, LlmError> {
        let mut req = req.clone();
        if !req.stream {
            req = req.streaming();
        }
        let resp = self
            .auth(self.http.post(self.url("/v1/chat/completions")))
            .json(&req)
            .send()
            .await?;
        let resp = check(resp).await?;
        Ok(Box::pin(events(
            resp.bytes_stream().map(|r| r.map_err(LlmError::from)),
        )))
    }
}

/// Turn non-2xx responses into `LlmError`, mapping the context-overflow 400.
async fn check(resp: reqwest::Response) -> Result<reqwest::Response, LlmError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    Err(map_http_error(status.as_u16(), body))
}

pub(crate) fn map_http_error(status: u16, body: String) -> LlmError {
    if status == 400 && body.contains("exceeds the available context size") {
        let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        let n = |k: &str| v["error"][k].as_u64().unwrap_or(0) as u32;
        return LlmError::ContextOverflow {
            need: n("n_prompt_tokens"),
            have: n("n_ctx"),
        };
    }
    LlmError::Http { status, body }
}

struct StreamState<S> {
    bytes: S,
    sse: SseDecoder,
    think: ThinkSplitter,
    pending: VecDeque<StreamEvent>,
    finish_reason: Option<String>,
    usage: Option<Usage>,
    timings: Option<Timings>,
    finished: bool,
}

impl<S> StreamState<S> {
    fn route(&mut self, segs: Vec<Segment>) {
        for s in segs {
            self.pending.push_back(match s {
                Segment::Content(t) => StreamEvent::Content(t),
                Segment::Reasoning(t) => StreamEvent::Reasoning(t),
            });
        }
    }

    fn handle(&mut self, items: Vec<SseItem>) -> Result<(), LlmError> {
        for item in items {
            match item {
                SseItem::Done => self.finish(),
                SseItem::Data(payload) => {
                    let chunk: ChatChunk = serde_json::from_str(&payload)
                        .map_err(|e| LlmError::Protocol(format!("bad stream chunk: {e}")))?;
                    if chunk.usage.is_some() {
                        self.usage = chunk.usage;
                    }
                    if chunk.timings.is_some() {
                        self.timings = chunk.timings;
                    }
                    for choice in chunk.choices {
                        if let Some(r) = choice.delta.reasoning_content.filter(|s| !s.is_empty()) {
                            self.pending.push_back(StreamEvent::Reasoning(r));
                        }
                        if let Some(c) = choice.delta.content.filter(|s| !s.is_empty()) {
                            let segs = self.think.push(&c);
                            self.route(segs);
                        }
                        if choice.finish_reason.is_some() {
                            self.finish_reason = choice.finish_reason;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        let tail = self.think.finish();
        self.route(tail);
        self.pending.push_back(StreamEvent::Done {
            finish_reason: self.finish_reason.take(),
            usage: self.usage.take(),
            timings: self.timings.take(),
        });
    }
}

/// Decode a byte stream of SSE into chat events.
pub(crate) fn events<S>(bytes: S) -> impl Stream<Item = Result<StreamEvent, LlmError>> + Send
where
    S: Stream<Item = Result<bytes::Bytes, LlmError>> + Send + Unpin + 'static,
{
    let state = StreamState {
        bytes,
        sse: SseDecoder::new(),
        think: ThinkSplitter::new(),
        pending: VecDeque::new(),
        finish_reason: None,
        usage: None,
        timings: None,
        finished: false,
    };
    stream::unfold(Some(state), |st| async move {
        let mut st = st?;
        loop {
            if let Some(ev) = st.pending.pop_front() {
                return Some((Ok(ev), Some(st)));
            }
            if st.finished {
                return None;
            }
            match st.bytes.next().await {
                Some(Ok(b)) => {
                    let items = st.sse.push(&b);
                    if let Err(e) = st.handle(items) {
                        return Some((Err(e), None));
                    }
                }
                Some(Err(e)) => return Some((Err(e), None)),
                None => {
                    let items = st.sse.finish();
                    if let Err(e) = st.handle(items) {
                        return Some((Err(e), None));
                    }
                    st.finish();
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    async fn collect(chunks: &[&str]) -> Vec<StreamEvent> {
        let s = stream::iter(
            chunks
                .iter()
                .map(|c| Ok(Bytes::from(c.to_string())))
                .collect::<Vec<_>>(),
        );
        events(s).map(|r| r.unwrap()).collect().await
    }

    #[tokio::test]
    async fn content_reasoning_and_done() {
        let ev = collect(&[
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hm\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3},\"timings\":{\"predicted_per_second\":9.0}}\n\ndata: [DONE]\n\n",
        ])
        .await;
        assert_eq!(ev[0], StreamEvent::Reasoning("hm".into()));
        assert_eq!(ev[1], StreamEvent::Content("Hel".into()));
        assert_eq!(ev[2], StreamEvent::Content("lo".into()));
        match &ev[3] {
            StreamEvent::Done {
                finish_reason,
                usage,
                timings,
            } => {
                assert_eq!(finish_reason.as_deref(), Some("stop"));
                assert_eq!(usage.as_ref().map(|u| u.total_tokens), Some(3));
                assert_eq!(timings.as_ref().map(|t| t.predicted_per_second), Some(9.0));
            }
            other => panic!("expected Done, got {other:?}"),
        }
        assert_eq!(ev.len(), 4);
    }

    #[tokio::test]
    async fn think_tags_split_across_chunks_become_reasoning() {
        let ev = collect(&[
            "data: {\"choices\":[{\"delta\":{\"content\":\"<thi\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"nk>plan\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"</think>ans\"}}]}\n\ndata: [DONE]\n\n",
        ])
        .await;
        assert_eq!(
            &ev[..3],
            &[
                StreamEvent::Reasoning("plan".into()),
                StreamEvent::Content("ans".into()),
                StreamEvent::Done {
                    finish_reason: None,
                    usage: None,
                    timings: None
                }
            ]
        );
    }

    #[tokio::test]
    async fn stream_without_done_still_ends_with_done() {
        let ev = collect(&["data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n"]).await;
        assert_eq!(ev.len(), 2);
        assert!(matches!(ev[1], StreamEvent::Done { .. }));
    }

    #[tokio::test]
    async fn bad_json_is_a_protocol_error() {
        let s = stream::iter(vec![Ok(Bytes::from("data: {nope\n\n"))]);
        let r: Vec<_> = events(s).collect().await;
        assert!(matches!(r[0], Err(LlmError::Protocol(_))));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn context_overflow_is_mapped() {
        let body = r#"{"error":{"code":400,"message":"the request exceeds the available context size, try increasing it","type":"exceed_context_size_error","n_prompt_tokens":9000,"n_ctx":8192}}"#;
        match map_http_error(400, body.into()) {
            LlmError::ContextOverflow { need, have } => assert_eq!((need, have), (9000, 8192)),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            map_http_error(500, "boom".into()),
            LlmError::Http { status: 500, .. }
        ));
    }
}
