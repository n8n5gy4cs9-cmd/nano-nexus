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
    build_config_for(ctx, None)
}

/// Like [`build_config`], but `prefer` (a registered model id, e.g. Duo's pick) wins over the active model.
pub fn build_config_for(ctx: &AppCtx, prefer: Option<&str>) -> Option<RuntimeConfig> {
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
    // The developer override wins; then the model chosen in the Models page (or the bundle default, via
    // the registry); then whatever the bundle manifest or the dev vendor folder offers.
    let registered = search
        .env_override
        .is_none()
        .then(|| {
            prefer
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .or_else(|| super::models::active_model_id(ctx))
        })
        .flatten()
        .and_then(|id| ctx.store.get_model(&id).ok().flatten())
        .filter(|m| std::path::Path::new(&m.path).is_file())
        .map(|m| runtime_paths::ModelChoice {
            id: m.id,
            path: PathBuf::from(m.path),
        });
    let model = registered.or_else(|| runtime_paths::resolve_model(&search))?;
    let mut binary = runtime_paths::server_binary(&ctx.resource_dir, &ctx.exe_dir);
    if cfg!(debug_assertions) && !binary.is_file() {
        let vendor = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../vendor");
        if let Some(dev) = runtime_paths::vendor_server_binary(&vendor) {
            binary = dev;
        }
    }
    let mut cfg = RuntimeConfig::from_settings(&settings, binary, model.path);
    cfg.model_id = model.id;
    Some(cfg)
}

/// Duo (v0.1): the model id this message should run on, or `None` when the assistant is not Duo.
pub fn duo_model_for(
    ctx: &AppCtx,
    assistant_id: &str,
    text: &str,
    attachment_tokens: u32,
    think: bool,
) -> Option<String> {
    if assistant_id != "duo" {
        return None;
    }
    let s = ctx.settings_snapshot();
    let tier = nexus_agent::duo::classify(&nexus_agent::duo::DuoSignals {
        text,
        attachment_tokens,
        think,
    });
    let active = super::models::active_model_id(ctx).unwrap_or_default();
    let id = nexus_agent::duo::pick_model(tier, &s.duo_light_model_id, &s.duo_strong_model_id, &active);
    // Without two chosen models Duo behaves like the plain chat assistant (no swap, no status line).
    if id.is_empty() || (s.duo_light_model_id.trim().is_empty() && s.duo_strong_model_id.trim().is_empty()) {
        return None;
    }
    // Only switch to a model that is installed and still on disk; otherwise keep the running one.
    let installed = ctx
        .store
        .get_model(id)
        .ok()
        .flatten()
        .is_some_and(|m| std::path::Path::new(&m.path).is_file());
    installed.then(|| id.to_owned())
}

/// Make sure the server runs `model_id`, restarting it when another model is loaded. Best effort:
/// if the model cannot be found or started, the turn runs on whatever is loaded.
pub async fn ensure_model(ctx: &AppCtx, model_id: &str) {
    if let RuntimeState::Ready {
        model_id: current, ..
    } = ctx.supervisor.state()
        && current == model_id
    {
        return;
    }
    let Some(cfg) = build_config_for(ctx, Some(model_id)) else {
        return;
    };
    if cfg.model_id != model_id {
        return; // the id was not found; keep the running model
    }
    if let Err(e) = ctx.supervisor.restart(cfg).await {
        tracing::warn!("duo: could not switch model: {e}");
    }
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
