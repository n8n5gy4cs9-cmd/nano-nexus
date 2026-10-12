#![allow(clippy::unwrap_used, clippy::expect_used)]
//! M14 audit (T14.01): Tauri commands are thin wrappers over `service::*` (M04 rule). Commands that
//! change state pass `Caller::Desktop`, and nothing in `commands/` touches the database, the permission
//! engine, or the file system directly. A new command that breaks the rule fails here.

use std::path::Path;

/// Commands that only drive the native shell (file dialogs, the OS privacy pane, a test notification).
const SHELL_ONLY: &[&str] = &[
    "fs_pick_folder",
    "fs_pick_any_file",
    "models_pick_file",
    "models_pick_folder",
    "os_open_privacy_settings",
    "notifications_send_test",
    "permission_requests",
];

/// Longest body of a command, in lines. Longer than this means logic that belongs in `service::*`.
const MAX_BODY_LINES: usize = 25;

struct Command {
    file: String,
    name: String,
    body: String,
}

fn commands() -> Vec<Command> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for chunk in text.split("#[tauri::command]").skip(1) {
            let sig = chunk.find("fn ").unwrap();
            let name: String = chunk[sig + 3..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            // The function ends at the first closing brace in column 0.
            let end = chunk.find("\n}\n").unwrap_or(chunk.len());
            out.push(Command {
                file: path.file_name().unwrap().to_string_lossy().into_owned(),
                name,
                body: chunk[sig..end].to_owned(),
            });
        }
    }
    assert!(out.len() > 100, "the scan found only {} commands", out.len());
    out
}

#[test]
fn every_command_delegates_to_a_service() {
    let bad: Vec<String> = commands()
        .iter()
        .filter(|c| !SHELL_ONLY.contains(&c.name.as_str()) && !c.body.contains("service::"))
        .map(|c| format!("{}::{}", c.file, c.name))
        .collect();
    assert!(bad.is_empty(), "commands without a service call: {bad:?}");
}

#[test]
fn commands_stay_thin() {
    let bad: Vec<String> = commands()
        .iter()
        .filter(|c| c.body.lines().count() > MAX_BODY_LINES)
        .map(|c| format!("{}::{} ({} lines)", c.file, c.name, c.body.lines().count()))
        .collect();
    assert!(bad.is_empty(), "move this logic into service::*: {bad:?}");
}

#[test]
fn commands_never_touch_storage_permissions_or_files_directly() {
    let forbidden = [
        ".store.",
        ".perms.",
        "std::fs::",
        "tokio::fs::",
        "rusqlite",
        "File::open",
    ];
    let bad: Vec<String> = commands()
        .iter()
        .flat_map(|c| {
            forbidden
                .iter()
                .filter(|f| c.body.contains(**f))
                .map(move |f| format!("{}::{} uses {f}", c.file, c.name))
        })
        .collect();
    assert!(bad.is_empty(), "{bad:?}");
}

#[test]
fn a_command_that_names_a_caller_names_the_desktop() {
    // Tauri commands are only reachable from the desktop webview; remote devices go through the
    // ShareApi allow-list in service/share.rs, which builds `Caller::Remote` itself.
    let bad: Vec<String> = commands()
        .iter()
        .filter(|c| c.body.contains("Caller::") && !c.body.contains("Caller::Desktop"))
        .map(|c| format!("{}::{}", c.file, c.name))
        .collect();
    assert!(bad.is_empty(), "{bad:?}");
    assert!(!commands().iter().any(|c| c.body.contains("Caller::Remote")));
}

#[test]
fn the_remote_allow_list_is_explicit_and_excludes_management_commands() {
    let text =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/service/share.rs")).unwrap();
    let start = text.find("impl ShareApi for AppShareApi").unwrap();
    let section = &text[start..];
    // Unknown commands are an error, never a fall-through to a handler.
    assert!(section.contains("unknown command"));
    for never in [
        "fs_grant_add",
        "fs_grant_update",
        "fs_grant_revoke",
        "fs_deny_add",
        "changeset_apply",
        "changeset_undo",
        "settings_set",
        "share_",
        "models_import",
        "models_delete",
        "models_pack_",
        "rename_",
        "editor_save",
        "kb_add",
        "watch_",
    ] {
        assert!(
            !section.contains(&format!("\"{never}")),
            "remote devices must not be offered {never}"
        );
    }
}
