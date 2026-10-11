//! SQLite store (DATA-AND-IPC §1): one connection behind a mutex, WAL, ordered migrations tracked
//! with `PRAGMA user_version`. Timestamps are unix milliseconds (UTC); ids are UUIDv7 text.
//!
//! **Never edit an applied migration; add a new one.**

mod migrate;
pub mod notifications;
pub mod settings;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use nexus_core::error::NexusError;
use rusqlite::Connection;

pub use migrate::{MIGRATIONS, schema_version};
pub use notifications::NewNotification;

/// Errors from the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid stored value for `{key}`: {reason}")]
    Invalid { key: String, reason: String },
    #[error("database was created by a newer NanoNexus (schema {found}, this build knows {known})")]
    TooNew { found: u32, known: u32 },
    #[error("database lock poisoned")]
    Poisoned,
}

impl From<StoreError> for NexusError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Invalid { .. } => NexusError::InvalidInput(e.to_string()),
            other => NexusError::Internal(other.to_string()),
        }
    }
}

/// The application database.
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Open (creating the file and parent folders), set pragmas, and run pending migrations.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::init(Connection::open(path)?)
    }

    /// A throwaway in-memory database (tests).
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
        )?;
        migrate::run(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Run `f` with the connection locked.
    pub fn with_conn<T>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut guard: MutexGuard<'_, Connection> = self.conn.lock().map_err(|_| StoreError::Poisoned)?;
        f(&mut guard)
    }
}
