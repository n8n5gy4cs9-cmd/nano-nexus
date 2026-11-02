//! Thin desktop-only commands for the image, speech and voice workspaces (UP.041-044).
use super::CmdResult;
use crate::{
    service::{
        self, Caller,
        media::{
            ImageGenerateInput, ImageStartInput, MediaStatus, OutputKind, TranscribeInput, TranscribeOutput,
            VoiceInfo,
        },
    },
    state::AppState,
};
use nexus_core::error::IpcError;
use nexus_models::media::SpeakRequest;
use nexus_studio::{outputs::OutputRecord, progress::TtsProgress};
use std::sync::Arc;
use tauri::{AppHandle, State, ipc::Channel};
use tauri_plugin_dialog::DialogExt;
type Ctx<'a> = State<'a, Arc<AppState>>;

#[tauri::command]
#[specta::specta]
pub async fn media_status(ctx: Ctx<'_>) -> CmdResult<MediaStatus> {
    service::media::status(&ctx, &Caller::Desktop)
        .await
        .map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn media_cancel(job_id: String) -> CmdResult<bool> {
    service::media::cancel(&Caller::Desktop, &job_id).map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn image_start(ctx: Ctx<'_>, input: ImageStartInput) -> CmdResult<String> {
    service::media::image_start(&ctx, &Caller::Desktop, input)
        .await
        .map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn image_stop() -> CmdResult<()> {
    service::media::image_stop(&Caller::Desktop)
        .await
        .map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn image_generate(
    ctx: Ctx<'_>,
    job_id: String,
    input: ImageGenerateInput,
) -> CmdResult<OutputRecord> {
    service::media::image_generate(&ctx, &Caller::Desktop, &job_id, input)
        .await
        .map_err(IpcError::from)
}
/// `progress`: percent 0-100 while whisper reports it, `null` while it cannot tell.
#[tauri::command]
#[specta::specta]
pub async fn speech_transcribe(
    ctx: Ctx<'_>,
    job_id: String,
    input: TranscribeInput,
    on_event: Channel<Option<u32>>,
) -> CmdResult<TranscribeOutput> {
    service::media::transcribe(
        &ctx,
        &Caller::Desktop,
        &job_id,
        input,
        Arc::new(move |p| {
            let _ = on_event.send(p);
        }),
    )
    .await
    .map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn speech_pick_audio(app: AppHandle) -> CmdResult<Option<String>> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("WAV audio", &["wav"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| IpcError::from(nexus_core::error::NexusError::Internal(e.to_string())))?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string()))
}
#[tauri::command]
#[specta::specta]
pub async fn tts_voices() -> CmdResult<Vec<VoiceInfo>> {
    Ok(service::media::voices())
}
#[tauri::command]
#[specta::specta]
pub async fn tts_speak(
    ctx: Ctx<'_>,
    job_id: String,
    request: SpeakRequest,
    on_event: Channel<TtsProgress>,
) -> CmdResult<OutputRecord> {
    service::media::speak(
        &ctx,
        &Caller::Desktop,
        &job_id,
        request,
        Arc::new(move |p| {
            let _ = on_event.send(p);
        }),
    )
    .await
    .map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn outputs_list(ctx: Ctx<'_>, kind: OutputKind) -> CmdResult<Vec<OutputRecord>> {
    service::media::list_outputs(&ctx, &Caller::Desktop, kind).map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn outputs_read(ctx: Ctx<'_>, kind: OutputKind, name: String) -> CmdResult<String> {
    service::media::read_output(&ctx, &Caller::Desktop, kind, &name).map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn outputs_delete(ctx: Ctx<'_>, kind: OutputKind, names: Vec<String>) -> CmdResult<usize> {
    service::media::delete_outputs(&ctx, &Caller::Desktop, kind, names).map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn hardware_telemetry() -> CmdResult<nexus_models::hardware::Telemetry> {
    service::media::telemetry(&Caller::Desktop).map_err(IpcError::from)
}
