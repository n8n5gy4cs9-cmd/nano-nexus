//! LAN Share: the controller (start/stop/restart, pairing, devices) and the [`ShareApi`] the
//! server calls, which delegates to the other `service::*` modules with `Caller::Remote`.

use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use nexus_core::chat::ChatEvent;
use nexus_core::error::{ErrorCode, IpcError, NexusError};
use nexus_core::events::NotificationLevel;
use nexus_core::settings::{Settings, SharePolicy};
use nexus_core::share::{PairingCode, ShareDevice, ShareStatus};
use nexus_core::sink::Sink;
use nexus_share::server::{ServerConfig, ShareServer, next_free_port};
use nexus_share::urls::{pairing_url, qr_svg, share_urls};
use nexus_share::{Auth, AuthConfig, AuthEvent, Push, ShareApi};
use nexus_store::{NewNotification, Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use super::Caller;
use crate::state::{AppEvent, AppState, Ctx};

/// A pairing code plus what the desktop shows: the URL the QR encodes and the QR itself (SVG).
#[derive(Clone, Debug, Serialize, Type)]
pub struct PairingView {
    pub code: PairingCode,
    pub url: String,
    pub qr_svg: String,
}

/// Owns the server (if running), the auth state, and the global push channel.
pub struct ShareController {
    server: tokio::sync::Mutex<Option<ShareServer>>,
    pub auth: Arc<Auth>,
    push: broadcast::Sender<Push>,
    status: Mutex<ShareStatus>,
}

fn auth_config(s: &Settings) -> AuthConfig {
    AuthConfig {
        default_policy: s.share_default_policy,
        session_days: s.share_session_days,
        max_devices: s.share_max_devices,
    }
}

impl ShareController {
    pub fn new(store: Arc<Store>, settings: &Settings) -> Self {
        Self {
            server: tokio::sync::Mutex::new(None),
            auth: Auth::new(store, auth_config(settings)),
            push: broadcast::channel(64).0,
            status: Mutex::new(ShareStatus {
                port: settings.share_port,
                ..ShareStatus::default()
            }),
        }
    }

    fn set_status(&self, f: impl FnOnce(&mut ShareStatus)) -> ShareStatus {
        match self.status.lock() {
            Ok(mut s) => {
                f(&mut s);
                s.clone()
            }
            Err(_) => ShareStatus::default(),
        }
    }

    /// Last known status (the device count is refreshed on every read).
    pub fn status(&self) -> ShareStatus {
        let devices = self.auth.devices().map(|d| d.len() as u32).unwrap_or(0);
        self.set_status(|s| s.devices = devices)
    }
}

fn bind_ip(settings: &Settings) -> Result<IpAddr, NexusError> {
    settings
        .share_bind
        .parse()
        .map_err(|_| NexusError::InvalidInput(format!("`{}` is not a valid address to listen on", settings.share_bind)))
}

fn announce(ctx: &Ctx, status: &ShareStatus) {
    let _ = ctx.bus.send(AppEvent::ShareStatus(status.clone()));
}

/// Status for the pill and the settings page.
pub fn status(ctx: &AppState) -> ShareStatus {
    ctx.share.status()
}

/// Start sharing with the current settings. Errors with `port_in_use` (and suggests a free port).
pub async fn start(ctx: &Ctx) -> Result<ShareStatus, NexusError> {
    let settings = ctx.settings_snapshot();
    let bind = bind_ip(&settings)?;
    let mut guard = ctx.share.server.lock().await;
    if guard.is_some() {
        return Ok(ctx.share.status());
    }
    ctx.share.auth.set_config(auth_config(&settings));
    let config = ServerConfig {
        bind,
        port: settings.share_port,
        secure_cookie: settings.share_https,
        session_days: settings.share_session_days,
    };
    let api: Arc<dyn ShareApi> = Arc::new(AppShareApi { ctx: ctx.clone() });
    match ShareServer::start(config, ctx.share.auth.clone(), api).await {
        Ok(server) => {
            *guard = Some(server);
            drop(guard);
            ctx.share.auth.new_pairing_code()?;
            let urls = share_urls(bind, settings.share_port, settings.share_https, None);
            let st = ctx.share.set_status(|s| {
                s.running = true;
                s.port = settings.share_port;
                s.urls = urls;
                s.error = None;
                s.suggested_port = None;
            });
            announce(ctx, &st);
            Ok(ctx.share.status())
        }
        Err(e) => {
            let suggested = if matches!(e, NexusError::PortInUse(_)) {
                next_free_port(bind, settings.share_port).await
            } else {
                None
            };
            let message = e.to_string();
            let st = ctx.share.set_status(|s| {
                s.running = false;
                s.error = Some(message);
                s.suggested_port = suggested;
            });
            announce(ctx, &st);
            Err(e)
        }
    }
}

/// Stop sharing: sockets close and the port is released.
pub async fn stop(ctx: &Ctx) -> ShareStatus {
    let server = ctx.share.server.lock().await.take();
    if let Some(server) = server {
        server.stop().await;
    }
    ctx.share.auth.clear_pairing();
    let st = ctx.share.set_status(|s| {
        s.running = false;
        s.urls.clear();
        s.error = None;
        s.suggested_port = None;
    });
    announce(ctx, &st);
    ctx.share.status()
}

/// Stop and start again (a setting that needs a new socket changed).
pub async fn restart(ctx: &Ctx) -> Result<ShareStatus, NexusError> {
    stop(ctx).await;
    start(ctx).await
}

/// Persist `share.enabled` (the pill switch) and start or stop to match.
pub async fn set_enabled(ctx: &Ctx, enabled: bool) -> Result<ShareStatus, NexusError> {
    persist(ctx, "share.enabled", Value::Bool(enabled))?;
    if enabled { start(ctx).await } else { Ok(stop(ctx).await) }
}

fn persist(ctx: &AppState, key: &str, value: Value) -> Result<Settings, NexusError> {
    let current = ctx.settings_snapshot();
    let next = ctx.store.set_setting(&current, key, value)?;
    if let Ok(mut guard) = ctx.settings.write() {
        *guard = next.clone();
    }
    Ok(next)
}

/// Change a `share.*` setting; running servers restart when the socket or its security changes.
pub async fn settings_set(ctx: &Ctx, key: &str, value: Value) -> Result<ShareStatus, NexusError> {
    if !key.starts_with("share.") || key == "share.enabled" {
        return Err(NexusError::InvalidInput("only sharing settings can be changed here".into()));
    }
    if key == "share.port" {
        let ok = value.as_u64().is_some_and(|p| (1024..=65535).contains(&p));
        if !ok {
            return Err(NexusError::InvalidInput("the port must be between 1024 and 65535".into()));
        }
    }
    let next = persist(ctx, key, value)?;
    ctx.share.auth.set_config(auth_config(&next));
    let needs_restart = matches!(key, "share.port" | "share.bind" | "share.https" | "share.session_days");
    if needs_restart && ctx.share.status().running {
        return restart(ctx).await;
    }
    let st = ctx.share.set_status(|s| {
        if !s.running {
            s.port = next.share_port;
        }
    });
    announce(ctx, &st);
    Ok(ctx.share.status())
}

/// A fresh pairing code (rotates the previous one) with its QR.
pub fn pairing_code(ctx: &AppState) -> Result<PairingView, NexusError> {
    let st = ctx.share.status();
    if !st.running {
        return Err(NexusError::Conflict("sharing is off".into()));
    }
    let code = ctx.share.auth.new_pairing_code()?;
    let base = st.urls.first().cloned().unwrap_or_default();
    let url = pairing_url(&base, &code.secret);
    let svg = qr_svg(&url).unwrap_or_default();
    Ok(PairingView { code, url, qr_svg: svg })
}

pub fn devices(ctx: &AppState) -> Result<Vec<ShareDevice>, NexusError> {
    ctx.share.auth.devices()
}

pub fn device_update(
    ctx: &AppState,
    id: &str,
    name: Option<&str>,
    policy: Option<SharePolicy>,
) -> Result<(), NexusError> {
    if let Some(n) = name {
        if n.trim().is_empty() || n.chars().count() > 60 {
            return Err(NexusError::InvalidInput("name a device with 1-60 characters".into()));
        }
    }
    Ok(ctx.store.update_share_device(id, name.map(str::trim), policy)?)
}

pub fn device_revoke(ctx: &Ctx, id: &str) -> Result<bool, NexusError> {
    let removed = ctx.share.auth.revoke(id)?;
    if removed {
        let st = ctx.share.status();
        announce(ctx, &st);
    }
    Ok(removed)
}

/// Autostart (when enabled) and the long-lived bridges: auth events → notifications, app events
/// and runtime status → pushes to subscribed devices. Call from inside the async runtime.
pub async fn init(ctx: Ctx) {
    let settings = ctx.settings_snapshot();
    {
        let ctx = ctx.clone();
        tokio::spawn(async move { bridge_auth(ctx).await });
    }
    {
        let ctx = ctx.clone();
        tokio::spawn(async move { bridge_pushes(ctx).await });
    }
    if settings.share_autostart {
        if let Err(e) = start(&ctx).await {
            tracing::warn!("LAN share did not autostart: {e}");
        }
    }
}

async fn bridge_auth(ctx: Ctx) {
    let mut rx = ctx.share.auth.subscribe();
    loop {
        let event = match rx.recv().await {
            Ok(e) => e,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        };
        let notify_pairing = ctx.settings_snapshot().share_notify_pairing;
        match event {
            AuthEvent::Paired(d) => {
                announce(&ctx, &ctx.share.status());
                if notify_pairing {
                    let _ = ctx.bus.send(AppEvent::Notify(NewNotification {
                        level: NotificationLevel::Info,
                        kind: "share".into(),
                        title: format!("{} paired", d.name),
                        body: Some("A new device can now use your assistants over the local network.".into()),
                        action: None,
                    }));
                }
            }
            AuthEvent::LockedOut(ip) => {
                let _ = ctx.bus.send(AppEvent::Notify(NewNotification {
                    level: NotificationLevel::Warning,
                    kind: "share".into(),
                    title: "Too many wrong pairing codes".into(),
                    body: Some(format!("{ip} is blocked for 10 minutes.")),
                    action: None,
                }));
            }
            AuthEvent::Revoked(_) => announce(&ctx, &ctx.share.status()),
        }
    }
}

async fn bridge_pushes(ctx: Ctx) {
    let mut app_rx = ctx.bus.subscribe();
    let mut runtime_rx = ctx.supervisor.subscribe();
    loop {
        tokio::select! {
            event = app_rx.recv() => match event {
                Ok(AppEvent::ConversationUpdated(id)) => {
                    let _ = ctx.share.push.send(Push { event: "conversation:updated".into(), data: json!(id) });
                }
                Ok(AppEvent::ModelChanged(id)) => {
                    let _ = ctx.share.push.send(Push { event: "model:changed".into(), data: json!(id) });
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            changed = runtime_rx.changed() => {
                if changed.is_err() { break; }
                let state = runtime_rx.borrow_and_update().clone();
                if let Ok(data) = serde_json::to_value(state) {
                    let _ = ctx.share.push.send(Push { event: "runtime:status".into(), data });
                }
            }
        }
    }
}

/// Adapts the `ChatEvent` sink the chat service wants to the server's JSON sink.
struct JsonSink(Arc<dyn Sink<Value>>);

impl Sink<ChatEvent> for JsonSink {
    fn send(&self, event: ChatEvent) -> bool {
        match serde_json::to_value(event) {
            Ok(v) => self.0.send(v),
            Err(_) => true,
        }
    }
}

/// The command surface for remote devices.
pub struct AppShareApi {
    pub ctx: Ctx,
}

fn bad(e: serde_json::Error) -> IpcError {
    IpcError {
        code: ErrorCode::InvalidInput,
        message: format!("bad arguments: {e}"),
        detail: None,
    }
}

fn args<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, IpcError> {
    serde_json::from_value(v).map_err(bad)
}

fn out<T: Serialize>(v: T) -> Result<Value, IpcError> {
    serde_json::to_value(v).map_err(|e| NexusError::Internal(e.to_string()).into())
}

fn ipc(e: NexusError) -> IpcError {
    e.into()
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MsgId {
    assistant_message_id: String,
}

#[derive(Deserialize)]
struct SendWrap {
    args: crate::service::chat::ChatSendArgs,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegenArgs {
    assistant_message_id: String,
    think: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditArgs {
    user_message_id: String,
    text: String,
    think: bool,
}

#[derive(Deserialize)]
struct ListArgs {
    query: Option<String>,
    limit: u32,
    before: Option<i64>,
}

#[derive(Deserialize)]
struct UpdateArgs {
    id: String,
    update: crate::service::conversations::ConversationUpdate,
}

#[derive(Deserialize)]
struct ReportsArgs {
    kind: Option<String>,
    limit: u32,
}

#[derive(Deserialize)]
struct Limit {
    limit: u32,
}

#[derive(Deserialize)]
struct Ids {
    ids: Vec<String>,
}

fn assistant_allowed(settings: &Settings, id: &str) -> bool {
    settings.share_allowed_assistants.iter().any(|a| a == "*" || a == id)
}

#[async_trait]
impl ShareApi for AppShareApi {
    async fn call(
        &self,
        device: &ShareDevice,
        cmd: &str,
        a: Value,
        sink: Arc<dyn Sink<Value>>,
        _cancel: CancellationToken,
    ) -> Result<Value, IpcError> {
        use crate::service as svc;
        use nexus_core::ids::{ConversationId, MessageId};

        let ctx = &self.ctx;
        let caller = Caller::Remote {
            device_id: device.id.clone(),
            policy: device.policy,
        };
        let settings = ctx.settings_snapshot();
        match cmd {
            "app_info" => out(svc::app::info(ctx, &caller)),
            "runtime_status" => out(svc::runtime::status(ctx)),
            "assistants_list" => {
                let cards: Vec<_> = svc::assistants::list(ctx)
                    .map_err(ipc)?
                    .into_iter()
                    .filter(|c| assistant_allowed(&settings, &c.id))
                    .collect();
                out(cards)
            }
            "assistant_get" => {
                let Id { id } = args(a)?;
                if !assistant_allowed(&settings, &id) {
                    return Err(ipc(NexusError::ForbiddenRemote));
                }
                out(svc::assistants::get(ctx, &id).map_err(ipc)?)
            }
            "chat_send" => {
                let SendWrap { args: send } = args(a)?;
                if !assistant_allowed(&settings, send.assistant_id.0.as_str()) || send.draft_spec.is_some() {
                    return Err(ipc(NexusError::ForbiddenRemote));
                }
                // Attachments from a phone come as text in the request, never as desktop file paths.
                let is_new = send.conversation_id.is_none();
                let result = svc::chat::send(ctx, &caller, send, Arc::new(JsonSink(sink))).map_err(ipc)?;
                if is_new {
                    let origin = format!("remote:{}", device.id);
                    if let Err(e) = ctx.store.set_conversation_origin(result.conversation_id.0.as_str(), &origin) {
                        tracing::warn!("could not tag the remote conversation: {e}");
                    }
                }
                out(result)
            }
            "chat_stop" => {
                let MsgId { assistant_message_id } = args(a)?;
                out(svc::chat::stop(ctx, &MessageId(assistant_message_id)))
            }
            "chat_regenerate" => {
                let r: RegenArgs = args(a)?;
                out(svc::chat::regenerate(ctx, &caller, &MessageId(r.assistant_message_id), r.think, Arc::new(JsonSink(sink))).map_err(ipc)?)
            }
            "chat_edit_resend" => {
                let e: EditArgs = args(a)?;
                out(svc::chat::edit_resend(ctx, &caller, &MessageId(e.user_message_id), &e.text, e.think, Arc::new(JsonSink(sink))).map_err(ipc)?)
            }
            "conversations_list" => {
                let l: ListArgs = args(a)?;
                out(svc::conversations::list(ctx, l.query.as_deref(), l.limit, l.before).map_err(ipc)?)
            }
            "conversation_get" => {
                let Id { id } = args(a)?;
                out(svc::conversations::get(ctx, &ConversationId(id)).map_err(ipc)?)
            }
            "conversation_update" => {
                let u: UpdateArgs = args(a)?;
                // A phone may retitle and pin; it cannot switch assistants or archive.
                let mut update = u.update;
                update.assistant_id = None;
                update.archived = None;
                svc::conversations::update(ctx, &ConversationId(u.id), update).map_err(ipc)?;
                Ok(Value::Null)
            }
            "conversation_export_md" => {
                let Id { id } = args(a)?;
                out(svc::conversations::export_markdown(ctx, &ConversationId(id)).map_err(ipc)?)
            }
            "notifications_list" => {
                let Limit { limit } = args(a)?;
                out(svc::notifications::list(ctx, limit).map_err(ipc)?)
            }
            "notifications_unread_count" => out(svc::notifications::unread_count(ctx).map_err(ipc)?),
            "notifications_mark_read" => {
                let Ids { ids } = args(a)?;
                svc::notifications::mark_read(ctx, &ids).map_err(ipc)?;
                Ok(Value::Null)
            }
            "reports_list" => {
                let r: ReportsArgs = args(a)?;
                out(svc::analyzer::reports_list(ctx, r.kind.as_deref(), r.limit).map_err(ipc)?)
            }
            "report_folder" => {
                let Id { id } = args(a)?;
                out(svc::analyzer::report_folder(ctx, &id).map_err(ipc)?)
            }
            "report_body" => {
                let Id { id } = args(a)?;
                out(svc::analyzer::report_body(ctx, &id).map_err(ipc)?)
            }
            "models_list" => out(svc::models::list_installed(ctx).map_err(ipc)?),
            "models_activate" => {
                let Id { id } = args(a)?;
                out(svc::models::activate(ctx, &caller, &id).await.map_err(ipc)?)
            }
            "fs_grants_list" => out(svc::fs::grants_list(ctx)),
            other => Err(IpcError {
                code: ErrorCode::NotFound,
                message: format!("unknown command `{other}`"),
                detail: None,
            }),
        }
    }

    fn subscribe(&self) -> broadcast::Receiver<Push> {
        self.ctx.share.push.subscribe()
    }
}
