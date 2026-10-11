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
