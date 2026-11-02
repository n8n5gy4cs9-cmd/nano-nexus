use serde::{Deserialize, Serialize};
use specta::Type;

use crate::state::AppCtx;

use super::Caller;

/// What `bundle.json` (written by `scripts/stage-bundle.mjs`) says about how this app was built.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, Type)]
pub struct BuildInfo {
    /// The bundle profile (`basic`, `studio`, …).
    pub profile: String,
    #[serde(default)]
    pub git_commit: Option<String>,
    /// ISO-8601 build time.
    #[serde(default)]
    pub built_at: Option<String>,
    /// Ids of the models inside the build.
    #[serde(default)]
    pub models: Vec<String>,
}

/// Static facts about this install.
#[derive(Clone, Debug, Serialize, Type)]
pub struct AppInfo {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub data_dir: String,
    pub log_dir: String,
    /// Desktop-only paths for this running install, including registered external sources.
    pub locations: Vec<StorageLocation>,
    /// `None` in dev builds, which have no `bundle.json`.
    pub build: Option<BuildInfo>,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct StorageLocation {
    pub label: String,
    pub path: String,
    pub purpose: String,
    pub exists: bool,
}

/// Read `<resource_dir>/bundle.json`; a missing or broken file is simply "no build info".
pub fn read_build_info(resource_dir: &std::path::Path) -> Option<BuildInfo> {
    let text = std::fs::read_to_string(resource_dir.join("bundle.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Only plain web links may be handed to the OS browser.
pub fn validate_external_url(url: &str) -> Result<(), nexus_core::error::NexusError> {
    let ok = (url.starts_with("https://") || url.starts_with("http://"))
        && !url.contains(char::is_whitespace)
        && url.len() <= 2048;
    if ok {
        Ok(())
    } else {
        Err(nexus_core::error::NexusError::InvalidInput(
            "only http(s) links can be opened".into(),
        ))
    }
}

/// Remotes see the version and platform but not where the data lives.
pub fn info(ctx: &AppCtx, caller: &Caller) -> AppInfo {
    let (data_dir, log_dir) = if caller.is_desktop() {
        (
            ctx.paths.data.to_string_lossy().into_owned(),
            ctx.paths.logs.to_string_lossy().into_owned(),
        )
    } else {
        (String::new(), String::new())
    };
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        data_dir,
        log_dir,
        locations: if caller.is_desktop() {
            locations(ctx)
        } else {
            Vec::new()
        },
        build: read_build_info(&ctx.resource_dir),
    }
}

fn locations(ctx: &AppCtx) -> Vec<StorageLocation> {
    let mut out = Vec::new();
    let mut add = |label: &str, path: &std::path::Path, purpose: &str| {
        out.push(StorageLocation {
            label: label.into(),
            path: path.to_string_lossy().into_owned(),
            purpose: purpose.into(),
            exists: path.exists(),
        });
    };
    if let Ok(executable) = std::env::current_exe() {
        add(
            "Running executable",
            &executable,
            "The app binary currently running; an .app or launcher may contain it.",
        );
    }
    add(
        "Executable folder",
        &ctx.exe_dir,
        "The folder beside this app binary; portable.flag here selects portable data.",
    );
    add(
        "Bundled resources",
        &ctx.resource_dir,
        "Models and other files shipped with this build.",
    );
    add(
        "App data",
        &ctx.paths.data,
        "The data root for this running install.",
    );
    add(
        "Database",
        &ctx.paths.db_file(),
        "Chats, settings, model registrations and library indexes.",
    );
    add(
        "Managed models",
        &ctx.paths.models,
        "Models copied or downloaded into app data.",
    );
    add(
        "Change journal",
        &ctx.paths.journal,
        "Change previews, Apply and Undo records.",
    );
    add("Reports", &ctx.paths.reports, "Saved analysis reports.");
    add(
        "Runtime state",
        &ctx.paths.runtime,
        "Local model server state and runtime files.",
    );
    add("Cache", &ctx.paths.cache, "Rebuildable cached data.");
    add("Logs", &ctx.paths.logs, "Local diagnostic logs.");
    let settings = ctx.settings_snapshot();
    for path in &settings.models_extra_dirs {
        add(
            "Extra model folder",
            std::path::Path::new(path),
            "Configured folder scanned for GGUF models.",
        );
    }
    if let Ok(models) = ctx.store.list_models() {
        for model in models {
            add(
                &format!("Model: {}", model.name),
                std::path::Path::new(&model.path),
                "Registered GGUF file; the file may be stored outside app data.",
            );
        }
    }
    if let Ok(libraries) = super::knowledge::list(ctx) {
        for library in libraries {
            add(
                &format!("Library: {}", library.name),
                std::path::Path::new(&library.path),
                "Library source folder; referenced sources stay at this path.",
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::settings::SharePolicy;

    #[test]
    fn external_urls_are_limited_to_web_links() {
        assert!(validate_external_url("https://example.com/a?b=1").is_ok());
        assert!(validate_external_url("http://localhost:3000").is_ok());
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ftp://x",
            "https://a b",
            "tauri://localhost",
            "",
        ] {
            assert!(validate_external_url(bad).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn build_info_comes_from_bundle_json() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_build_info(dir.path()), None);
        std::fs::write(dir.path().join("bundle.json"), "not json").unwrap();
        assert_eq!(read_build_info(dir.path()), None);
        std::fs::write(
            dir.path().join("bundle.json"),
            r#"{"profile":"studio","git_commit":"abc123def456","built_at":"2026-10-20T12:00:00Z","app_version":"0.1.0","target":"macos-arm64","models":["a","b"],"knowledge":[]}"#,
        )
        .unwrap();
        let b = read_build_info(dir.path()).unwrap();
        assert_eq!(
            (b.profile.as_str(), b.git_commit.as_deref(), b.models.len()),
            ("studio", Some("abc123def456"), 2)
        );
        // The app context points at a resources folder; without a bundle there is no build info.
        let ctx = AppCtx::for_tests(dir.path());
        assert_eq!(info(&ctx, &Caller::Desktop).build, None);
        std::fs::create_dir_all(&ctx.resource_dir).unwrap();
        std::fs::write(ctx.resource_dir.join("bundle.json"), r#"{"profile":"basic"}"#).unwrap();
        assert_eq!(info(&ctx, &Caller::Desktop).build.unwrap().profile, "basic");
    }

    #[tokio::test]
    async fn remotes_do_not_see_local_paths() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppCtx::for_tests(dir.path());
        assert!(!info(&ctx, &Caller::Desktop).data_dir.is_empty());
        let desktop = info(&ctx, &Caller::Desktop);
        assert!(
            desktop
                .locations
                .iter()
                .any(|p| p.label == "Database" && p.path.ends_with("nexus.db"))
        );
        assert!(
            desktop
                .locations
                .iter()
                .any(|p| p.label == "Managed models" && p.path == ctx.paths.models.to_string_lossy())
        );
        let remote = Caller::Remote {
            device_id: "d".into(),
            policy: SharePolicy::ChatRead,
        };
        let i = info(&ctx, &remote);
        assert!(i.data_dir.is_empty() && i.log_dir.is_empty());
        assert!(i.locations.is_empty());
        assert_eq!(i.version, env!("CARGO_PKG_VERSION"));
    }
}
