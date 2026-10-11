//! `nexus-knowledge`: offline reference libraries (M17). Extract -> heading-aware chunks -> SQLite
//! FTS5 (BM25) -> cited passages. No LLM calls and no network; no Tauri types. The job wrapper,
//! commands, and tool live in the app.
//!
//! Library text is DATA: [`prompt`] wraps every passage in `<doc>` tags and tells the model to cite
//! `[n]` and to answer "not in the library" when nothing relevant was found.

pub mod chunk;
pub mod extract;
pub mod index;
pub mod prompt;
pub mod repo;
pub mod search;
pub mod types;

pub use chunk::{ChunkOpts, ChunkOut, chunk_document};
pub use extract::{KbDoc, KbPage, extract_file, is_supported};
pub use index::{IndexOpts, IndexProgress, IndexSummary, index_library};
pub use prompt::{CitationReport, NOT_IN_LIBRARY, build_context, verify_citations};
pub use search::{SearchOpts, search};
pub use types::{DocumentRow, KbError, Library, LibrarySource, Passage};
