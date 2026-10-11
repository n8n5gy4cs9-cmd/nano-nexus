#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support;

use futures_util::StreamExt;
use nexus_llm::{ChatMessage, ChatRequest, Health, LlmClient, LlmError, StreamEvent};
use secrecy::SecretString;
use support::mock_server::{MockConfig, spawn};

fn client(port: u16, token: &str) -> LlmClient {
    LlmClient::new(port, SecretString::from(token.to_owned())).unwrap()
}

#[tokio::test]
async fn health_goes_from_loading_to_ok() {
    let mut cfg = MockConfig::new("tok");
    cfg.loading_polls = 2;
    let (port, _) = spawn(cfg).await;
    let c = client(port, "tok");
    assert_eq!(c.health().await.unwrap(), Health::Loading);
    assert_eq!(c.health().await.unwrap(), Health::Loading);
    assert_eq!(c.health().await.unwrap(), Health::Ok);
}

#[tokio::test]
async fn models_checks_auth() {
    let (port, _) = spawn(MockConfig::new("good")).await;
    let m = client(port, "good").models().await.unwrap();
    assert_eq!(m.data[0].id, "mock.gguf");
    match client(port, "bad").models().await {
        Err(LlmError::Http { status: 401, .. }) => {}
        other => panic!("expected 401, got {other:?}"),
    }
}

#[tokio::test]
async fn tokenize_returns_ids() {
    let (port, _) = spawn(MockConfig::new("t")).await;
    assert_eq!(client(port, "t").tokenize("a b c").await.unwrap().len(), 3);
}

#[tokio::test]
async fn chat_non_stream_returns_text_and_sends_contract_fields() {
    let mut cfg = MockConfig::new("t");
    cfg.replies = vec!["hello there".into()];
    let (port, shared) = spawn(cfg).await;
    let resp = client(port, "t")
        .chat(&ChatRequest::new(vec![ChatMessage::user("hi")]))
        .await
        .unwrap();
    assert_eq!(resp.text(), "hello there");
    let sent = &shared.chat_requests()[0];
    assert_eq!(sent["stream"], false);
    assert_eq!(sent["min_p"], 0.0);
    assert_eq!(sent["chat_template_kwargs"]["enable_thinking"], false);
}

#[tokio::test]
async fn chat_stream_with_split_chunks() {
    let mut cfg = MockConfig::new("t");
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"H\u{e9}llo\"}}]}\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\" w\"}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    // Split at awkward byte positions (inside the multi-byte char and inside a line).
    let bytes = body.as_bytes();
    cfg.sse_chunks = vec![
        bytes[..40].to_vec(),
        bytes[40..43].to_vec(),
        bytes[43..120].to_vec(),
        bytes[120..].to_vec(),
    ];
    let (port, _) = spawn(cfg).await;
    let mut s = client(port, "t")
        .chat_stream(&ChatRequest::new(vec![ChatMessage::user("hi")]))
        .await
        .unwrap();
    let mut text = String::new();
    let mut done = 0;
    while let Some(ev) = s.next().await {
        match ev.unwrap() {
            StreamEvent::Content(c) => text.push_str(&c),
            StreamEvent::Done { finish_reason, .. } => {
                assert_eq!(finish_reason.as_deref(), Some("stop"));
                done += 1;
            }
            StreamEvent::Reasoning(_) => panic!("unexpected reasoning"),
        }
    }
    assert_eq!(text, "H\u{e9}llo w");
    assert_eq!(done, 1);
}

#[tokio::test]
async fn context_overflow_is_mapped() {
    let (port, _) = spawn(MockConfig::new("t")).await;
    match client(port, "t")
        .chat(&ChatRequest::new(vec![ChatMessage::user("OVERFLOW")]))
        .await
    {
        Err(LlmError::ContextOverflow {
            need: 9000,
            have: 8192,
        }) => {}
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn connection_refused_is_transport_error() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    assert!(matches!(
        client(port, "t").health().await,
        Err(LlmError::Transport(_))
    ));
}
