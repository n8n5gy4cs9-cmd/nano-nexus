//! Where the `llama-server` binary and the models live (BUNDLING §4, §11).
//!
//! Everything here is a pure function of directories passed in, so it is testable without Tauri.
//! llama.cpp opens GGUF files by path (mmap): never embed or copy them.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// A model the supervisor can load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelChoice {
    pub id: String,
    pub path: PathBuf,
}

/// One entry of `models/manifest.json` (written by `scripts/stage-bundle.mjs`).
#[derive(Clone, Debug, Deserialize)]
pub struct ManifestModel {
    pub id: String,
    pub name: Option<String>,
    /// Relative to the manifest's folder.
    pub path: String,
}

/// `models/manifest.json` (MODEL-MANAGER §9).
#[derive(Clone, Debug, Deserialize)]
pub struct BundleManifest {
    pub default: String,
    pub models: Vec<ManifestModel>,
}

/// `vendor/models/index.json` (dev builds).
#[derive(Clone, Debug, Deserialize)]
struct VendorIndex {
    default: String,
    models: std::collections::BTreeMap<String, VendorEntry>,
}

#[derive(Clone, Debug, Deserialize)]
struct VendorEntry {
    file: String,
}

/// Path of the `llama-server` executable.
///
/// Windows: `<resource_dir>/runtime/llama-server.exe` (its DLLs sit beside it).
/// Elsewhere: next to the app executable (Tauri copies `externalBin` there, also in dev).
pub fn server_binary(resource_dir: &Path, exe_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        resource_dir.join("runtime").join("llama-server.exe")
    } else {
        exe_dir.join("llama-server")
    }
}

/// Debug builds only: the `llama-server` that `scripts/setup-runtime.sh` built under `<repo>/vendor/runtime`.
/// `tauri dev` has no `externalBin` copy next to the executable, so without this the runtime never starts.
pub fn vendor_server_binary(vendor_dir: &Path) -> Option<PathBuf> {
    let runtime = vendor_dir.join("runtime");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&runtime)
        .ok()?
        .flatten()
        .filter(|d| d.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|d| std::fs::read_dir(d.path()).ok())
        .flat_map(|rd| rd.flatten())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("llama-server"))
                && p.extension().is_none_or(|x| x != "json")
        })
        .collect();
    found.sort();
    // Prefer the folder for this OS and CPU.
    let arch = if std::env::consts::ARCH == "aarch64" { "arm64" } else { std::env::consts::ARCH };
    let host = format!("{}-{arch}", std::env::consts::OS);
    let mine = found.iter().find(|p| p.to_string_lossy().contains(&host)).cloned();
    mine.or_else(|| found.into_iter().next())
}

/// Read `<models_dir>/manifest.json`.
pub fn read_manifest(models_dir: &Path) -> Option<BundleManifest> {
    serde_json::from_slice(&std::fs::read(models_dir.join("manifest.json")).ok()?).ok()
}

/// The bundled model with id `active_id`, else the manifest default. `None` if there is no manifest
/// or the chosen file is missing.
pub fn bundled_model(resource_dir: &Path, active_id: &str) -> Option<ModelChoice> {
    let dir = resource_dir.join("models");
    let manifest = read_manifest(&dir)?;
    let pick = manifest
        .models
        .iter()
        .find(|m| m.id == active_id)
        .or_else(|| manifest.models.iter().find(|m| m.id == manifest.default))?;
    let path = dir.join(&pick.path);
    path.is_file().then(|| ModelChoice {
        id: pick.id.clone(),
        path,
    })
}

/// The default model recorded in `vendor/models/index.json` (dev builds).
pub fn vendor_model(vendor_dir: &Path) -> Option<ModelChoice> {
    let dir = vendor_dir.join("models");
    let index: VendorIndex = serde_json::from_slice(&std::fs::read(dir.join("index.json")).ok()?).ok()?;
    let entry = index.models.get(&index.default)?;
    let path = dir.join(&index.default).join(&entry.file);
    path.is_file().then(|| ModelChoice {
        id: index.default.clone(),
        path,
    })
}

/// Inputs for [`resolve_model`].
#[derive(Clone, Debug, Default)]
pub struct ModelSearch {
    /// `NEXUS_MODEL_PATH` (honoured by the caller in debug builds only).
    pub env_override: Option<PathBuf>,
    pub resource_dir: PathBuf,
    /// Setting `models.active_model_id` (`"builtin"` = the bundle default).
    pub active_model_id: String,
    /// `<repo>/vendor`, set only in debug builds.
    pub vendor_dir: Option<PathBuf>,
}

/// Pick the model to load: env override, then the bundle, then the dev vendor folder, else `None`
/// (onboarding shows "Download a model").
pub fn resolve_model(s: &ModelSearch) -> Option<ModelChoice> {
    if let Some(p) = s.env_override.as_ref().filter(|p| p.is_file()) {
        let id = p
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "custom".into());
        return Some(ModelChoice { id, path: p.clone() });
    }
    bundled_model(&s.resource_dir, &s.active_model_id)
        .or_else(|| s.vendor_dir.as_deref().and_then(vendor_model))
}

/// Extra folders scanned for models: `<exe_dir>/models-extra` (Windows offline kit) plus the user's
/// folders from `models.extra_dirs`.
pub fn extra_model_dirs(exe_dir: &Path, user_dirs: &[String]) -> Vec<PathBuf> {
    let mut dirs = vec![exe_dir.join("models-extra")];
    dirs.extend(user_dirs.iter().map(PathBuf::from));
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn bundle(res: &Path) {
        write(
            &res.join("models/manifest.json"),
            r#"{"schema":1,"default":"small","models":[
                {"id":"small","name":"Small","path":"small/s.gguf"},
                {"id":"big","path":"big/b.gguf"}]}"#,
        );
        write(&res.join("models/small/s.gguf"), "GGUF");
        write(&res.join("models/big/b.gguf"), "GGUF");
    }

    #[test]
    fn server_binary_location() {
        let p = server_binary(Path::new("/res"), Path::new("/app/MacOS"));
        if cfg!(windows) {
            assert_eq!(p, Path::new("/res/runtime/llama-server.exe"));
        } else {
            assert_eq!(p, Path::new("/app/MacOS/llama-server"));
        }
    }

    #[test]
    fn bundled_default_and_active_selection() {
        let dir = tempfile::tempdir().unwrap();
        bundle(dir.path());
        assert_eq!(bundled_model(dir.path(), "builtin").unwrap().id, "small");
        assert_eq!(bundled_model(dir.path(), "big").unwrap().id, "big");
        assert_eq!(
            bundled_model(dir.path(), "gone").unwrap().id,
            "small",
            "invalid id falls back to default"
        );
    }

    #[test]
    fn missing_file_or_manifest_gives_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(bundled_model(dir.path(), "builtin").is_none());
        bundle(dir.path());
        std::fs::remove_file(dir.path().join("models/small/s.gguf")).unwrap();
        assert!(bundled_model(dir.path(), "builtin").is_none());
    }

    #[test]
    fn vendor_index_default() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("models/index.json"),
            r#"{"default":"m","models":{"m":{"file":"m.gguf","sha256":"x","bytes":1}}}"#,
        );
        assert!(vendor_model(dir.path()).is_none(), "file not downloaded yet");
        write(&dir.path().join("models/m/m.gguf"), "GGUF");
        assert_eq!(
            vendor_model(dir.path()).unwrap().path,
            dir.path().join("models/m/m.gguf")
        );
    }

    #[test]
    fn resolution_order() {
        let res = tempfile::tempdir().unwrap();
        let vendor = tempfile::tempdir().unwrap();
        let env = tempfile::tempdir().unwrap();
        write(
            &vendor.path().join("models/index.json"),
            r#"{"default":"v","models":{"v":{"file":"v.gguf"}}}"#,
        );
        write(&vendor.path().join("models/v/v.gguf"), "GGUF");
        write(&env.path().join("mine.gguf"), "GGUF");

        let mut s = ModelSearch {
            resource_dir: res.path().to_owned(),
            active_model_id: "builtin".into(),
            vendor_dir: Some(vendor.path().to_owned()),
            ..Default::default()
        };
        assert_eq!(resolve_model(&s).unwrap().id, "v", "vendor when no bundle");
        bundle(res.path());
        assert_eq!(resolve_model(&s).unwrap().id, "small", "bundle beats vendor");
        s.env_override = Some(env.path().join("mine.gguf"));
        assert_eq!(resolve_model(&s).unwrap().id, "mine", "env override wins");
        s.env_override = Some(env.path().join("missing.gguf"));
        assert_eq!(resolve_model(&s).unwrap().id, "small", "bad override is ignored");
        s.vendor_dir = None;
        std::fs::remove_dir_all(res.path().join("models")).unwrap();
        assert!(resolve_model(&s).is_none());
    }

    #[test]
    fn extra_dirs_include_the_offline_kit_folder() {
        let d = extra_model_dirs(Path::new("/app"), &["/mnt/models".into()]);
        assert_eq!(
            d,
            vec![PathBuf::from("/app/models-extra"), PathBuf::from("/mnt/models")]
        );
    }
}
