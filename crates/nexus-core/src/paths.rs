//! Resolved on-disk locations. The app resolves the roots (Tauri path API); this module only
//! derives the subdirectories so every crate agrees on the layout (OVERVIEW §8).

use std::path::{Path, PathBuf};

/// File name that, placed next to the executable, switches the app to portable mode.
pub const PORTABLE_FLAG: &str = "portable.flag";

/// Versioned marker at the root of a shared USB distribution.
pub const PORTABLE_MANIFEST: &str = "portable.json";

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PortableManifest {
    schema: u32,
}

/// Resolve the shared USB root from the exact platform installation layout.
///
/// Only fixed-depth layouts are recognized; arbitrary ancestor markers are never used.
/// A present but invalid marker is an error, preventing fallback to host storage.
pub fn distribution_root(executable: &Path) -> std::io::Result<Option<PathBuf>> {
    let Some(exe_dir) = executable.parent() else {
        return Ok(None);
    };
    let candidate = if exe_dir.file_name().is_some_and(|n| n == "windows-x64")
        || exe_dir.file_name().is_some_and(|n| n == "windows-arm64")
    {
        exe_dir
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == "apps"))
            .and_then(Path::parent)
    } else if exe_dir.file_name().is_some_and(|n| n == "MacOS") {
        exe_dir
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == "Contents"))
            .and_then(Path::parent)
            .filter(|p| p.file_name().is_some_and(|n| n == "NanoNexus.app"))
            .and_then(Path::parent)
            .filter(|p| p.file_name().is_some_and(|n| n == "macos-arm64"))
            .and_then(Path::parent)
            .filter(|p| p.file_name().is_some_and(|n| n == "apps"))
            .and_then(Path::parent)
    } else {
        None
    };
    let Some(root) = candidate else {
        return Ok(None);
    };
    // A recognized distribution layout must never silently become an installed app.
    let marker_path = crate::portable::contained(root, Path::new(PORTABLE_MANIFEST))?;
    let file = std::fs::File::open(marker_path)?;
    if file.metadata()?.len() > 4096 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "portable marker exceeds 4096 bytes",
        ));
    }
    let marker: PortableManifest =
        serde_json::from_reader(file).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if marker.schema != 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unsupported portable schema",
        ));
    }
    root.canonicalize().map(Some)
}

/// All directories NanoNexus owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    /// Shared USB root, absent for legacy and installed layouts.
    pub portable_root: Option<PathBuf>,
    pub data: PathBuf,
    pub models: PathBuf,
    pub journal: PathBuf,
    pub reports: PathBuf,
    pub runtime: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
    /// Persistent generated media and transcripts.
    pub outputs: PathBuf,
    /// Application-owned recoverable deletions.
    pub trash: PathBuf,
}

impl AppPaths {
    /// Shared distribution paths. Models are external to both platform applications.
    pub fn distribution(root: &Path) -> Self {
        let mut paths = Self::from_roots(root.join("data"), root.join("logs").join(std::env::consts::OS));
        paths.portable_root = Some(root.to_owned());
        paths.models = root.join("models");
        paths.cache = root.join("cache").join(std::env::consts::OS);
        paths.runtime = paths.data.join("runtime").join(std::env::consts::OS);
        paths.outputs = root.join("outputs");
        paths
    }

    /// Derive every directory from the data root and the log root.
    pub fn from_roots(data_dir: impl Into<PathBuf>, log_dir: impl Into<PathBuf>) -> Self {
        let data = data_dir.into();
        Self {
            portable_root: None,
            models: data.join("models"),
            journal: data.join("journal"),
            reports: data.join("reports"),
            runtime: data.join("runtime"),
            cache: data.join("cache"),
            logs: log_dir.into(),
            outputs: data.join("outputs"),
            trash: data.join("trash"),
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
            &self.outputs,
            &self.trash,
        ] {
            if let Some(root) = &self.portable_root {
                let relative = dir.strip_prefix(root).map_err(std::io::Error::other)?;
                let checked = crate::portable::ensure_directory(root, relative)?;
                crate::portable::require_writable(&checked)?;
            } else {
                std::fs::create_dir_all(dir)?;
            }
        }
        if let Some(root) = &self.portable_root {
            // SQLite opens sidecars itself; validate all existing destinations before opening it.
            for name in ["nexus.db", "nexus.db-wal", "nexus.db-shm", "nexus.db-journal"] {
                crate::portable::contained(root, &Path::new("data").join(name))?;
            }
            for name in ["knowledge", "attachments", "backups"] {
                crate::portable::ensure_directory(root, &Path::new("data").join(name))?;
            }
        }
        Ok(())
    }

    /// Revalidate a destination before an application-owned write.
    pub fn owned_path(&self, path: &Path) -> std::io::Result<PathBuf> {
        match &self.portable_root {
            Some(root) => {
                let relative = path.strip_prefix(root).map_err(std::io::Error::other)?;
                crate::portable::contained(root, relative)
            }
            None => Ok(path.to_owned()),
        }
    }

    /// Persist an application-owned path independently of this USB's mount name.
    /// Installed-mode paths retain their absolute representation.
    pub fn storage_reference(&self, path: &Path) -> std::io::Result<String> {
        let checked = self.owned_path(path)?;
        let Some(root) = &self.portable_root else {
            return Ok(checked.to_string_lossy().into_owned());
        };
        let relative = checked.strip_prefix(root).map_err(std::io::Error::other)?;
        let parts = relative
            .components()
            .map(|part| {
                part.as_os_str().to_str().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "stored portable path must be UTF-8",
                    )
                })
            })
            .collect::<std::io::Result<Vec<_>>>()?;
        if parts.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cannot store the distribution root as a file",
            ));
        }
        Ok(format!("dist:{}", parts.join("/")))
    }

    /// Resolve a stored reference strictly below its owning subsystem directory.
    /// Old absolute paths are accepted only when still canonically inside the current owner.
    pub fn resolve_storage_reference(&self, value: &str, owner: &Path) -> std::io::Result<PathBuf> {
        let invalid = || {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid or unavailable owned storage reference",
            )
        };
        let candidate = if let Some(relative) = value.strip_prefix("dist:") {
            let root = self.portable_root.as_ref().ok_or_else(invalid)?;
            if relative.is_empty()
                || relative.contains('\\')
                || relative
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
            {
                return Err(invalid());
            }
            crate::portable::contained(root, Path::new(relative))?
        } else {
            let path = Path::new(value);
            if !path.is_absolute() {
                return Err(invalid());
            }
            path.to_path_buf()
        };
        let canonical_owner = owner.canonicalize()?;
        let candidate = match candidate.strip_prefix(owner) {
            Ok(relative) if !relative.as_os_str().is_empty() => {
                crate::portable::contained(&canonical_owner, relative)?
            }
            _ => candidate.canonicalize()?,
        };
        if candidate == canonical_owner || !candidate.starts_with(&canonical_owner) {
            return Err(invalid());
        }
        self.owned_path(&candidate)
    }

    /// Explicit private worker environment; never mutate process-global HOME.
    pub fn worker_environment(&self) -> std::io::Result<std::collections::BTreeMap<String, PathBuf>> {
        let mut env = std::collections::BTreeMap::new();
        if let Some(root) = &self.portable_root {
            for (name, suffix) in [
                ("HOME", "home"),
                ("USERPROFILE", "home"),
                ("TMPDIR", "temp"),
                ("TMP", "temp"),
                ("TEMP", "temp"),
                ("XDG_CACHE_HOME", "workers"),
                ("XDG_CONFIG_HOME", "config"),
                ("XDG_DATA_HOME", "home/data"),
                ("APPDATA", "config"),
                ("LOCALAPPDATA", "workers"),
                ("HF_HOME", "workers/huggingface"),
                ("HUGGINGFACE_HUB_CACHE", "workers/huggingface/hub"),
                ("TORCH_HOME", "workers/torch"),
                ("CUDA_CACHE_PATH", "workers/cuda"),
                ("MESA_SHADER_CACHE_DIR", "workers/mesa"),
                ("ORT_CACHE_DIR", "workers/onnx"),
                ("npm_config_cache", "workers/npm"),
                ("PIP_CACHE_DIR", "workers/pip"),
                ("CLAUDE_CONFIG_DIR", "config/claude"),
            ] {
                let path = self.cache.join(suffix);
                let relative = path.strip_prefix(root).map_err(std::io::Error::other)?;
                env.insert(name.into(), crate::portable::ensure_directory(root, relative)?);
            }
        }
        Ok(env)
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
