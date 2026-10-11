use std::sync::Arc;

use nexus_core::chat::ChatEvent;
use nexus_core::error::IpcError;
use nexus_core::ids::MessageId;
use tauri::State;
use tauri::ipc::Channel;

use super::CmdResult;
use crate::service::chat::{ChatSendArgs, ChatSendResult};
use crate::service::{self, Caller, ChannelSink};
use crate::state::AppState;

/// Send a message; the answer streams over `on_event`.
#[tauri::command]
#[specta::specta]
pub async fn chat_send(
    ctx: State<'_, Arc<AppState>>,
    args: ChatSendArgs,
    on_event: Channel<ChatEvent>,
) -> CmdResult<ChatSendResult> {
    service::chat::send(&ctx, &Caller::Desktop, args, Arc::new(ChannelSink(on_event))).map_err(IpcError::from)
}

/// Stop a running answer (the partial text is kept).
#[tauri::command]
#[specta::specta]
pub async fn chat_stop(ctx: State<'_, Arc<AppState>>, assistant_message_id: MessageId) -> CmdResult<bool> {
    Ok(service::chat::stop(&ctx, &assistant_message_id))
}

/// Answer the same question again.
#[tauri::command]
#[specta::specta]
pub async fn chat_regenerate(
    ctx: State<'_, Arc<AppState>>,
    assistant_message_id: MessageId,
    think: bool,
    on_event: Channel<ChatEvent>,
) -> CmdResult<ChatSendResult> {
    service::chat::regenerate(
        &ctx,
        &Caller::Desktop,
        &assistant_message_id,
        think,
        Arc::new(ChannelSink(on_event)),
    )
    .map_err(IpcError::from)
}

/// Edit one of your messages and get a new answer (later messages are dropped).
#[tauri::command]
#[specta::specta]
pub async fn chat_edit_resend(
    ctx: State<'_, Arc<AppState>>,
    user_message_id: MessageId,
    text: String,
    think: bool,
    on_event: Channel<ChatEvent>,
) -> CmdResult<ChatSendResult> {
    service::chat::edit_resend(
        &ctx,
        &Caller::Desktop,
        &user_message_id,
        &text,
        think,
        Arc::new(ChannelSink(on_event)),
    )
    .map_err(IpcError::from)
}
