use serde::Serialize;
use specta::Type;

use crate::state::AppCtx;

use super::Caller;

/// Static facts about this install.
#[derive(Clone, Debug, Serialize, Type)]
pub struct AppInfo {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub data_dir: String,
    pub log_dir: String,
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
    }
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
    async fn remotes_do_not_see_local_paths() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppCtx::for_tests(dir.path());
        assert!(!info(&ctx, &Caller::Desktop).data_dir.is_empty());
        let remote = Caller::Remote {
            device_id: "d".into(),
            policy: SharePolicy::ChatRead,
        };
        let i = info(&ctx, &remote);
        assert!(i.data_dir.is_empty() && i.log_dir.is_empty());
        assert_eq!(i.version, env!("CARGO_PKG_VERSION"));
    }
}
