//! Saved outputs (images, audio clips, transcripts) with a metadata sidecar, ported from the reference's
//! `saveGeneratedOutput` / `listGeneratedOutputs`.
//!
//! This module only **writes new files** into the directory it is given (the USB `outputs/` area) and
//! **never deletes**: [`removal_paths`] returns what would be removed so the caller can send it to the
//! application Trash or through the Journal (UP.042). Names from callers are reduced to a base name and
//! checked to live directly inside the directory, so nothing here can reach outside it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::StudioError;

/// Reduce a user-influenced fragment to `[A-Za-z0-9._-]`, at most 80 characters.
pub fn safe_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(['.', '-']);
    trimmed.chars().take(80).collect()
}

/// A saved output as listed to the UI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct OutputRecord {
    /// Data file name inside the outputs directory.
    pub file: String,
    /// Sidecar file name.
    pub metadata: String,
    pub created_at: String,
    /// Remaining sidecar fields (prompt, seed, voice, text, ...), as written.
    #[specta(type = specta_typescript::Unknown)]
    pub details: Value,
}

fn timestamp() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

/// Write `bytes` and a sidecar `details` object. The stem is `{prefix}-{timestamp}[-{suffix}]`.
pub fn save(
    dir: &Path,
    prefix: &str,
    suffix: Option<&str>,
    ext: &str,
    bytes: &[u8],
    details: Value,
) -> Result<OutputRecord, StudioError> {
    std::fs::create_dir_all(dir)?;
    let created_at = timestamp();
    let stamp = created_at.replace([':', '.'], "-");
    let mut stem = format!("{}-{stamp}", safe_name(prefix));
    if let Some(s) = suffix.map(safe_name).filter(|s| !s.is_empty()) {
        stem.push('-');
        stem.push_str(&s);
    }
    let ext = safe_name(ext);
    if ext.is_empty() {
        return Err(StudioError::Invalid("The output type is not valid.".into()));
    }
    let file = format!("{stem}.{ext}");
    let metadata = format!("{stem}.json");
    let mut sidecar = match details {
        Value::Object(m) => m,
        other => {
            let mut m = Map::new();
            m.insert("value".into(), other);
            m
        }
    };
    sidecar.insert("createdAt".into(), Value::String(created_at.clone()));
    sidecar.insert("file".into(), Value::String(file.clone()));
    sidecar.insert("metadata".into(), Value::String(metadata.clone()));
    let details = Value::Object(sidecar);
    let json = serde_json::to_vec_pretty(&details).map_err(|e| StudioError::Io(e.to_string()))?;
    // Data first, sidecar last: a listed output always has its file.
    write_new(&dir.join(&file), bytes)?;
    write_new(&dir.join(&metadata), &json)?;
    Ok(OutputRecord {
        file,
        metadata,
        created_at,
        details,
    })
}

/// Create a file that must not exist yet (two outputs in the same millisecond never overwrite).
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), StudioError> {
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

/// All outputs in `dir`, newest first. Unreadable or incomplete sidecars are skipped.
pub fn list(dir: &Path) -> Vec<OutputRecord> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<OutputRecord> = rd
        .filter_map(Result::ok)
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.ends_with(".json") && !n.starts_with('.')
        })
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            let details: Value = serde_json::from_str(&text).ok()?;
            let file = details.get("file")?.as_str()?.to_owned();
            if !dir.join(&file).is_file() {
                return None;
            }
            Some(OutputRecord {
                metadata: e.file_name().to_string_lossy().into_owned(),
                created_at: details
                    .get("createdAt")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                file,
                details,
            })
        })
        .collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    out
}

/// Resolve a client-supplied name to a file directly inside `dir`.
pub fn resolve(dir: &Path, name: &str) -> Result<PathBuf, StudioError> {
    let base = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| *n == name && !n.starts_with('.'))
        .ok_or_else(|| StudioError::Invalid("That file name is not valid.".into()))?;
    let p = dir.join(base);
    if p.is_file() {
        Ok(p)
    } else {
        Err(StudioError::Invalid("That output no longer exists.".into()))
    }
}

/// Files (data + sidecar) that deleting `names` would remove. Nothing is deleted here.
pub fn removal_paths(dir: &Path, names: &[String]) -> Result<Vec<PathBuf>, StudioError> {
    let mut out = Vec::new();
    for name in names {
        let data = resolve(dir, name)?;
        let stem = data.with_extension("");
        let side = stem.with_extension("json");
        if side != data && side.is_file() {
            out.push(side);
        }
        out.push(data);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names_are_reduced_and_confined() {
        assert_eq!(safe_name("../a b/c.png"), "a-b-c.png");
        assert_eq!(safe_name("..."), "");
        let d = tempfile::tempdir().unwrap();
        assert!(resolve(d.path(), "../etc").is_err());
        assert!(resolve(d.path(), "sub/x").is_err());
    }

    #[test]
    fn save_list_and_removal_plan_round_trip() {
        let d = tempfile::tempdir().unwrap();
        let r = save(
            d.path(),
            "output",
            Some("42"),
            "png",
            b"png",
            json!({"prompt": "p", "seed": 42}),
        )
        .unwrap();
        assert!(r.file.ends_with("-42.png"));
        let l = list(d.path());
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].details["prompt"], "p");
        let plan = removal_paths(d.path(), std::slice::from_ref(&r.file)).unwrap();
        assert_eq!(plan.len(), 2);
        assert!(d.path().join(&r.file).exists(), "planning must not delete");
    }
}
