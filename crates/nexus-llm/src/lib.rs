//! `nexus-llm`: owns the `llama-server` process and talks to it (INFERENCE-CONTRACT.md).

pub mod client;
pub mod error;
pub mod sse;
pub mod think;
pub mod types;

pub use client::{EventStream, Health, LlmClient};
pub use error::LlmError;
pub use sse::{SseDecoder, SseItem};
pub use think::{Segment, ThinkSplitter};
pub use types::*;
