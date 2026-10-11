//! Shared types and the crate error.

use nexus_core::error::NexusError;
use nexus_store::StoreError;
use serde::{Deserialize, Serialize};

/// Errors from the knowledge crate.
#[derive(Debug, thiserror::Error)]
pub enum KbError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Extract(String),
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("indexing was cancelled")]
    Cancelled,
}

impl From<KbError> for NexusError {
    fn from(e: KbError) -> Self {
        match e {
            KbError::NotFound(w) => NexusError::NotFound(w.to_owned()),
            KbError::Invalid(m) => NexusError::InvalidInput(m),
            KbError::Cancelled => NexusError::Cancelled,
            other => NexusError::Internal(other.to_string()),
        }
    }
}

/// Where a library came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySource {
    /// Shipped inside the app resources (read-only).
    Bundled,
    /// Added by the user.
    User,
}

impl LibrarySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bundled => "bundled",
            Self::User => "user",
        }
    }
    pub fn parse(s: &str) -> Self {
        if s == "bundled" { Self::Bundled } else { Self::User }
    }
}

/// A named folder of documents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Library {
    pub id: String,
    pub name: String,
    pub source: LibrarySource,
    pub path: String,
    /// `copy` (files live in the app data dir) or `reference` (read in place).
    pub mode: String,
    pub enabled: bool,
    pub doc_count: i64,
    pub chunk_count: i64,
    pub bytes: i64,
    pub indexed_at: Option<i64>,
    pub created_at: i64,
}

/// One indexed (or skipped/failed) file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DocumentRow {
    pub id: i64,
    pub library_id: String,
    pub path: String,
    pub title: Option<String>,
    pub hash: String,
    pub kind: String,
    pub pages: i64,
    pub bytes: i64,
    /// `indexed | skipped | failed`
    pub status: String,
    pub note: Option<String>,
}

/// A retrieved chunk with everything needed to cite it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Passage {
    pub chunk_id: i64,
    pub document_id: i64,
    pub library_id: String,
    pub library_name: String,
    /// Document title, or its relative path when it has none.
    pub document: String,
    pub doc_path: String,
    /// Breadcrumb, e.g. `Reaper manual > Routing > Sends`.
    pub heading_path: String,
    pub page: Option<u32>,
    pub text: String,
    pub tokens: u32,
    /// Higher is better (negated BM25 plus boosts).
    pub score: f64,
}

impl Passage {
    /// The `source` shown to the model and the user: `<library> > <breadcrumb>`.
    pub fn source(&self) -> String {
        format!("{} > {}", self.library_name, self.heading_path)
    }
}
