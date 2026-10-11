//! Resolved on-disk locations. The app resolves the roots (Tauri path API); this module only
//! derives the subdirectories so every crate agrees on the layout (OVERVIEW §8).

use std::path::{Path, PathBuf};

/// File name that, placed next to the executable, switches the app to portable mode.
pub const PORTABLE_FLAG: &str = "portable.flag";

/// All directories NanoNexus owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    pub data: PathBuf,
    pub models: PathBuf,
    pub journal: PathBuf,
    pub reports: PathBuf,
    pub runtime: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
}

impl AppPaths {
    /// Derive every directory from the data root and the log root.
    pub fn from_roots(data_dir: impl Into<PathBuf>, log_dir: impl Into<PathBuf>) -> Self {
        let data = data_dir.into();
        Self {
            models: data.join("models"),
            journal: data.join("journal"),
            reports: data.join("reports"),
            runtime: data.join("runtime"),
            cache: data.join("cache"),
            logs: log_dir.into(),
            data,
        }
    }

    /// Portable layout: everything under `<exe_dir>/data` (logs in `data/logs`).
    pub fn portable(exe_dir: &Path) -> Self {
        let data = exe_dir.join("data");
        let logs = data.join("logs");
        Self::from_roots(data, logs)
    }

    /// True if `portable.flag` exists in `exe_dir`.
    pub fn is_portable(exe_dir: &Path) -> bool {
        exe_dir.join(PORTABLE_FLAG).is_file()
    }

    /// Create every directory (idempotent).
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [
            &self.data,
            &self.models,
            &self.journal,
            &self.reports,
            &self.runtime,
            &self.cache,
            &self.logs,
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    /// Path of the SQLite database.
    pub fn db_file(&self) -> PathBuf {
        self.data.join("nexus.db")
    }

    /// Path of the llama-server state file used for stale-process reaping.
    pub fn server_json(&self) -> PathBuf {
        self.runtime.join("server.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_dirs_creates_tree_and_is_idempotent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = AppPaths::from_roots(tmp.path().join("data"), tmp.path().join("logs"));
        p.ensure_dirs().unwrap();
        p.ensure_dirs().unwrap();
        for d in [
            &p.data, &p.models, &p.journal, &p.reports, &p.runtime, &p.cache, &p.logs,
        ] {
            assert!(d.is_dir(), "{d:?}");
        }
        assert_eq!(p.db_file(), tmp.path().join("data/nexus.db"));
        assert_eq!(p.server_json(), tmp.path().join("data/runtime/server.json"));
    }

    #[test]
    fn portable_mode_uses_flag_and_local_data() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(!AppPaths::is_portable(tmp.path()));
        std::fs::write(tmp.path().join(PORTABLE_FLAG), "").unwrap();
        assert!(AppPaths::is_portable(tmp.path()));
        let p = AppPaths::portable(tmp.path());
        assert_eq!(p.data, tmp.path().join("data"));
        assert_eq!(p.logs, tmp.path().join("data/logs"));
        p.ensure_dirs().unwrap();
        assert!(p.logs.is_dir());
    }
}
