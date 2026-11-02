//! Mount-independent internal library references, distinct from host-bound source folders.

use std::path::{Path, PathBuf};

use nexus_core::{error::NexusError, portable};
use nexus_knowledge::{Library, LibrarySource, repo};

use crate::state::AppCtx;

const PREFIX: &str = "dist:";

/// Encode only a directory already contained in this distribution; never guess old mounts.
pub(super) fn stored(ctx: &AppCtx, path: &Path) -> Result<String, NexusError> {
    let path = path.canonicalize()?;
    let Some(root) = &ctx.paths.portable_root else {
        return Ok(path.to_string_lossy().into_owned());
    };
    let relative = path
        .strip_prefix(root)
        .map_err(|_| NexusError::InvalidInput("library is outside the USB distribution".into()))?;
    let checked = portable::contained(root, relative)?;
    if !checked.is_dir() {
        return Err(NexusError::InvalidInput("library folder is unavailable".into()));
    }
    let relative = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    Ok(format!("{PREFIX}{relative}"))
}

/// Internal service reads may access their own data; external references always need a live grant.
pub(super) fn resolve(ctx: &AppCtx, library: &Library) -> Result<PathBuf, NexusError> {
    let internal = library.mode == "copy" || library.source == LibrarySource::Bundled;
    let path = if let Some(relative) = library.path.strip_prefix(PREFIX) {
        let root = ctx.paths.portable_root.as_ref().ok_or_else(|| {
            NexusError::InvalidInput("USB library requires its complete DIST folder".into())
        })?;
        if !internal
            || relative.is_empty()
            || relative.contains('\\')
            || relative.split('/').any(|c| c.is_empty() || c == "." || c == "..")
        {
            return Err(NexusError::InvalidInput(
                "invalid portable library reference".into(),
            ));
        }
        portable::contained(root, Path::new(relative))?
    } else {
        let path = Path::new(&library.path);
        if !path.is_absolute() {
            return Err(NexusError::InvalidInput(
                "library path is not absolute; choose the source again".into(),
            ));
        }
        path.canonicalize()?
    };
    let path = path.canonicalize()?;
    if !path.is_dir() {
        return Err(NexusError::InvalidInput("library folder is unavailable".into()));
    }
    if internal {
        let owner = if library.mode == "copy" {
            ctx.paths.data.join("knowledge")
        } else {
            ctx.resource_dir.join("knowledge")
        }
        .canonicalize()?;
        if path == owner || !path.starts_with(&owner) {
            return Err(NexusError::PathDenied(
                "library escapes its owned directory".into(),
            ));
        }
        ctx.paths.owned_path(&path)?;
        if ctx.paths.portable_root.is_some() && !library.path.starts_with(PREFIX) {
            let reference = stored(ctx, &path)?;
            repo::set_path(&ctx.store, &library.id, &library.path, &reference).map_err(NexusError::from)?;
        }
    } else {
        ctx.perms.check("user", &path, nexus_core::fs::Access::Read)?;
    }
    Ok(path)
}

/// Enforce per-file containment and external deny/grant rules during indexing and source reads.
pub(super) fn authorize_file(
    ctx: &AppCtx,
    library: &Library,
    root: &Path,
    file: &Path,
) -> Result<(), NexusError> {
    let file = file.canonicalize()?;
    if !file.starts_with(root) || file == root {
        return Err(NexusError::PathDenied("document escapes its library".into()));
    }
    if library.mode == "copy" || library.source == LibrarySource::Bundled {
        ctx.paths.owned_path(&file)?;
    } else {
        ctx.perms.check("user", &file, nexus_core::fs::Access::Read)?;
    }
    Ok(())
}
