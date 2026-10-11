#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support;

use std::time::Duration;

use nexus_llm::{
    ChatMessage, LlmClient, LlmError, Priority, Scheduler, Structured, StructuredSpec, Validate,
    ValidationCtx, chat_json,
};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{Value, json};
use support::mock_server::{MockConfig, Shared, spawn};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize, PartialEq)]
struct Title {
    title: String,
}

impl Validate for Title {
    fn validate(&self, ctx: &ValidationCtx) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        if self.title.len() < 3 {
            errs.push("title is too short".to_owned());
        }
        if ctx.reserved_names.contains(&self.title) {
            errs.push(format!("`{}` is reserved", self.title));
        }
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }
}

fn schema() -> Value {
    json!({"$comment": "doc only", "type": "object", "additionalProperties": false, "required": ["title"],
        "properties": {"title": {"type": "string", "minLength": 3, "maxLength": 48}}})
}

async fn run(
    replies: &[&str],
    ctx: ValidationCtx,
    cancel: CancellationToken,
    user: &str,
) -> (Result<Structured<Title>, LlmError>, std::sync::Arc<Shared>) {
    let mut cfg = MockConfig::new("t");
    cfg.replies = replies.iter().map(|s| (*s).to_owned()).collect();
    let (port, shared) = spawn(cfg).await;
    let sched = Scheduler::with_client(LlmClient::new(port, SecretString::from("t".to_owned())).unwrap());
    let schema = schema();
    let spec = StructuredSpec {
        name: "title",
        schema: &schema,
        temperature: 0.4,
        max_tokens: 64,
    };
    let r = chat_json::<Title>(
        &sched,
        Priority::Background,
        vec![ChatMessage::user(user)],
        spec,
        &ctx,
        cancel,
    )
    .await;
    (r, shared)
}

#[tokio::test]
async fn valid_first_try() {
    let (r, shared) = run(
        &[r#"{"title":"Sourdough tips"}"#],
        ValidationCtx::default(),
        CancellationToken::new(),
        "x",
    )
    .await;
    assert_eq!(
        r.unwrap(),
        Structured::Valid(Title {
            title: "Sourdough tips".into()
        })
    );
    let reqs = shared.chat_requests();
    assert_eq!(reqs.len(), 1);
    let rf = &reqs[0]["response_format"];
    assert_eq!(rf["type"], "json_schema");
    assert_eq!(rf["json_schema"]["name"], "title");
    assert!(
        rf["json_schema"]["schema"].get("$comment").is_none(),
        "doc keys must be stripped"
    );
    assert_eq!(
        reqs[0]["temperature"].as_f64().map(|t| (t * 10.0).round()),
        Some(4.0)
    );
}

#[tokio::test]
async fn invalid_then_valid_reasks_once_with_the_problems_and_lower_temperature() {
    let (r, shared) = run(
        &[r#"{"title":"ab"}"#, r#"{"title":"Better title"}"#],
        ValidationCtx::default(),
        CancellationToken::new(),
        "x",
    )
    .await;
    assert_eq!(
        r.unwrap(),
        Structured::Valid(Title {
            title: "Better title".into()
        })
    );
    let reqs = shared.chat_requests();
    assert_eq!(reqs.len(), 2);
    let msgs = reqs[1]["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[1]["role"], "assistant");
    assert_eq!(msgs[1]["content"], r#"{"title":"ab"}"#);
    let fix = msgs[2]["content"].as_str().unwrap();
    assert!(
        fix.contains("title is too short") && fix.contains("Return corrected JSON only"),
        "{fix}"
    );
    assert!(reqs[1]["temperature"].as_f64().unwrap() <= 0.21);
}

#[tokio::test]
async fn malformed_json_counts_as_invalid() {
    let (r, shared) = run(
        &["not json at all", r#"{"title":"Fine now"}"#],
        ValidationCtx::default(),
        CancellationToken::new(),
        "x",
    )
    .await;
    assert!(matches!(r.unwrap(), Structured::Valid(_)));
    assert_eq!(shared.chat_requests().len(), 2);
}

#[tokio::test]
async fn invalid_twice_falls_back_without_more_requests() {
    let (r, shared) = run(
        &[r#"{"title":"no"}"#],
        ValidationCtx::default(),
        CancellationToken::new(),
        "x",
    )
    .await;
    match r.unwrap() {
        Structured::Fallback { reason } => assert!(reason.contains("too short"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(shared.chat_requests().len(), 2);
}

#[tokio::test]
async fn context_is_passed_to_the_validator() {
    let ctx = ValidationCtx {
        reserved_names: vec!["Taken".into()],
        ..Default::default()
    };
    let (r, _) = run(
        &[r#"{"title":"Taken"}"#, r#"{"title":"Free name"}"#],
        ctx,
        CancellationToken::new(),
        "x",
    )
    .await;
    assert_eq!(
        r.unwrap(),
        Structured::Valid(Title {
            title: "Free name".into()
        })
    );
}

#[tokio::test]
async fn cancel_midway_returns_cancelled() {
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        c.cancel();
    });
    let (r, _) = run(
        &[r#"{"title":"Slow answer"}"#],
        ValidationCtx::default(),
        cancel,
        "SLOW please",
    )
    .await;
    assert!(matches!(r, Err(LlmError::Cancelled)), "{r:?}");
}

#[tokio::test]
async fn transport_errors_propagate() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let sched = Scheduler::with_client(LlmClient::new(port, SecretString::from("t".to_owned())).unwrap());
    let schema = schema();
    let spec = StructuredSpec {
        name: "title",
        schema: &schema,
        temperature: 0.3,
        max_tokens: 32,
    };
    let r = chat_json::<Title>(
        &sched,
        Priority::Interactive,
        vec![ChatMessage::user("x")],
        spec,
        &ValidationCtx::default(),
        CancellationToken::new(),
    )
    .await;
    assert!(matches!(r, Err(LlmError::Transport(_))), "{r:?}");
}
