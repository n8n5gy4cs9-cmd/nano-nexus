//! The tauri-specta builder: one place that lists every command and event, shared by the app and
//! by the `export_bindings` test that regenerates `apps/desktop/src/bindings.ts`.

use specta_typescript::Typescript;
use tauri_specta::{Builder, collect_commands, collect_events};

use crate::commands::{
    analyzer, app, assistants, attachments, chat, conversations, editor, fs, models, notifications, permissions, runtime,
    settings, share,
};

/// All commands and events.
pub fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            app::app_info,
            app::open_external,
            app::app_diagnostics,
            attachments::attachments_read,
            attachments::attachments_pick,
            runtime::runtime_status,
            runtime::runtime_restart,
            runtime::runtime_logs,
            settings::settings_get,
            settings::settings_set,
            chat::chat_send,
            chat::chat_stop,
            chat::chat_regenerate,
            chat::chat_edit_resend,
            conversations::conversations_list,
            conversations::conversation_get,
            conversations::conversation_update,
            conversations::conversation_delete,
            conversations::conversation_export_md,
            notifications::notifications_list,
            notifications::notifications_unread_count,
            notifications::notifications_mark_read,
            notifications::notifications_clear,
            notifications::notifications_send_test,
            fs::fs_grants_list,
            fs::fs_grant_add,
            fs::fs_grant_update,
            fs::fs_grant_revoke,
            fs::fs_deny_list,
            fs::fs_deny_add,
            fs::fs_deny_remove,
            fs::fs_pick_folder,
            fs::fs_pick_any_file,
            fs::fs_list_dir,
            fs::fs_read_text,
            fs::fs_stat,
            fs::os_permissions_status,
            fs::os_open_privacy_settings,
            fs::audit_list,
            fs::changesets_list,
            fs::changeset_preview,
            fs::changeset_apply,
            fs::changeset_undo,
            fs::changeset_discard,
            fs::debug_make_test_changeset,
            permissions::permission_respond,
            permissions::permission_requests,
            assistants::assistants_list,
            assistants::assistant_get,
            assistants::assistant_validate,
            assistants::assistant_save,
            assistants::assistant_delete,
            assistants::assistant_duplicate,
            assistants::assistant_set_enabled,
            assistants::assistant_export,
            assistants::assistant_import,
            assistants::assistant_render_toml,
            assistants::assistant_draft,
            analyzer::analyze_folder,
            analyzer::analyze_cancel,
            analyzer::reports_list,
            analyzer::report_folder,
            analyzer::report_body,
            analyzer::report_delete,
            analyzer::report_narrative_save,
            analyzer::report_export,
            analyzer::file_report,
            analyzer::report_cleanup_plan,
            models::models_list,
            models::models_catalog,
            models::models_memory,
            models::models_search,
            models::models_repo_files,
            models::models_downloads,
            models::models_download_catalog,
            models::models_download_hf,
            models::models_download_pause,
            models::models_download_resume,
            models::models_download_cancel,
            models::models_activate,
            models::models_delete,
            models::models_pick_file,
            models::models_import,
            models::models_rescan,
            models::models_verify_bundled,
            editor::editor_can_write,
            editor::editor_save,
            editor::editor_ai,
            editor::editor_ai_cancel,
            share::share_status,
            share::share_start,
            share::share_stop,
            share::share_set_enabled,
            share::share_settings_set,
            share::share_pairing_code,
            share::share_devices_list,
            share::share_device_update,
            share::share_device_revoke,
        ])
        .events(collect_events![
            runtime::RuntimeStatus,
            crate::notify::NotificationNew,
            fs::ChangeSetUpdate,
            models::DownloadProgressEvent,
            models::ModelChanged,
            share::ShareStatusChanged
        ])
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
