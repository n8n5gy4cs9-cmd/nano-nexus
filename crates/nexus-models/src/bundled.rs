//! Models that are already on disk: the bundle's `models/manifest.json`, user-added folders, and
//! re-verification (MODEL-MANAGER §1 and §9).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use nexus_store::{ModelRow, Store};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::gguf;

pub const MANIFEST_SCHEMA: &str = include_str!("../../../schemas/bundle-manifest.schema.json");

#[derive(Debug, thiserror::Error)]
pub enum ModelsError {
    #[error("the bundle manifest is not valid: {0}")]
    Manifest(String),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Store(#[from] nexus_store::StoreError),
    #[error("cancelled")]
    Cancelled,
}

/// One model entry of a manifest or model pack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManifestModel {
    pub id: String,
    pub name: String,
    /// Relative to the manifest's folder, forward slashes.
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub revision: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub catalog_id: Option<String>,
    #[serde(default)]
    pub sampling: Option<serde_json::Value>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub profile: String,
    pub built_at: String,
    #[serde(default)]
    pub app_version: Option<String>,
    pub default: String,
    pub models: Vec<ManifestModel>,
}

/// Parse and validate `manifest.json` text.
pub fn parse_manifest(text: &str) -> Result<Manifest, ModelsError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| ModelsError::Manifest(e.to_string()))?;
    let schema: serde_json::Value =
        serde_json::from_str(MANIFEST_SCHEMA).map_err(|e| ModelsError::Manifest(e.to_string()))?;
    let validator = jsonschema::validator_for(&schema).map_err(|e| ModelsError::Manifest(e.to_string()))?;
    let errors: Vec<String> = validator
        .iter_errors(&value)
        .map(|e| format!("{}: {e}", e.instance_path))
        .collect();
    if !errors.is_empty() {
        return Err(ModelsError::Manifest(errors.join("; ")));
    }
    let m: Manifest = serde_json::from_value(value).map_err(|e| ModelsError::Manifest(e.to_string()))?;
    if !m.models.iter().any(|x| x.id == m.default) {
        return Err(ModelsError::Manifest(format!(
            "the default model `{}` is not in the list",
            m.default
        )));
    }
    Ok(m)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Reject manifest paths that climb out of the models folder.
fn safe_join(base: &Path, rel: &str) -> Option<PathBuf> {
    let p = Path::new(rel);
    if p.is_absolute()
        || p.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        })
    {
        return None;
    }
    Some(base.join(p))
}

fn meta_json(path: &Path) -> (Option<String>, Option<gguf::GgufMeta>) {
    match gguf::parse_file(path) {
        Ok(m) => (serde_json::to_string(&m).ok(), Some(m)),
        Err(e) => {
            tracing::warn!(path = %path.display(), "could not read the GGUF header: {e}");
            (None, None)
        }
    }
}

/// What registering the bundle did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BundleReport {
    pub registered: Vec<String>,
    /// Manifest entries whose file is missing on disk.
    pub missing: Vec<String>,
    pub default: Option<String>,
}

/// Register the bundled models of `<models_dir>/manifest.json` (source `builtin`). Hashes are not
/// recomputed here (1+ GB): "Verify bundled models" does that on demand. No manifest = no-op.
pub fn register_bundled(store: &Store, models_dir: &Path) -> Result<BundleReport, ModelsError> {
    let manifest_path = models_dir.join("manifest.json");
    let Ok(text) = std::fs::read_to_string(&manifest_path) else {
        return Ok(BundleReport::default());
    };
    let manifest = parse_manifest(&text)?;
    let mut report = BundleReport {
        default: Some(manifest.default.clone()),
        ..BundleReport::default()
    };
    for m in &manifest.models {
        let Some(path) = safe_join(models_dir, &m.path) else {
            report.missing.push(m.id.clone());
            continue;
        };
        if !path.is_file() {
            report.missing.push(m.id.clone());
            continue;
        }
        let (gguf_meta, meta) = meta_json(&path);
        let extra = serde_json::json!({"sampling": m.sampling, "capabilities": m.capabilities});
        store.upsert_model(&ModelRow {
            id: m.id.clone(),
            source: "builtin".into(),
            name: m.name.clone(),
            repo: m.repo.clone(),
            revision: m.revision.clone(),
            file: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            path: path.display().to_string(),
            sha256: Some(m.sha256.clone()),
            bytes: m.bytes,
            quant: meta.as_ref().and_then(|x| x.quant.clone()),
            catalog_id: m.catalog_id.clone(),
            gguf_meta,
            license: m.license.clone(),
            extra: Some(extra.to_string()),
            added_at: now_ms(),
        })?;
        report.registered.push(m.id.clone());
    }
    Ok(report)
}

/// Which model should run: the saved choice if it still exists, else the bundle's default, else the
/// first installed model.
pub fn resolve_active(
    store: &Store,
    saved: Option<&str>,
    bundle_default: Option<&str>,
) -> Result<Option<String>, ModelsError> {
    let exists = |id: &str| -> Result<bool, ModelsError> {
        Ok(store.get_model(id)?.is_some_and(|m| Path::new(&m.path).is_file()))
    };
    if let Some(id) = saved.filter(|s| !s.is_empty())
        && exists(id)?
    {
        return Ok(Some(id.to_owned()));
    }
    if let Some(id) = bundle_default
        && exists(id)?
    {
        return Ok(Some(id.to_owned()));
    }
    for m in store.list_models()? {
        if Path::new(&m.path).is_file() {
            return Ok(Some(m.id));
        }
    }
    Ok(None)
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() || c == '.' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_owned()
}

/// A model file found in a user-added folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discovered {
    pub id: String,
    pub path: PathBuf,
    pub bytes: u64,
}

/// Stable id for a file found by scanning: `local-<stem>-<8 hex of the path hash>`.
pub fn discovered_id(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let h = Sha256::digest(path.display().to_string().as_bytes());
    let short: String = h.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("local-{}-{short}", slug(&stem))
}

/// Find `*.gguf` files under `dir` (up to 4 levels deep). Split shards other than the first are skipped.
pub fn scan_folder(dir: &Path) -> Vec<Discovered> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_owned(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() && depth < 4 {
                stack.push((p, depth + 1));
            } else if ft.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("gguf")) {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let later_shard = name.contains("-of-") && !name.contains("-00001-of-");
                if later_shard {
                    continue;
                }
                let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
                out.push(Discovered {
                    id: discovered_id(&p),
                    path: p,
                    bytes,
                });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Scan the user's extra folders and make the database match (`discovered` rows). Returns how many
/// models are now registered from them. Files are referenced in place, never copied or deleted.
pub fn sync_extra_folders(store: &Store, dirs: &[PathBuf]) -> Result<usize, ModelsError> {
    let mut keep = Vec::new();
    for dir in dirs {
        for d in scan_folder(dir) {
            let (gguf_meta, meta) = meta_json(&d.path);
            if meta.is_none() {
                continue; // not a readable GGUF
            }
            let name = meta.as_ref().and_then(|m| m.name.clone()).unwrap_or_else(|| {
                d.path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
            let added_at = store.get_model(&d.id)?.map_or_else(now_ms, |m| m.added_at);
            store.upsert_model(&ModelRow {
                id: d.id.clone(),
                source: "discovered".into(),
                name,
                repo: None,
                revision: None,
                file: d.path.file_name().map(|n| n.to_string_lossy().into_owned()),
                path: d.path.display().to_string(),
                sha256: None,
                bytes: d.bytes,
                quant: meta.as_ref().and_then(|x| x.quant.clone()),
                catalog_id: None,
                gguf_meta,
                license: meta.as_ref().and_then(|m| m.license.clone()),
                extra: None,
                added_at,
            })?;
            keep.push(d.id);
        }
    }
    store.prune_discovered(&keep)?;
    Ok(keep.len())
}

/// Result of re-hashing one model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename = "ModelVerifyResult")]
pub struct VerifyResult {
    pub id: String,
    pub ok: bool,
    pub missing: bool,
    pub actual_sha256: Option<String>,
}

/// SHA-256 of a file, reporting bytes read; cancelable.
pub fn hash_file(
    path: &Path,
    cancel: &CancellationToken,
    mut on_bytes: impl FnMut(u64),
) -> Result<String, ModelsError> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(ModelsError::Cancelled);
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        total += n as u64;
        on_bytes(total);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Re-hash every `builtin` model that has a recorded checksum (the "Verify bundled models" button).
/// `on_progress(model_id, done_models, total_models)` fires between models.
pub fn verify_bundled(
    store: &Store,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(&str, usize, usize),
) -> Result<Vec<VerifyResult>, ModelsError> {
    let rows: Vec<ModelRow> = store
        .list_models()?
        .into_iter()
        .filter(|m| m.source == "builtin")
        .collect();
    let total = rows.len();
    let mut out = Vec::new();
    for (i, m) in rows.iter().enumerate() {
        on_progress(&m.id, i, total);
        let path = Path::new(&m.path);
        if !path.is_file() {
            out.push(VerifyResult {
                id: m.id.clone(),
                ok: false,
                missing: true,
                actual_sha256: None,
            });
            continue;
        }
        let actual = hash_file(path, cancel, |_| {})?;
        let ok = m
            .sha256
            .as_deref()
            .is_some_and(|e| e.eq_ignore_ascii_case(&actual));
        out.push(VerifyResult {
            id: m.id.clone(),
            ok,
            missing: false,
            actual_sha256: Some(actual),
        });
    }
    on_progress("", total, total);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gguf::fixture;

    fn write_model(dir: &Path, rel: &str, pad: usize) -> (PathBuf, String, u64) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut data = fixture::sample();
        data.resize(data.len() + pad, 3);
        std::fs::write(&p, &data).unwrap();
        let sha: String = Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect();
        (p, sha, data.len() as u64)
    }

    fn manifest(models: &[(&str, &str, &str, u64)], default: &str) -> String {
        let ms: Vec<_> = models.iter().map(|(id, path, sha, bytes)| serde_json::json!({"id": id, "name": format!("Model {id}"), "path": path, "sha256": sha, "bytes": bytes, "capabilities": ["chat"]})).collect();
        serde_json::json!({"schema": 1, "profile": "studio", "built_at": "2026-10-20T12:00:00Z", "default": default, "models": ms}).to_string()
    }

    #[test]
    fn registers_one_or_many_bundled_models() {
        let dir = tempfile::tempdir().unwrap();
        let (_, sha_a, bytes_a) = write_model(dir.path(), "a/a.gguf", 100);
        let (_, sha_b, bytes_b) = write_model(dir.path(), "b/b.gguf", 200);
        std::fs::write(
            dir.path().join("manifest.json"),
            manifest(
                &[
                    ("a", "a/a.gguf", &sha_a, bytes_a),
                    ("b", "b/b.gguf", &sha_b, bytes_b),
                    ("gone", "c/c.gguf", &sha_a, 5),
                ],
                "a",
            ),
        )
        .unwrap();
        let store = Store::open_in_memory().unwrap();
        let r = register_bundled(&store, dir.path()).unwrap();
        assert_eq!(
            (r.registered.len(), r.missing.clone(), r.default.as_deref()),
            (2, vec!["gone".to_string()], Some("a"))
        );
        let a = store.get_model("a").unwrap().unwrap();
        assert_eq!(
            (a.source.as_str(), a.quant.as_deref()),
            ("builtin", Some("Q4_K_M"))
        );
        assert!(a.path.ends_with("a.gguf") && a.gguf_meta.is_some());
        // Registering again is idempotent.
        register_bundled(&store, dir.path()).unwrap();
        assert_eq!(store.list_models().unwrap().len(), 2);
    }

    #[test]
    fn no_manifest_is_fine_and_a_bad_one_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        assert_eq!(
            register_bundled(&store, dir.path()).unwrap(),
            BundleReport::default()
        );
        std::fs::write(dir.path().join("manifest.json"), "{\"schema\": 1}").unwrap();
        assert!(matches!(
            register_bundled(&store, dir.path()),
            Err(ModelsError::Manifest(_))
        ));
        let bad_default = manifest(&[("a", "a.gguf", &"0".repeat(64), 1)], "zzz");
        assert!(parse_manifest(&bad_default).is_err());
    }

    #[test]
    fn manifest_paths_cannot_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let (_, sha, bytes) = write_model(outside.path(), "evil.gguf", 1);
        let rel = format!(
            "../{}/evil.gguf",
            outside.path().file_name().unwrap().to_string_lossy()
        );
        std::fs::write(
            dir.path().join("manifest.json"),
            manifest(&[("evil", &rel, &sha, bytes)], "evil"),
        )
        .unwrap();
        let store = Store::open_in_memory().unwrap();
        let r = register_bundled(&store, dir.path()).unwrap();
        assert!(r.registered.is_empty() && r.missing == vec!["evil".to_string()]);
    }

    #[test]
    fn active_model_resolution_falls_back_sensibly() {
        let dir = tempfile::tempdir().unwrap();
        let (_, sha_a, bytes_a) = write_model(dir.path(), "a/a.gguf", 10);
        let (_, sha_b, bytes_b) = write_model(dir.path(), "b/b.gguf", 20);
        std::fs::write(
            dir.path().join("manifest.json"),
            manifest(
                &[
                    ("a", "a/a.gguf", &sha_a, bytes_a),
                    ("b", "b/b.gguf", &sha_b, bytes_b),
                ],
                "b",
            ),
        )
        .unwrap();
        let store = Store::open_in_memory().unwrap();
        register_bundled(&store, dir.path()).unwrap();
        assert_eq!(
            resolve_active(&store, Some("a"), Some("b")).unwrap().as_deref(),
            Some("a")
        );
        assert_eq!(
            resolve_active(&store, Some("deleted"), Some("b"))
                .unwrap()
                .as_deref(),
            Some("b")
        );
        assert_eq!(resolve_active(&store, None, None).unwrap().as_deref(), Some("a"));
        std::fs::remove_file(dir.path().join("b/b.gguf")).unwrap();
        assert_eq!(
            resolve_active(&store, Some("b"), Some("b")).unwrap().as_deref(),
            Some("a")
        );
        assert_eq!(
            resolve_active(&Store::open_in_memory().unwrap(), None, None).unwrap(),
            None
        );
    }

    #[test]
    fn extra_folders_are_discovered_refreshed_and_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let nas = dir.path().join("nas");
        write_model(&nas, "x/tiny-q4.gguf", 5);
        write_model(&nas, "big-Q8_0-00001-of-00002.gguf", 5);
        write_model(&nas, "big-Q8_0-00002-of-00002.gguf", 5);
        std::fs::write(nas.join("notes.txt"), "hi").unwrap();
        std::fs::write(nas.join("broken.gguf"), "not a model").unwrap();
        let store = Store::open_in_memory().unwrap();
        assert_eq!(sync_extra_folders(&store, std::slice::from_ref(&nas)).unwrap(), 2);
        let rows = store.list_models().unwrap();
        assert!(
            rows.iter()
                .all(|m| m.source == "discovered" && m.sha256.is_none())
        );
        assert!(rows.iter().any(|m| m.name == "Tiny Test"));
        // Remove the folder from the list: rows go away, files stay.
        assert_eq!(sync_extra_folders(&store, &[]).unwrap(), 0);
        assert!(store.list_models().unwrap().is_empty());
        assert!(nas.join("x/tiny-q4.gguf").exists());
        assert_eq!(
            discovered_id(&nas.join("x/tiny-q4.gguf")),
            discovered_id(&nas.join("x/tiny-q4.gguf"))
        );
    }

    #[test]
    fn verify_bundled_detects_corruption_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let (pa, sha_a, bytes_a) = write_model(dir.path(), "a/a.gguf", 1000);
        let (pb, sha_b, bytes_b) = write_model(dir.path(), "b/b.gguf", 1000);
        let (_pc, sha_c, bytes_c) = write_model(dir.path(), "c/c.gguf", 1000);
        std::fs::write(
            dir.path().join("manifest.json"),
            manifest(
                &[
                    ("a", "a/a.gguf", &sha_a, bytes_a),
                    ("b", "b/b.gguf", &sha_b, bytes_b),
                    ("c", "c/c.gguf", &sha_c, bytes_c),
                ],
                "a",
            ),
        )
        .unwrap();
        let store = Store::open_in_memory().unwrap();
        register_bundled(&store, dir.path()).unwrap();
        let mut data = std::fs::read(&pb).unwrap();
        let last = data.len() - 1;
        data[last] ^= 1;
        std::fs::write(&pb, data).unwrap();
        std::fs::remove_file(dir.path().join("c/c.gguf")).unwrap();
        let _ = pa;
        let mut seen = Vec::new();
        let res = verify_bundled(&store, &CancellationToken::new(), |id, done, total| {
            seen.push((id.to_owned(), done, total))
        })
        .unwrap();
        let by = |id: &str| res.iter().find(|r| r.id == id).unwrap();
        assert!(by("a").ok);
        assert!(!by("b").ok && !by("b").missing);
        assert!(by("c").missing);
        assert_eq!(seen.last().map(|s| (s.1, s.2)), Some((3, 3)));
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(matches!(
            verify_bundled(&store, &cancel, |_, _, _| {}),
            Err(ModelsError::Cancelled)
        ));
    }
}
