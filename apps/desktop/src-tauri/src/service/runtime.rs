use std::path::PathBuf;

use nexus_core::error::NexusError;
use nexus_core::events::RuntimeState;
use nexus_llm::RuntimeConfig;

use crate::runtime_paths::{self, ModelSearch};
use crate::state::AppCtx;

use super::Caller;

pub fn status(ctx: &AppCtx) -> RuntimeState {
    ctx.supervisor.state()
}

pub async fn logs(ctx: &AppCtx, lines: u32) -> Vec<String> {
    ctx.supervisor.log_tail(lines.min(500) as usize).await
}

/// A launch config from the current settings, or `None` when no model is installed (the UI then
/// shows "Download a model").
pub fn build_config(ctx: &AppCtx) -> Option<RuntimeConfig> {
    let settings = ctx.settings_snapshot();
    let search = ModelSearch {
        // Developer conveniences exist in debug builds only.
        env_override: cfg!(debug_assertions)
            .then(|| std::env::var_os("NEXUS_MODEL_PATH").map(PathBuf::from))
            .flatten(),
        resource_dir: ctx.resource_dir.clone(),
        active_model_id: settings.models_active_model_id.clone(),
        vendor_dir: cfg!(debug_assertions)
            .then(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../vendor")),
    };
    let model = runtime_paths::resolve_model(&search)?;
    let mut cfg = RuntimeConfig::from_settings(
        &settings,
        runtime_paths::server_binary(&ctx.resource_dir, &ctx.exe_dir),
        model.path,
    );
    cfg.model_id = model.id;
    Some(cfg)
}

/// Restart the server with the current settings. Desktop only.
pub async fn restart(ctx: &AppCtx, caller: &Caller) -> Result<(), NexusError> {
    if !caller.is_desktop() {
        return Err(NexusError::ForbiddenRemote);
    }
    let cfg = build_config(ctx).ok_or(NexusError::ModelMissing)?;
    ctx.supervisor.restart(cfg).await.map_err(NexusError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::error::ErrorCode;
    use nexus_core::settings::SharePolicy;

    #[tokio::test]
    async fn restart_without_a_model_is_model_missing_and_remotes_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppCtx::for_tests(dir.path());
        assert_eq!(status(&ctx), RuntimeState::Stopped);
        // NEXUS_MODEL_PATH is unset in tests and there is no bundle under the temp resource dir; the
        // repo's vendor/ folder may exist on a dev machine, so only assert on the remote refusal there.
        let remote = Caller::Remote {
            device_id: "d".into(),
            policy: SharePolicy::Full,
        };
        assert_eq!(
            restart(&ctx, &remote).await.unwrap_err().code(),
            ErrorCode::ForbiddenRemote
        );
        if build_config(&ctx).is_none() {
            assert_eq!(
                restart(&ctx, &Caller::Desktop).await.unwrap_err().code(),
                ErrorCode::ModelMissing
            );
        }
        assert!(logs(&ctx, 10).await.is_empty());
    }
}
