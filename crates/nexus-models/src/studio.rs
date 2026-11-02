//! Shared, content-addressed Studio asset storage. No hard links or host caches.

use nexus_core::{error::NexusError, portable};
use nexus_store::{
    Store,
    studio::{StudioAsset, StudioModel},
};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

/// Validate model metadata before publishing a registry entry.
pub fn register(store: &Store, root: &Path, model: &StudioModel) -> Result<(), NexusError> {
    if !identifier(&model.id)
        || !identifier(&model.variant)
        || model.name.trim().is_empty()
        || model.name.len() > 512
        || model.metadata.len() > 65_536
        || model.assets.is_empty()
        || model.assets.len() > 64
    {
        return Err(invalid("invalid model metadata or asset count"));
    }
    let _: serde_json::Value = serde_json::from_str(&model.metadata).map_err(|e| invalid(&e.to_string()))?;
    let mut roles = HashSet::new();
    for role in &model.assets {
        if !identifier(&role.role) || !roles.insert(&role.role) {
            return Err(invalid("invalid or duplicate asset role"));
        }
        validate_asset(root, &role.asset)?;
    }
    store.studio_upsert(model)?;
    Ok(())
}

/// Resolve a registered asset, enforcing portable containment and the recorded size.
pub fn validate_asset(root: &Path, asset: &StudioAsset) -> Result<PathBuf, NexusError> {
    if !valid_digest(&asset.sha256)
        || asset.bytes == 0
        || asset.bytes > i64::MAX as u64
        || !asset.path.starts_with("models/")
        || asset.path.contains('\\')
        || asset
            .path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(invalid("invalid shared asset reference"));
    }
    let path = portable::contained(root, Path::new(&asset.path))?;
    let meta = std::fs::metadata(&path)?;
    if !meta.is_file() || meta.len() != asset.bytes {
        return Err(invalid("shared asset is missing or has changed size"));
    }
    Ok(path)
}

/// Import one file once. Duplicate content reuses its existing bytes, regardless of model kind.
/// Source is always retained; staged copy is hashed before publication and removed on failure.
pub fn import_asset(
    store: &Store,
    root: &Path,
    source: &Path,
    cancel: &CancellationToken,
    progress: impl Fn(u64),
) -> Result<StudioAsset, NexusError> {
    static IMPORT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = IMPORT_LOCK
        .lock()
        .map_err(|_| NexusError::Internal("asset import lock poisoned".into()))?;
    if cancel.is_cancelled() {
        return Err(NexusError::Cancelled);
    }
    let source = source.canonicalize()?;
    let meta = std::fs::metadata(&source)?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > i64::MAX as u64 {
        return Err(invalid("choose a nonempty model asset file"));
    }
    let sha = crate::bundled::hash_file(&source, cancel, |_| {}).map_err(|e| {
        if cancel.is_cancelled() {
            NexusError::Cancelled
        } else {
            invalid(&e.to_string())
        }
    })?;
    if let Some(existing) = store.studio_asset(&sha)? {
        let path = validate_asset(root, &existing)?;
        let actual = crate::bundled::hash_file(&path, cancel, |_| {}).map_err(|e| invalid(&e.to_string()))?;
        if actual != sha {
            return Err(invalid(
                "existing shared asset failed integrity verification; repair it before importing",
            ));
        }
        progress(existing.bytes);
        return Ok(existing);
    }
    let extension = source
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("bin")
        .to_ascii_lowercase();
    if !identifier(&extension) || extension.len() > 16 {
        return Err(invalid("unsupported asset filename extension"));
    }
    let relative = format!("models/blobs/{sha}/asset.{extension}");
    let destination = portable::contained(root, Path::new(&relative))?;
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("asset has no parent"))?;
    portable::ensure_directory(
        root,
        parent.strip_prefix(root).map_err(|e| invalid(&e.to_string()))?,
    )?;
    // A prior interrupted publication may already have left the verified file in place.
    if destination.exists() {
        let actual =
            crate::bundled::hash_file(&destination, cancel, |_| {}).map_err(|e| invalid(&e.to_string()))?;
        if actual != sha {
            return Err(invalid("unregistered asset path contains different bytes"));
        }
        let asset = StudioAsset {
            sha256: sha,
            path: relative,
            bytes: meta.len(),
        };
        store.studio_publish_asset(&asset)?;
        return Ok(asset);
    }
    if let Some(have) = crate::download::available_space(root)
        && have < meta.len().saturating_add(crate::download::DISK_HEADROOM)
    {
        return Err(invalid("not enough free USB space to import this asset"));
    }
    let staging = portable::ensure_directory(root, Path::new("cache/imports"))?
        .join(format!("{}.part", nexus_core::ids::JobId::new()));
    let result = (|| -> Result<(), NexusError> {
        let mut input = std::fs::File::open(&source)?;
        let mut output = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&staging)?;
        let mut hash = Sha256::new();
        let mut bytes = 0_u64;
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            if cancel.is_cancelled() {
                return Err(NexusError::Cancelled);
            }
            let n = input.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            output.write_all(&buffer[..n])?;
            hash.update(&buffer[..n]);
            bytes += n as u64;
            progress(bytes);
        }
        if bytes != meta.len() || format!("{:x}", hash.finalize()) != sha {
            return Err(invalid("source changed during import"));
        }
        output.sync_all()?;
        drop(output);
        if cancel.is_cancelled() {
            return Err(NexusError::Cancelled);
        }
        std::fs::rename(&staging, &destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    result?;
    let asset = StudioAsset {
        sha256: sha,
        path: relative,
        bytes: meta.len(),
    };
    store.studio_publish_asset(&asset)?;
    Ok(asset)
}

/// SHA-256 registry keys use one normalized representation.
pub fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}
fn invalid(message: &str) -> NexusError {
    NexusError::InvalidInput(message.into())
}
