//! OS integration that is optional and off by default: the tray / menu-bar icon (with the LAN Share
//! status) and "start when I log in". Both follow settings and can be changed while the app runs.

use nexus_core::settings::Settings;
use nexus_core::share::ShareStatus;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

const TRAY_ID: &str = "nanonexus";

/// The disabled status line of the tray menu.
pub fn share_label(status: &ShareStatus) -> String {
    if !status.running {
        "Sharing is off".to_owned()
    } else if status.devices == 1 {
        format!("Sharing on :{} · 1 device", status.port)
    } else {
        format!("Sharing on :{} · {} devices", status.port, status.devices)
    }
}

fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn build_menu(app: &AppHandle, status: &ShareStatus) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let open = MenuItemBuilder::with_id("open", "Open NanoNexus").build(app)?;
    let share = MenuItemBuilder::with_id("share", share_label(status))
        .enabled(false)
        .build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit NanoNexus").build(app)?;
    MenuBuilder::new(app)
        .items(&[&open, &share])
        .separator()
        .item(&quit)
        .build()
}

/// Create or remove the tray icon.
pub fn apply_tray(app: &AppHandle, enabled: bool, status: &ShareStatus) {
    if !enabled {
        let _ = app.remove_tray_by_id(TRAY_ID);
        return;
    }
    if app.tray_by_id(TRAY_ID).is_some() {
        update_share(app, status);
        return;
    }
    let result = (|| -> tauri::Result<()> {
        let mut builder = TrayIconBuilder::with_id(TRAY_ID)
            .menu(&build_menu(app, status)?)
            .tooltip("NanoNexus")
            .on_menu_event(|app, event| match event.id().as_ref() {
                "open" => show_main_window(app),
                "quit" => app.exit(0),
                _ => {}
            });
        if let Some(icon) = app.default_window_icon() {
            builder = builder.icon(icon.clone());
        }
        builder.build(app)?;
        Ok(())
    })();
    if let Err(e) = result {
        tracing::warn!("could not create the tray icon: {e}");
    }
}

/// Refresh the sharing line (called on every `share:status`).
pub fn update_share(app: &AppHandle, status: &ShareStatus) {
    if let Some(tray) = app.tray_by_id(TRAY_ID)
        && let Ok(menu) = build_menu(app, status)
    {
        let _ = tray.set_menu(Some(menu));
    }
}

/// Turn "start at login" on or off (a no-op when it already matches).
pub fn apply_autostart(app: &AppHandle, enabled: bool) {
    let launcher = app.autolaunch();
    match launcher.is_enabled() {
        Ok(now) if now == enabled => {}
        _ => {
            let r = if enabled {
                launcher.enable()
            } else {
                launcher.disable()
            };
            if let Err(e) = r {
                tracing::warn!("could not change start-at-login: {e}");
            }
        }
    }
}

/// Apply the settings that this module owns.
pub fn apply_settings(app: &AppHandle, settings: &Settings, status: &ShareStatus) {
    apply_tray(app, settings.app_show_tray_icon, status);
    apply_autostart(app, settings.app_launch_at_login);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(running: bool, devices: u32) -> ShareStatus {
        ShareStatus {
            running,
            port: 9889,
            urls: vec![],
            devices,
            https_fingerprint: None,
            error: None,
            suggested_port: None,
        }
    }

    #[test]
    fn the_sharing_line_names_the_port_and_devices() {
        assert_eq!(share_label(&status(false, 0)), "Sharing is off");
        assert_eq!(share_label(&status(true, 0)), "Sharing on :9889 · 0 devices");
        assert_eq!(share_label(&status(true, 1)), "Sharing on :9889 · 1 device");
        assert_eq!(share_label(&status(true, 3)), "Sharing on :9889 · 3 devices");
    }
}
