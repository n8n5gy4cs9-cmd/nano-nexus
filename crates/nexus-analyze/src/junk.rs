//! Junk rules (ANALYZER §1): files and folders that are usually clutter. Regenerable folders are
//! size-reported but never marked safe to trash.

use serde::{Deserialize, Serialize};

const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct JunkHit {
    pub rule: String,
    pub rel_path: String,
    pub bytes: u64,
    /// Safe to move to the Trash without a second thought.
    pub safe_to_trash: bool,
    /// Rebuilt automatically by a tool (node_modules, target, ...): report the size, never auto-trash.
    pub regenerable: bool,
}

/// A file-level rule that matched.
pub struct FileJunk {
    pub rule: &'static str,
    pub safe_to_trash: bool,
}

/// Check one file. `stale` = older than 7 days (for partial downloads).
pub fn check_file(name: &str, bytes: u64, modified_ms: Option<i64>, now_ms: i64) -> Option<FileJunk> {
    let lower = name.to_ascii_lowercase();
    let stale = modified_ms.is_some_and(|m| now_ms - m > 7 * DAY_MS);
    let hit = |rule, safe| {
        Some(FileJunk {
            rule,
            safe_to_trash: safe,
        })
    };
    if lower == ".ds_store" {
        return hit(".DS_Store", true);
    }
    if lower == "thumbs.db" {
        return hit("Thumbs.db", true);
    }
    if lower == "desktop.ini" {
        return hit("desktop.ini", true);
    }
    if lower.starts_with("~$") {
        return hit("Office lock file", true);
    }
    if lower.ends_with(".tmp") {
        return hit("*.tmp", true);
    }
    if lower.ends_with(".crdownload") {
        return hit("*.crdownload", stale);
    }
    if lower.ends_with(".part") {
        return hit("*.part", stale);
    }
    if lower.ends_with(".log") && bytes > 50 * 1024 * 1024 {
        return hit("*.log over 50 MB", false);
    }
    None
}

/// Folders a tool regenerates. Returns the rule name.
pub fn regenerable_dir(name: &str) -> Option<&'static str> {
    Some(match name {
        "node_modules" => "node_modules/",
        "target" => "target/ (Rust)",
        ".venv" | "venv" => ".venv/",
        "dist" => "dist/",
        "build" => "build/",
        "__pycache__" => "__pycache__/",
        ".gradle" => ".gradle/",
        "DerivedData" => "DerivedData/",
        ".next" => ".next/",
        ".cache" => ".cache/",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    #[test]
    fn simple_junk_files() {
        for n in [".DS_Store", "Thumbs.db", "desktop.ini", "~$report.docx", "x.TMP"] {
            let j = check_file(n, 10, Some(NOW), NOW).unwrap_or_else(|| panic!("{n}"));
            assert!(j.safe_to_trash, "{n}");
        }
        assert!(check_file("report.docx", 10, Some(NOW), NOW).is_none());
    }

    #[test]
    fn partial_downloads_are_safe_only_when_stale() {
        let old = NOW - 8 * DAY_MS;
        assert!(
            !check_file("a.crdownload", 1, Some(NOW), NOW)
                .unwrap()
                .safe_to_trash
        );
        assert!(
            check_file("a.crdownload", 1, Some(old), NOW)
                .unwrap()
                .safe_to_trash
        );
        assert!(check_file("a.part", 1, Some(old), NOW).unwrap().safe_to_trash);
        assert!(!check_file("a.part", 1, None, NOW).unwrap().safe_to_trash);
    }

    #[test]
    fn big_logs_are_reported_not_trashed() {
        let j = check_file("server.log", 60 * 1024 * 1024, Some(NOW), NOW).unwrap();
        assert!(!j.safe_to_trash);
        assert!(check_file("server.log", 1024, Some(NOW), NOW).is_none());
    }

    #[test]
    fn regenerable_folders() {
        assert_eq!(regenerable_dir("node_modules"), Some("node_modules/"));
        assert!(regenerable_dir("src").is_none());
    }
}
