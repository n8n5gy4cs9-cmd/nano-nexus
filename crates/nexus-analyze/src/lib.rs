//! `nexus-analyze`: deterministic analysis. Rust computes the facts; the LLM only writes the story
//! (ANALYZER.md).

pub mod categories;
pub mod dups;
pub mod facts;
pub mod fmt;
pub mod junk;
pub mod naming;
pub mod render;
pub mod report;
pub mod scan;

pub use categories::Category;
pub use dups::DupMode;
pub use report::{FileEntry, FolderReport};
pub use scan::{ScanError, ScanOptions, ScanProgress, scan_folder};
