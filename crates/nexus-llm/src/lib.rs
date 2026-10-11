//! `nexus-llm`: owns the `llama-server` process and talks to it (INFERENCE-CONTRACT.md).

#![cfg_attr(
    test,
    allow(clippy::unwrap_used, clippy::expect_used, clippy::field_reassign_with_default)
)]
pub mod client;
pub mod config;
pub mod error;
pub mod logring;
pub mod port;
pub mod process;
pub mod sse;
pub mod think;
pub mod token;
pub mod types;

pub use client::{EventStream, Health, LlmClient};
pub use config::RuntimeConfig;
pub use error::LlmError;
pub use logring::LogRing;
pub use process::ServerProcess;
pub use sse::{SseDecoder, SseItem};
pub use think::{Segment, ThinkSplitter};
pub use types::*;
