//! Get a schema-constrained, validated struct from the local model.
//!
//! `cargo run -p nexus-llm --example json`

mod common;

use nexus_llm::{
    ChatMessage, Priority, Scheduler, Structured, StructuredSpec, Validate, ValidationCtx, chat_json,
};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize)]
struct Title {
    title: String,
}

impl Validate for Title {
    fn validate(&self, _ctx: &ValidationCtx) -> Result<(), Vec<String>> {
        let n = self.title.chars().count();
        if (3..=48).contains(&n) {
            Ok(())
        } else {
            Err(vec![format!("title must be 3-48 characters, got {n}")])
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../schemas/llm/title.schema.json"))?;
    let sup = common::start().await?;
    let sched = Scheduler::new(sup.clone());

    let spec = StructuredSpec {
        name: "title",
        schema: &schema,
        temperature: 0.3,
        max_tokens: 64,
    };
    let msgs = vec![
        ChatMessage::system("Reply with a short conversation title."),
        ChatMessage::user("How do I keep a sourdough starter alive while I'm on holiday?"),
    ];
    let out: Structured<Title> = chat_json(
        &sched,
        Priority::Interactive,
        msgs,
        spec,
        &ValidationCtx::default(),
        CancellationToken::new(),
    )
    .await?;
    match out {
        Structured::Valid(t) => println!("{t:#?}"),
        Structured::Fallback { reason } => println!("fell back: {reason}"),
    }
    sup.shutdown().await;
    Ok(())
}
