//! Contained paths and startup ownership for a shared USB distribution.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

/// Keep this guard alive until all services and engines have stopped.
/// The OS releases the exclusive lock on process exit, including a crash.
pub struct DistributionLock {
    _file: File,
}

impl DistributionLock {
    /// Acquire the single writer lock. Unsupported filesystem locking fails closed.
    pub fn acquire(root: &Path) -> io::Result<Self> {
        let path = contained(root, Path::new(".nexus.lock"))?;
        let file = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(path)?;
        file.try_lock().map_err(|e| io::Error::other(format!(
            "Cannot lock the portable folder (another instance may be running, or this drive does not support locking): {e}"
        )))?;
        Ok(Self { _file: file })
    }
}

/// Resolve an owned relative path, rejecting portable-incompatible names and escaping links.
/// Nonexistent suffixes are allowed only below an existing contained directory.
pub fn contained(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let root = root.canonicalize()?;
    if relative.as_os_str().is_empty() {
        return Err(invalid("empty portable path"));
    }
    let mut path = root.clone();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(invalid("portable paths must contain only relative normal components"));
        };
        let name_str = name.to_str().ok_or_else(|| invalid("portable path must be UTF-8"))?;
        if name_str.contains(['\\', ':', '\0']) {
            return Err(invalid("invalid portable path component"));
        }
        path.push(name);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                let canonical = path.canonicalize()?;
                if !canonical.starts_with(&root) {
                    return Err(io::Error::new(io::ErrorKind::PermissionDenied, "portable path escapes the USB folder"));
                }
                path = canonical;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(path)
}

/// Create an owned directory while validating each existing ancestor before mutation.
pub fn ensure_directory(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let path = contained(root, relative)?;
    std::fs::create_dir_all(&path)?;
    let checked = contained(root, relative)?;
    if !checked.is_dir() {
        return Err(invalid("portable directory is not a directory"));
    }
    Ok(checked)
}

/// Confirm writable storage using a unique, tiny USB-local file and remove it immediately.
/// No user data or installed-mode source is migrated as a startup side effect.
pub fn require_writable(directory: &Path) -> io::Result<()> {
    let probe = directory.join(format!(".nexus-write-{}", uuid::Uuid::now_v7()));
    let mut file = OpenOptions::new().create_new(true).write(true).open(&probe)?;
    let result = file.write_all(b"NanoNexus portable storage\n").and_then(|()| file.sync_all());
    drop(file);
    let cleanup = std::fs::remove_file(probe);
    result.and(cleanup).map_err(|e| io::Error::new(e.kind(), format!(
        "The portable folder is not writable; reconnect the USB drive and check its free space: {e}"
    )))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
