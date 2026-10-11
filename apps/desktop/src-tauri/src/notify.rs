//! The `notify()` service: persist, push to the webview, and (when appropriate) show an OS
//! notification. Used by jobs, downloads, and the share server.
//!
//! macOS shows notifications reliably only from the bundled, signed app; `tauri dev` may show them
//! as the terminal or not at all.

use std::sync::Arc;

use nexus_core::error::NexusError;
use nexus_core::events::NotificationPayload;
use nexus_core::settings::Settings;
use nexus_store::NewNotification;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use tauri_specta::Event as _;

use crate::state::AppState;

/// Global event `notification:new`.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "notification:new")]
pub struct NotificationNew(pub NotificationPayload);

/// Should this notification also go to the OS? Only when the user wants it, the kind is enabled, and
/// the window is not focused (an in-app bell is enough while they are looking at the app).
pub fn wants_os_notification(settings: &Settings, kind: &str, window_focused: bool) -> bool {
    settings.notifications_os_when_unfocused && settings.notification_kind_enabled(kind) && !window_focused
}

/// Store, emit, and maybe show an OS notification.
pub fn notify(app: &AppHandle, input: NewNotification) -> Result<NotificationPayload, NexusError> {
    let state = app.state::<Arc<AppState>>();
    let payload = state.store.insert_notification(input)?;

    if let Err(e) = NotificationNew(payload.clone()).emit(app) {
        tracing::warn!("could not emit notification:new: {e}");
    }

    let focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    if wants_os_notification(&state.settings_snapshot(), &payload.kind, focused) {
        let mut builder = app.notification().builder().title(&payload.title);
        if let Some(body) = &payload.body {
            builder = builder.body(body);
        }
        if let Err(e) = builder.show() {
            tracing::warn!("OS notification failed: {e}");
        }
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_notification_rules() {
        let mut s = Settings::default();
        assert!(wants_os_notification(&s, "download", false));
        assert!(
            !wants_os_notification(&s, "download", true),
            "focused window: bell only"
        );
        s.notifications_os_when_unfocused = false;
        assert!(!wants_os_notification(&s, "download", false));
        s.notifications_os_when_unfocused = true;
        s.notifications_kinds.insert("download".into(), false);
        assert!(!wants_os_notification(&s, "download", false));
        assert!(wants_os_notification(&s, "job", false));
    }
}
