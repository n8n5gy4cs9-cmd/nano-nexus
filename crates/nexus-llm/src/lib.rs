//! `nexus-llm`: owns the `llama-server` process and talks to it (INFERENCE-CONTRACT.md).

pub mod sse;
pub mod think;
pub mod types;

pub use sse::{SseDecoder, SseItem};
pub use think::{Segment, ThinkSplitter};
pub use types::*;
