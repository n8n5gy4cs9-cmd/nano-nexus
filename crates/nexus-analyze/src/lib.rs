//! `nexus-analyze`: deterministic analysis. Rust computes the facts; the LLM only writes the story
//! (ANALYZER.md).

pub mod bm25;
pub mod categories;
pub mod chunk;
pub mod code;
pub mod dups;
pub mod extract;
pub mod facts;
pub mod file_report;
pub mod fmt;
pub mod junk;
pub mod logs;
pub mod naming;
pub mod profile;
pub mod render;
pub mod report;
pub mod scan;
pub mod secrets;

pub use categories::Category;
pub use dups::DupMode;
pub use report::{FileEntry, FolderReport};
pub use scan::{ScanError, ScanOptions, ScanProgress, scan_folder};
