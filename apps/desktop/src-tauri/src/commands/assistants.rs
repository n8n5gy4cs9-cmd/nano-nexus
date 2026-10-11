use std::sync::Arc;

use nexus_agent::registry::ValidationReport;
use nexus_agent::spec::AssistantSpec;
use nexus_core::error::IpcError;
use tauri::State;

use super::CmdResult;
use crate::service::assistants::{AssistantCard, AssistantDetail};
use crate::service::{self};
use crate::state::AppState;

type Ctx<'a> = State<'a, Arc<AppState>>;

#[tauri::command]
#[specta::specta]
pub async fn assistants_list(ctx: Ctx<'_>) -> CmdResult<Vec<AssistantCard>> {
    service::assistants::list(&ctx).map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn assistant_get(ctx: Ctx<'_>, id: String) -> CmdResult<AssistantDetail> {
    service::assistants::get(&ctx, &id).map_err(IpcError::from)
}

/// Live builder validation: field errors and the prompt's token count.
#[tauri::command]
#[specta::specta]
pub async fn assistant_validate(toml: String) -> CmdResult<ValidationReport> {
    Ok(service::assistants::validate(&toml))
}

/// Create or update a user assistant (`previous_id` = the id being edited).
#[tauri::command]
#[specta::specta]
pub async fn assistant_save(
    ctx: Ctx<'_>,
    toml: String,
    previous_id: Option<String>,
) -> CmdResult<AssistantDetail> {
    service::assistants::save(&ctx, &toml, previous_id.as_deref()).map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn assistant_delete(ctx: Ctx<'_>, id: String) -> CmdResult<()> {
    service::assistants::delete(&ctx, &id).map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn assistant_duplicate(
    ctx: Ctx<'_>,
    id: String,
    name: Option<String>,
) -> CmdResult<AssistantDetail> {
    service::assistants::duplicate(&ctx, &id, name.as_deref()).map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn assistant_set_enabled(ctx: Ctx<'_>, id: String, enabled: bool) -> CmdResult<AssistantCard> {
    service::assistants::set_enabled(&ctx, &id, enabled).map_err(IpcError::from)
}

/// The `.toml` text for export.
#[tauri::command]
#[specta::specta]
pub async fn assistant_export(ctx: Ctx<'_>, id: String) -> CmdResult<String> {
    service::assistants::export(&ctx, &id).map_err(IpcError::from)
}

/// Import `.toml` text (ids already in use get a numeric suffix).
#[tauri::command]
#[specta::specta]
pub async fn assistant_import(ctx: Ctx<'_>, toml: String) -> CmdResult<AssistantDetail> {
    service::assistants::import(&ctx, &toml).map_err(IpcError::from)
}

/// Serialize a spec edited in the builder form to TOML.
#[tauri::command]
#[specta::specta]
pub async fn assistant_render_toml(spec: AssistantSpec) -> CmdResult<String> {
    service::assistants::render_toml(&spec).map_err(IpcError::from)
}
