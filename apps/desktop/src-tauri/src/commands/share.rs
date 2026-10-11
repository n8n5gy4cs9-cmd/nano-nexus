use std::sync::Arc;

use nexus_core::error::IpcError;
use nexus_core::settings::SharePolicy;
use nexus_core::share::{ShareDevice, ShareStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use tauri::State;

use super::CmdResult;
use crate::service::settings::SettingValue;
use crate::service::share::PairingView;
use crate::service::{self};
use crate::state::AppState;

/// Global event `share:status`: sharing started, stopped, or the device list changed.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "share:status")]
pub struct ShareStatusChanged(pub ShareStatus);

type Ctx<'a> = State<'a, Arc<AppState>>;

#[tauri::command]
#[specta::specta]
pub async fn share_status(ctx: Ctx<'_>) -> CmdResult<ShareStatus> {
    Ok(service::share::status(&ctx))
}

/// Start sharing with the saved settings (`port_in_use` if the port is taken).
#[tauri::command]
#[specta::specta]
pub async fn share_start(ctx: Ctx<'_>) -> CmdResult<ShareStatus> {
    service::share::start(&ctx).await.map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn share_stop(ctx: Ctx<'_>) -> CmdResult<ShareStatus> {
    Ok(service::share::stop(&ctx).await)
}

/// The pill switch: saves `share.enabled` and starts or stops.
#[tauri::command]
#[specta::specta]
pub async fn share_set_enabled(ctx: Ctx<'_>, enabled: bool) -> CmdResult<ShareStatus> {
    service::share::set_enabled(&ctx, enabled).await.map_err(IpcError::from)
}

/// Change one `share.*` setting (restarts a running server when needed).
#[tauri::command]
#[specta::specta]
pub async fn share_settings_set(ctx: Ctx<'_>, key: String, value: SettingValue) -> CmdResult<ShareStatus> {
    service::share::settings_set(&ctx, &key, Value::from(value))
        .await
        .map_err(IpcError::from)
}

/// A new QR + PIN (the old one stops working).
#[tauri::command]
#[specta::specta]
pub async fn share_pairing_code(ctx: Ctx<'_>) -> CmdResult<PairingView> {
    service::share::pairing_code(&ctx).map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn share_devices_list(ctx: Ctx<'_>) -> CmdResult<Vec<ShareDevice>> {
    service::share::devices(&ctx).map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn share_device_update(
    ctx: Ctx<'_>,
    id: String,
    name: Option<String>,
    policy: Option<SharePolicy>,
) -> CmdResult<()> {
    service::share::device_update(&ctx, &id, name.as_deref(), policy).map_err(IpcError::from)
}

/// Revoke a device: its session ends immediately.
#[tauri::command]
#[specta::specta]
pub async fn share_device_revoke(ctx: Ctx<'_>, id: String) -> CmdResult<bool> {
    service::share::device_revoke(&ctx, &id).map_err(IpcError::from)
}
