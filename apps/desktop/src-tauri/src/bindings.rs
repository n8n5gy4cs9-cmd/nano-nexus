//! The tauri-specta builder: one place that lists every command and event, shared by the app and
//! by the `export_bindings` test that regenerates `apps/desktop/src/bindings.ts`.

use specta_typescript::Typescript;
use tauri_specta::{Builder, collect_commands, collect_events};

use crate::commands::{app, runtime, settings};

/// All commands and events.
pub fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            app::app_info,
            runtime::runtime_status,
            runtime::runtime_restart,
            runtime::runtime_logs,
            settings::settings_get,
            settings::settings_set,
        ])
        .events(collect_events![runtime::RuntimeStatus])
        // Timestamps (ms) and byte counts stay far below 2^53, so numbers are exact.
        .dangerously_cast_bigints_to_number()
}

/// Write `src/bindings.ts` (generated: never edit by hand).
pub fn export(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    builder().export(Typescript::default(), path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    /// `cargo test -p nano-nexus export_bindings` regenerates the file; the assertions keep the
    /// committed copy honest.
    #[test]
    fn export_bindings() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/bindings.ts");
        super::export(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        for needle in [
            "appInfo",
            "runtimeStatus",
            "settingsSet",
            "runtime:status",
            "RuntimeState",
        ] {
            assert!(text.contains(needle), "bindings.ts is missing `{needle}`");
        }
    }
}
