//! Model packs: copy installed models to a folder or USB stick, and bring them into any NanoNexus
//! without the internet (MODEL-MANAGER §9). A pack is `NanoNexus-ModelPack-<name>/` with `pack.json`
//! (same entries as a bundle manifest, plus `"kind": "model-pack"`), `README.txt` and
//! `models/<id>/<file>.gguf`. Every file is verified by SHA-256 on the way in.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use nexus_store::{ModelRow, Store};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::bundled::{ManifestModel, ModelsError, hash_file, meta_json, now_ms, safe_join, slug};
use crate::download::available_space;

/// `pack.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pack {
    pub schema: u32,
    pub kind: String,
    pub name: String,
    pub created_at: String,
    #[serde(default)]
    pub created_by: Option<String>,
    pub models: Vec<ManifestModel>,
}

/// Where a running export or import is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct PackProgress {
    /// `copying` (export, or copy-import) or `verifying` (hashing in place).
    pub phase: String,
    pub model_id: String,
    /// 1-based position of the model being handled.
    pub model_index: u32,
    pub model_total: u32,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

/// A model that could not be brought in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct PackFailure {
    pub id: String,
    pub reason: String,
}

/// What an import did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct PackImport {
    pub imported: Vec<String>,
    /// Already installed with the same checksum.
    pub already_installed: Vec<String>,
    pub failed: Vec<PackFailure>,
}

/// What an export wrote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct PackExport {
    pub dir: String,
    pub models: u32,
    pub bytes: u64,
}

/// Copy the files into the app's folder, or use them where they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportMode {
    Copy,
    Reference,
}

/// How an imported model is recorded: a user import, or part of an offline kit (read-only, "Bundled").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackSource {
    Import,
    Bundled,
}

impl PackSource {
    fn db(self) -> &'static str {
        match self {
            PackSource::Import => "import",
            PackSource::Bundled => "builtin",
        }
    }
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Read and check `<dir>/pack.json`.
pub fn read_pack(dir: &Path) -> Result<Pack, ModelsError> {
    let text = std::fs::read_to_string(dir.join("pack.json"))
        .map_err(|e| ModelsError::Manifest(format!("{}: {e}", dir.join("pack.json").display())))?;
    let pack: Pack = serde_json::from_str(&text).map_err(|e| ModelsError::Manifest(e.to_string()))?;
    let bad = |m: String| Err(ModelsError::Manifest(m));
    if pack.schema != 1 || pack.kind != "model-pack" {
        return bad(
            "this is not a NanoNexus model pack (pack.json needs schema 1 and kind \"model-pack\")".into(),
        );
    }
    if pack.models.is_empty() {
        return bad("the pack has no models".into());
    }
    for m in &pack.models {
        if !is_sha256(&m.sha256) {
            return bad(format!("{}: sha256 must be 64 lowercase hex digits", m.id));
        }
        if m.bytes == 0 || m.id.is_empty() || m.path.is_empty() {
            return bad(format!("{}: id, path and bytes are required", m.id));
        }
    }
    Ok(pack)
}

fn check_space(path: &Path, need: u64) -> Result<(), ModelsError> {
    if let Some(have) = available_space(path)
        && have < need.saturating_add(256 * 1024 * 1024)
    {
        return Err(ModelsError::Io(std::io::Error::other(format!(
            "not enough free disk space: need {} MB, have {} MB",
            need / (1024 * 1024),
            have / (1024 * 1024)
        ))));
    }
    Ok(())
}

/// Stream-copy `src` to `dest` while hashing it. Reports bytes and honours `cancel`.
fn copy_hashed(
    src: &Path,
    dest: &Path,
    cancel: &CancellationToken,
    mut on_bytes: impl FnMut(u64),
) -> Result<String, ModelsError> {
    let mut from = std::fs::File::open(src)?;
    let mut to = std::fs::File::create(dest)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(ModelsError::Cancelled);
        }
        let n = from.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        to.write_all(&buf[..n])?;
        total += n as u64;
        on_bytes(total);
    }
    to.sync_all()?;
    Ok(hex(&h.finalize()))
}

fn readme(name: &str, models: &[ManifestModel]) -> String {
    let mut s = format!("NanoNexus model pack: {name}\n\nModels in this pack:\n");
    for m in models {
        s.push_str(&format!(
            "  - {} ({}, {:.2} GB)\n",
            m.name,
            m.id,
            m.bytes as f64 / 1024f64.powi(3)
        ));
    }
    s.push_str(
        "\nTo install on any NanoNexus (macOS or Windows), no internet needed:\n  1. Open NanoNexus → Models → \"Import model pack…\"\n  2. Choose this folder.\n  3. Wait while every file is verified, then pick the model in the chat header.\n",
    );
    s
}

/// Copy the installed models `ids` into `<parent>/NanoNexus-ModelPack-<name>/`.
/// A partly written pack is removed on failure or cancel.
pub fn export_pack(
    store: &Store,
    ids: &[String],
    parent: &Path,
    name: &str,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(PackProgress),
) -> Result<PackExport, ModelsError> {
    if ids.is_empty() {
        return Err(ModelsError::Manifest("select at least one model".into()));
    }
    let mut rows = Vec::new();
    for id in ids {
        let row = store
            .get_model(id)?
            .ok_or_else(|| ModelsError::Manifest(format!("unknown model `{id}`")))?;
        if !Path::new(&row.path).is_file() {
            return Err(ModelsError::Manifest(format!(
                "`{}` is missing on disk",
                row.name
            )));
        }
        rows.push(row);
    }
    let slug_name = {
        let s = slug(name);
        if s.is_empty() { "models".to_owned() } else { s }
    };
    let dir = parent.join(format!("NanoNexus-ModelPack-{slug_name}"));
    if dir.join("pack.json").exists() {
        return Err(ModelsError::Manifest(format!(
            "{} already holds a pack; choose another name or remove it",
            dir.display()
        )));
    }
    let total: u64 = rows.iter().map(|r| r.bytes).sum();
    std::fs::create_dir_all(parent)?;
    check_space(parent, total)?;

    let result = (|| -> Result<PackExport, ModelsError> {
        let mut entries = Vec::new();
        let mut done_before = 0u64;
        for (i, row) in rows.iter().enumerate() {
            let file = Path::new(&row.path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .ok_or_else(|| ModelsError::Manifest(format!("{}: no file name", row.name)))?;
            let dest = dir.join("models").join(&row.id).join(&file);
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p)?;
            }
            let progress = |b: u64| PackProgress {
                phase: "copying".into(),
                model_id: row.id.clone(),
                model_index: i as u32 + 1,
                model_total: rows.len() as u32,
                bytes_done: done_before + b,
                bytes_total: total,
            };
            on_progress(progress(0));
            let sha = copy_hashed(Path::new(&row.path), &dest, cancel, |b| on_progress(progress(b)))?;
            let bytes = std::fs::metadata(&dest)?.len();
            if let Some(expected) = row.sha256.as_deref()
                && !expected.eq_ignore_ascii_case(&sha)
            {
                return Err(ModelsError::Manifest(format!(
                    "`{}` does not match its recorded checksum; it may be damaged",
                    row.name
                )));
            }
            let extra: Option<serde_json::Value> =
                row.extra.as_deref().and_then(|e| serde_json::from_str(e).ok());
            entries.push(ManifestModel {
                id: row.id.clone(),
                name: row.name.clone(),
                path: format!("{}/{file}", row.id),
                sha256: sha,
                bytes,
                repo: row.repo.clone(),
                revision: row.revision.clone(),
                license: row.license.clone(),
                catalog_id: row.catalog_id.clone(),
                sampling: extra
                    .as_ref()
                    .and_then(|e| e.get("sampling"))
                    .filter(|v| !v.is_null())
                    .cloned(),
                capabilities: extra
                    .as_ref()
                    .and_then(|e| e.get("capabilities"))
                    .and_then(|c| serde_json::from_value(c.clone()).ok())
                    .unwrap_or_default(),
            });
            done_before += bytes;
        }
        let pack = Pack {
            schema: 1,
            kind: "model-pack".into(),
            name: slug_name.clone(),
            created_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            created_by: Some("NanoNexus".into()),
            models: entries,
        };
        std::fs::write(dir.join("README.txt"), readme(&slug_name, &pack.models))?;
        std::fs::write(
            dir.join("pack.json"),
            serde_json::to_string_pretty(&pack).map_err(|e| ModelsError::Manifest(e.to_string()))? + "\n",
        )?;
        Ok(PackExport {
            dir: dir.display().to_string(),
            models: pack.models.len() as u32,
            bytes: done_before,
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

/// Bring the models of the pack in `pack_dir` into the app. One bad file does not stop the others;
/// cancelling stops at once (and deletes the file being copied).
pub fn import_pack(
    store: &Store,
    pack_dir: &Path,
    models_dir: &Path,
    mode: ImportMode,
    source: PackSource,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(PackProgress),
) -> Result<PackImport, ModelsError> {
    let pack = read_pack(pack_dir)?;
    let base = pack_dir.join("models");
    let mut report = PackImport::default();
    let mut todo: Vec<(&ManifestModel, PathBuf)> = Vec::new();
    for m in &pack.models {
        if let Some(row) = store.get_model(&m.id)?
            && row
                .sha256
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(&m.sha256))
            && Path::new(&row.path).is_file()
        {
            report.already_installed.push(m.id.clone());
            continue;
        }
        let Some(src) = safe_join(&base, &m.path) else {
            report.failed.push(PackFailure {
                id: m.id.clone(),
                reason: "unsafe path in pack.json".into(),
            });
            continue;
        };
        match std::fs::metadata(&src) {
            Ok(md) if md.len() == m.bytes => todo.push((m, src)),
            Ok(md) => report.failed.push(PackFailure {
                id: m.id.clone(),
                reason: format!("the file is {} bytes, pack.json says {}", md.len(), m.bytes),
            }),
            Err(_) => report.failed.push(PackFailure {
                id: m.id.clone(),
                reason: "the file is missing".into(),
            }),
        }
    }
    let total: u64 = todo.iter().map(|(m, _)| m.bytes).sum();
    if mode == ImportMode::Copy && total > 0 {
        std::fs::create_dir_all(models_dir)?;
        check_space(models_dir, total)?;
    }

    let n = todo.len() as u32;
    let mut done_before = 0u64;
    for (i, (m, src)) in todo.into_iter().enumerate() {
        let phase = if mode == ImportMode::Copy {
            "copying"
        } else {
            "verifying"
        };
        let progress = |b: u64| PackProgress {
            phase: phase.into(),
            model_id: m.id.clone(),
            model_index: i as u32 + 1,
            model_total: n,
            bytes_done: done_before + b,
            bytes_total: total,
        };
        on_progress(progress(0));
        let outcome = (|| -> Result<PathBuf, ModelsError> {
            match mode {
                ImportMode::Copy => {
                    let file = src.file_name().map(|f| f.to_owned()).unwrap_or_default();
                    let dest_dir = models_dir.join("packs").join(slug(&m.id));
                    std::fs::create_dir_all(&dest_dir)?;
                    let dest = dest_dir.join(&file);
                    let part = dest_dir.join(format!("{}.part", file.to_string_lossy()));
                    let sha = match copy_hashed(&src, &part, cancel, |b| on_progress(progress(b))) {
                        Ok(s) => s,
                        Err(e) => {
                            let _ = std::fs::remove_file(&part);
                            return Err(e);
                        }
                    };
                    if !sha.eq_ignore_ascii_case(&m.sha256) {
                        let _ = std::fs::remove_file(&part);
                        return Err(ModelsError::Manifest(
                            "checksum mismatch: the file was damaged".into(),
                        ));
                    }
                    std::fs::rename(&part, &dest)?;
                    Ok(dest)
                }
                ImportMode::Reference => {
                    let sha = hash_file(&src, cancel, |b| on_progress(progress(b)))?;
                    if !sha.eq_ignore_ascii_case(&m.sha256) {
                        return Err(ModelsError::Manifest(
                            "checksum mismatch: the file was damaged".into(),
                        ));
                    }
                    Ok(src.clone())
                }
            }
        })();
        match outcome {
            Err(ModelsError::Cancelled) => return Err(ModelsError::Cancelled),
            Err(e) => report.failed.push(PackFailure {
                id: m.id.clone(),
                reason: e.to_string(),
            }),
            Ok(path) => {
                let (gguf_meta, meta) = meta_json(&path);
                let extra = serde_json::json!({"sampling": m.sampling, "capabilities": m.capabilities, "storage": if mode == ImportMode::Copy { "copy" } else { "reference" }});
                store.upsert_model(&ModelRow {
                    id: m.id.clone(),
                    source: source.db().into(),
                    name: m.name.clone(),
                    repo: m.repo.clone(),
                    revision: m.revision.clone(),
                    file: path.file_name().map(|f| f.to_string_lossy().into_owned()),
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
                report.imported.push(m.id.clone());
            }
        }
        done_before += m.bytes;
    }
    Ok(report)
}

/// The Windows offline kit: `<exe_dir>/models-extra/pack.json` is imported in place (no copy) and the
/// models count as bundled. Models already registered with the same checksum are skipped, so later
/// launches do not hash gigabytes again. No `pack.json` = nothing to do.
pub fn register_models_extra(
    store: &Store,
    dir: &Path,
    cancel: &CancellationToken,
    on_progress: impl FnMut(PackProgress),
) -> Result<PackImport, ModelsError> {
    if !dir.join("pack.json").is_file() {
        return Ok(PackImport::default());
    }
    import_pack(
        store,
        dir,
        dir,
        ImportMode::Reference,
        PackSource::Bundled,
        cancel,
        on_progress,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gguf::fixture;

    fn add_model(store: &Store, dir: &Path, id: &str, pad: usize) -> (PathBuf, String) {
        let p = dir.join(id).join(format!("{id}.gguf"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut data = fixture::sample();
        data.resize(data.len() + pad, 7);
        std::fs::write(&p, &data).unwrap();
        let sha = hex(&Sha256::digest(&data));
        store
            .upsert_model(&ModelRow {
                id: id.into(),
                source: "download".into(),
                name: format!("Model {id}"),
                repo: Some("o/r".into()),
                revision: None,
                file: Some(format!("{id}.gguf")),
                path: p.display().to_string(),
                sha256: Some(sha.clone()),
                bytes: data.len() as u64,
                quant: None,
                catalog_id: None,
                gguf_meta: None,
                license: Some("mit".into()),
                extra: Some(
                    r#"{"sampling":{"no_think":{"temperature":0.5}},"capabilities":["chat"]}"#.into(),
                ),
                added_at: 1,
            })
            .unwrap();
        (p, sha)
    }

    fn setup() -> (tempfile::TempDir, Store, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let src = tmp.path().join("src");
        add_model(&store, &src, "a", 500);
        add_model(&store, &src, "b", 900);
        (tmp, store, src)
    }

    #[test]
    fn export_writes_a_verified_pack_and_import_installs_it_elsewhere() {
        let (tmp, store, _) = setup();
        let usb = tmp.path().join("usb");
        let mut seen = Vec::new();
        let ex = export_pack(
            &store,
            &["a".into(), "b".into()],
            &usb,
            "Studio Pack",
            &CancellationToken::new(),
            |p| seen.push(p),
        )
        .unwrap();
        assert_eq!(ex.models, 2);
        assert!(ex.dir.ends_with("NanoNexus-ModelPack-studio-pack"));
        assert!(seen.iter().any(|p| p.model_index == 2) && seen.iter().all(|p| p.phase == "copying"));
        let pack = read_pack(Path::new(&ex.dir)).unwrap();
        assert_eq!(
            pack.models[0].sampling.as_ref().unwrap()["no_think"]["temperature"],
            0.5
        );
        assert!(Path::new(&ex.dir).join("README.txt").is_file());

        // A second machine: a fresh database and data dir. Copy mode.
        let other = Store::open_in_memory().unwrap();
        let data = tmp.path().join("data");
        let r = import_pack(
            &other,
            Path::new(&ex.dir),
            &data,
            ImportMode::Copy,
            PackSource::Import,
            &CancellationToken::new(),
            |_| {},
        )
        .unwrap();
        assert_eq!(r.imported, ["a", "b"]);
        let row = other.get_model("a").unwrap().unwrap();
        assert_eq!(row.source, "import");
        assert!(row.path.starts_with(data.to_str().unwrap()) && Path::new(&row.path).is_file());
        assert_eq!(row.sha256, pack.models[0].sha256.clone().into());
        assert!(row.gguf_meta.is_some());
        // Importing again changes nothing.
        let again = import_pack(
            &other,
            Path::new(&ex.dir),
            &data,
            ImportMode::Copy,
            PackSource::Import,
            &CancellationToken::new(),
            |_| {},
        )
        .unwrap();
        assert_eq!((again.imported.len(), again.already_installed.len()), (0, 2));
    }

    #[test]
    fn reference_mode_uses_the_files_in_place_and_corruption_is_rejected() {
        let (tmp, store, _) = setup();
        let ex = export_pack(
            &store,
            &["a".into(), "b".into()],
            &tmp.path().join("usb"),
            "p",
            &CancellationToken::new(),
            |_| {},
        )
        .unwrap();
        let packdir = PathBuf::from(&ex.dir);
        // Flip a byte of b.
        let b = packdir.join("models/b/b.gguf");
        let mut data = std::fs::read(&b).unwrap();
        let last = data.len() - 1;
        data[last] ^= 1;
        std::fs::write(&b, data).unwrap();
        let other = Store::open_in_memory().unwrap();
        let mut phases = Vec::new();
        let r = import_pack(
            &other,
            &packdir,
            tmp.path(),
            ImportMode::Reference,
            PackSource::Import,
            &CancellationToken::new(),
            |p| phases.push(p.phase),
        )
        .unwrap();
        assert_eq!(r.imported, ["a"]);
        assert_eq!(r.failed.len(), 1);
        assert_eq!(r.failed[0].id, "b");
        assert!(r.failed[0].reason.contains("checksum mismatch"));
        assert!(phases.iter().all(|p| p == "verifying"));
        assert_eq!(
            other.get_model("a").unwrap().unwrap().path,
            packdir.join("models/a/a.gguf").display().to_string()
        );
        assert!(other.get_model("b").unwrap().is_none());
    }

    #[test]
    fn copy_mode_leaves_no_partial_file_on_corruption_or_cancel() {
        let (tmp, store, _) = setup();
        let ex = export_pack(
            &store,
            &["a".into()],
            &tmp.path().join("usb"),
            "p",
            &CancellationToken::new(),
            |_| {},
        )
        .unwrap();
        let packdir = PathBuf::from(&ex.dir);
        let a = packdir.join("models/a/a.gguf");
        let mut data = std::fs::read(&a).unwrap();
        data[100] ^= 1;
        std::fs::write(&a, data).unwrap();
        let other = Store::open_in_memory().unwrap();
        let data_dir = tmp.path().join("data");
        let r = import_pack(
            &other,
            &packdir,
            &data_dir,
            ImportMode::Copy,
            PackSource::Import,
            &CancellationToken::new(),
            |_| {},
        )
        .unwrap();
        assert_eq!(r.failed.len(), 1);
        let left: Vec<_> = walk(&data_dir);
        assert!(left.is_empty(), "left behind: {left:?}");

        let cancel = CancellationToken::new();
        cancel.cancel();
        let res = import_pack(
            &other,
            &packdir,
            &data_dir,
            ImportMode::Copy,
            PackSource::Import,
            &cancel,
            |_| {},
        );
        assert!(matches!(res, Err(ModelsError::Cancelled)));
        assert!(walk(&data_dir).is_empty());
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    out.extend(walk(&p));
                } else {
                    out.push(p);
                }
            }
        }
        out
    }

    #[test]
    fn a_cancelled_export_removes_the_partial_pack_and_existing_packs_are_not_overwritten() {
        let (tmp, store, _) = setup();
        let usb = tmp.path().join("usb");
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(matches!(
            export_pack(&store, &["a".into()], &usb, "p", &cancel, |_| {}),
            Err(ModelsError::Cancelled)
        ));
        assert!(!usb.join("NanoNexus-ModelPack-p").exists());
        export_pack(
            &store,
            &["a".into()],
            &usb,
            "p",
            &CancellationToken::new(),
            |_| {},
        )
        .unwrap();
        assert!(matches!(
            export_pack(&store, &["a".into()], &usb, "p", &CancellationToken::new(), |_| {}),
            Err(ModelsError::Manifest(m)) if m.contains("already holds a pack")
        ));
        assert!(matches!(
            export_pack(&store, &[], &usb, "q", &CancellationToken::new(), |_| {}),
            Err(ModelsError::Manifest(_))
        ));
        assert!(matches!(
            export_pack(
                &store,
                &["ghost".into()],
                &usb,
                "q",
                &CancellationToken::new(),
                |_| {}
            ),
            Err(ModelsError::Manifest(_))
        ));
    }

    #[test]
    fn bad_packs_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_pack(tmp.path()).is_err());
        let write = |json: &str| std::fs::write(tmp.path().join("pack.json"), json).unwrap();
        write(r#"{"schema":1,"kind":"other","name":"x","created_at":"t","models":[]}"#);
        assert!(read_pack(tmp.path()).is_err());
        write(
            r#"{"schema":1,"kind":"model-pack","name":"x","created_at":"t","models":[{"id":"a","name":"A","path":"a/a.gguf","sha256":"zz","bytes":1}]}"#,
        );
        assert!(
            read_pack(tmp.path())
                .unwrap_err()
                .to_string()
                .contains("64 lowercase hex")
        );
        // A path that climbs out of the pack is reported, not followed.
        let sha = "0".repeat(64);
        write(&format!(
            r#"{{"schema":1,"kind":"model-pack","name":"x","created_at":"t","models":[{{"id":"evil","name":"E","path":"../evil.gguf","sha256":"{sha}","bytes":1}}]}}"#
        ));
        let store = Store::open_in_memory().unwrap();
        let r = import_pack(
            &store,
            tmp.path(),
            tmp.path(),
            ImportMode::Reference,
            PackSource::Import,
            &CancellationToken::new(),
            |_| {},
        )
        .unwrap();
        assert_eq!(r.failed[0].reason, "unsafe path in pack.json");
    }

    #[test]
    fn models_extra_registers_in_place_as_bundled_and_skips_known_models() {
        let (tmp, store, _) = setup();
        let ex = export_pack(
            &store,
            &["a".into(), "b".into()],
            tmp.path(),
            "kit",
            &CancellationToken::new(),
            |_| {},
        )
        .unwrap();
        let extra = tmp.path().join("models-extra");
        std::fs::rename(&ex.dir, &extra).unwrap();
        let fresh = Store::open_in_memory().unwrap();
        let r = register_models_extra(&fresh, &extra, &CancellationToken::new(), |_| {}).unwrap();
        assert_eq!(r.imported.len(), 2);
        let row = fresh.get_model("a").unwrap().unwrap();
        assert_eq!(row.source, "builtin");
        assert!(row.path.starts_with(extra.to_str().unwrap()));
        // Next launch: nothing is hashed again.
        let mut calls = 0;
        let r2 = register_models_extra(&fresh, &extra, &CancellationToken::new(), |_| calls += 1).unwrap();
        assert_eq!((r2.imported.len(), r2.already_installed.len(), calls), (0, 2, 0));
        // No pack.json: nothing to do.
        assert_eq!(
            register_models_extra(
                &fresh,
                tmp.path().join("none").as_path(),
                &CancellationToken::new(),
                |_| {}
            )
            .unwrap(),
            PackImport::default()
        );
    }
}
