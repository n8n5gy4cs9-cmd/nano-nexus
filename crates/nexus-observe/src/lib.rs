//! `nexus-observe`: the Activity Observatory (ADR-017, `docs/architecture/OBSERVE.md`).
//!
//! A passive, read-only, metadata-only view of what is running on this computer: NanoNexus' own
//! work and, when the owner opts in, Claude Code and Codex sessions. External agents are observed by
//! tailing the transcript files they already write; the parsers keep event kinds, model names, token
//! counts, tool names and the path of edit tools, and drop everything else at parse time.

pub mod aggregate;
pub mod claude;
pub mod codex;
pub mod demo;
pub mod model;
pub mod observer;
pub mod render;
pub mod replay;
pub mod selfsrc;
pub mod tail;
pub mod timefmt;

pub use aggregate::{Aggregator, Config};
pub use model::{
    AgentView, ConflictView, Drift, Obs, ObsKind, ProjectView, Role, Snapshot, Source, SourceView, Status,
    TokenCount, Totals,
};
pub use observer::{Attention, AttentionKind, Observer, ObserverConfig};
pub use replay::Replay;
