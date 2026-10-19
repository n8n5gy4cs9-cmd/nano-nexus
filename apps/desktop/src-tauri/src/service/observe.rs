//! Activity Observatory glue (ADR-017, M19). The logic lives in `nexus-observe`; this file reads the
//! settings, runs the poll loop, and turns alerts into notifications. Desktop only: the commands are
//! not on LAN Share's allow-list.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use nexus_core::error::NexusError;
use nexus_core::events::{NotificationAction, NotificationLevel};
use nexus_observe::{ActivitySnapshot, Hub, Obs, ObserveView, Switches, selfsrc};
use nexus_store::NewNotification;

use crate::state::{AppCtx, AppEvent};

/// Live part of the Activity page.
#[derive(Default)]
pub struct ObserveRuntime {
    hub: Hub,
    /// The Activity page is open: poll fast and push `observe:snapshot`.
    watching: AtomicBool,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn switches(ctx: &AppCtx) -> Switches {
    let s = ctx.settings_snapshot();
    Switches {
        nanonexus: s.observe_nanonexus,
        claude_code: s.observe_claude_code,
        codex: s.observe_codex,
        window_hours: s.observe_window_hours,
        alerts: s.observe_alerts,
    }
}

/// Something happened inside NanoNexus (a chat turn, a tool call, a job).
pub fn record(ctx: &AppCtx, obs: impl IntoIterator<Item = Obs>) {
    ctx.observe.hub.record(&switches(ctx), obs);
}

pub fn chat_started(ctx: &AppCtx, conversation: &str, assistant: &str, model: &str) {
    record(
        ctx,
        selfsrc::chat_started(conversation, assistant, model, now_ms()),
    );
}

pub fn chat_finished(ctx: &AppCtx, conversation: &str, tokens_prompt: u32, tokens_out: u32, ok: bool) {
    let at = now_ms();
    let tokens = nexus_observe::TokenTotals {
        input: u64::from(tokens_prompt),
        output: u64::from(tokens_out),
        ..Default::default()
    };
    record(
        ctx,
        [
            selfsrc::chat_tokens(conversation, tokens, at),
            selfsrc::chat_finished(conversation, ok, at),
        ],
    );
}

pub fn tool_started(ctx: &AppCtx, conversation: &str, call_id: &str, tool: &str) {
    record(
        ctx,
        [selfsrc::tool_started(
            conversation,
            call_id,
            tool,
            Vec::new(),
            now_ms(),
        )],
    );
}

pub fn tool_finished(ctx: &AppCtx, conversation: &str, call_id: &str, ok: bool) {
    record(ctx, [selfsrc::tool_finished(conversation, call_id, ok, now_ms())]);
}

/// Settings changed: switch sources on or off.
pub fn apply_settings(ctx: &AppCtx) {
    ctx.observe.hub.apply(&switches(ctx));
}

/// Read what is new and return the picture. Alerts become notifications.
pub fn poll(ctx: &AppCtx) -> ObserveView {
    let (view, alerts) = ctx.observe.hub.poll(&switches(ctx), now_ms());
    for a in alerts {
        let _ = ctx.bus.send(AppEvent::Notify(NewNotification {
            level: NotificationLevel::Warning,
            kind: "activity".into(),
            title: a.title,
            body: Some(a.body),
            action: Some(NotificationAction {
                label: "Open Activity".into(),
                route: "/activity".into(),
            }),
        }));
    }
    view
}

/// The latest picture (reads once if nothing was read yet).
pub fn view(ctx: &AppCtx) -> ObserveView {
    ctx.observe.hub.cached().unwrap_or_else(|| poll(ctx))
}

pub fn replay(ctx: &AppCtx, at_ms: i64) -> Option<ActivitySnapshot> {
    ctx.observe.hub.frame_at(&switches(ctx), at_ms)
}

pub fn set_demo(ctx: &AppCtx, on: bool) -> ObserveView {
    ctx.observe.hub.set_demo(&switches(ctx), on);
    poll(ctx)
}

/// The page opened or closed.
pub fn watch(ctx: &AppCtx, on: bool) -> Result<(), NexusError> {
    ctx.observe.watching.store(on, Ordering::Relaxed);
    Ok(())
}

/// Poll in the background: every 2 s while the page is open, every 10 s while something external is
/// switched on (so alerts work with the window closed), and not at all otherwise.
pub async fn run_loop(ctx: Arc<AppCtx>) {
    let mut every = tokio::time::interval(Duration::from_secs(2));
    let mut since_slow = 0u32;
    loop {
        every.tick().await;
        let watching = ctx.observe.watching.load(Ordering::Relaxed);
        since_slow += 1;
        let slow_due = since_slow >= 5 && ctx.observe.hub.external_active();
        if !watching && !slow_due {
            continue;
        }
        since_slow = 0;
        let c = ctx.clone();
        let Ok(view) = tokio::task::spawn_blocking(move || poll(&c)).await else {
            continue;
        };
        if watching {
            let _ = ctx.bus.send(AppEvent::Observe(Box::new(view)));
        }
    }
}
