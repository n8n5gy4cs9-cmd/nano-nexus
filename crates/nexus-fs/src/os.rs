//! OS consent layer (PERMISSIONS §6): detect macOS TCC and Windows Controlled Folder Access denials and
//! describe them so the UI can explain and link to the right settings pane.
//!
//! Manual macOS test (cannot run in CI): System Settings > Privacy & Security > Files and Folders >
//! switch NanoNexus off for Documents, then call `probe_folder` on `~/Documents` -> `Denied`. The deep
//! link from `privacy_deep_link(PrivacyPane::FilesAndFolders)` must open that pane.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use nexus_core::error::NexusError;

/// Result of probing one protected location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsAccess {
    Granted,
    /// The OS refused (macOS TCC, Controlled Folder Access).
    Denied,
    /// The folder does not exist here.
    Missing,
    /// POSIX permissions (not an OS privacy prompt) or another error.
    Other,
}

/// Which System Settings pane to open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivacyPane {
    FilesAndFolders,
    FullDiskAccess,
}

/// The `x-apple.systempreferences:` link for a pane (macOS).
pub fn privacy_deep_link(pane: PrivacyPane) -> &'static str {
    match pane {
        PrivacyPane::FilesAndFolders => "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders",
        PrivacyPane::FullDiskAccess => "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles",
    }
}

/// Folders macOS protects with TCC (relative to home).
pub const TCC_FOLDERS: &[&str] = &["Desktop", "Documents", "Downloads"];

/// Does `path` live under a TCC-protected root of `home`?
pub fn is_tcc_protected(path: &Path, home: &Path) -> bool {
    TCC_FOLDERS.iter().any(|f| path.starts_with(home.join(f)))
        || path.starts_with(home.join("Library/Mobile Documents"))
        || path.starts_with("/Volumes")
}

/// Classify an I/O error from touching `path`.
///
/// macOS: `EPERM` (raw 1) on a TCC-protected path is the OS refusing; `EACCES` (13) is POSIX permissions.
/// Windows: `ERROR_ACCESS_DENIED` (5) on a *write* while reads work is Controlled Folder Access.
pub fn classify_error(e: &std::io::Error, path: &Path, home: &Path, writing: bool) -> OsAccess {
    match e.kind() {
        ErrorKind::NotFound => OsAccess::Missing,
        ErrorKind::PermissionDenied => {
            let code = e.raw_os_error();
            if cfg!(target_os = "macos") && code == Some(1) && is_tcc_protected(path, home) {
                OsAccess::Denied
            } else if cfg!(windows) && code == Some(5) && writing {
                OsAccess::Denied
            } else {
                OsAccess::Other
            }
        }
        _ => OsAccess::Other,
    }
}

/// Probe by listing a directory. Only call on user action: on macOS the first access to a protected
/// folder shows the system prompt.
pub fn probe_folder(path: &Path, home: &Path) -> OsAccess {
    match std::fs::read_dir(path) {
        Ok(_) => OsAccess::Granted,
        Err(e) => classify_error(&e, path, home, false),
    }
}

/// Full Disk Access is detectable by reading the TCC database directory (readable only with FDA).
pub fn probe_full_disk_access(home: &Path) -> OsAccess {
    probe_folder(&home.join("Library/Application Support/com.apple.TCC"), home)
}

/// Turn an OS denial into the error the UI explains ("macOS blocked access to Documents").
pub fn explain(path: &Path, home: &Path) -> NexusError {
    let location = TCC_FOLDERS
        .iter()
        .find(|f| path.starts_with(home.join(f)))
        .map(|f| (*f).to_owned())
        .unwrap_or_else(|| path.display().to_string());
    NexusError::OsPermissionDenied(location)
}

/// Home-relative display of the TCC folders for the status cards.
pub fn tcc_folder_paths(home: &Path) -> Vec<PathBuf> {
    TCC_FOLDERS.iter().map(|f| home.join(f)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_links_are_the_documented_urls() {
        assert!(privacy_deep_link(PrivacyPane::FilesAndFolders).ends_with("Privacy_FilesAndFolders"));
        assert!(privacy_deep_link(PrivacyPane::FullDiskAccess).ends_with("Privacy_AllFiles"));
    }

    #[test]
    fn tcc_roots() {
        let home = Path::new("/Users/me");
        assert!(is_tcc_protected(Path::new("/Users/me/Documents/x/y.txt"), home));
        assert!(is_tcc_protected(Path::new("/Volumes/USB/a"), home));
        assert!(!is_tcc_protected(Path::new("/Users/me/Projects"), home));
    }

    #[test]
    fn error_classification() {
        let home = Path::new("/Users/me");
        let nf = std::io::Error::from(ErrorKind::NotFound);
        assert_eq!(classify_error(&nf, Path::new("/x"), home, false), OsAccess::Missing);
        let eacces = std::io::Error::from_raw_os_error(13);
        assert_eq!(classify_error(&eacces, Path::new("/Users/me/Documents/a"), home, false), OsAccess::Other);
        let eperm = std::io::Error::from_raw_os_error(1);
        let r = classify_error(&eperm, Path::new("/Users/me/Documents/a"), home, false);
        assert_eq!(r, if cfg!(target_os = "macos") { OsAccess::Denied } else { OsAccess::Other });
    }

    #[test]
    fn explain_names_the_folder() {
        let e = explain(Path::new("/Users/me/Documents/a.txt"), Path::new("/Users/me"));
        assert!(matches!(e, NexusError::OsPermissionDenied(ref l) if l == "Documents"));
    }

    #[test]
    fn a_readable_folder_is_granted_and_a_missing_one_is_missing() {
        let d = std::env::temp_dir();
        assert_eq!(probe_folder(&d, Path::new("/nonexistent-home")), OsAccess::Granted);
        assert_eq!(probe_folder(&d.join("nn-definitely-missing-dir"), Path::new("/h")), OsAccess::Missing);
    }
}
