//! Coding-session scope rules (UP.084): project or projectless sessions, USB-local attachments and
//! per-host reconfirmation of project folders.
//!
//! Pure policy, no I/O. A project folder is host-bound: it is remembered with the host it was chosen on and
//! is trusted again only when the user re-grants it on the current host. A projectless session works only
//! on files attached into its own folder inside the DIST.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Largest accepted attachment file name.
pub const MAX_NAME_BYTES: usize = 120;

/// What a coding session may touch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scope {
    /// Only the session's own attachment folder.
    Projectless,
    /// A folder the user chose, bound to the host it was chosen on.
    Project {
        name: String,
        path: String,
        /// Opaque id of the computer where `path` was chosen.
        host_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ScopeError {
    #[error("a project folder must be an absolute path")]
    NotAbsolute,
    #[error("a project folder cannot be inside the application distribution")]
    InsideDistribution,
    #[error("the project folder contains `..`")]
    Traversal,
    #[error("the file name is empty, too long or contains path separators")]
    BadName,
    #[error("a session id may contain only letters, digits and dashes")]
    BadSessionId,
}

/// What the app must do before using a session's scope on this computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reconfirm {
    /// Nothing to ask.
    Ready,
    /// The project was chosen on another computer: ask the user to grant the folder again.
    GrantAgain,
    /// The folder is known on this host but no live grant exists this launch.
    GrantMissing,
}

/// Validate a project folder before it is remembered.
pub fn validate_project(path: &Path, dist_root: Option<&Path>) -> Result<(), ScopeError> {
    if !path.is_absolute() {
        return Err(ScopeError::NotAbsolute);
    }
    if path.components().any(|c| c == Component::ParentDir) {
        return Err(ScopeError::Traversal);
    }
    if dist_root.is_some_and(|root| path.starts_with(root)) {
        return Err(ScopeError::InsideDistribution);
    }
    Ok(())
}

/// Decide whether a stored scope is usable now. `granted` are the folders granted this launch.
pub fn reconfirm(scope: &Scope, current_host: &str, granted: &[PathBuf]) -> Reconfirm {
    match scope {
        Scope::Projectless => Reconfirm::Ready,
        Scope::Project { path, host_id, .. } => {
            if host_id != current_host {
                Reconfirm::GrantAgain
            } else if granted.iter().any(|g| Path::new(path).starts_with(g)) {
                Reconfirm::Ready
            } else {
                Reconfirm::GrantMissing
            }
        }
    }
}

/// `<data>/coding/<session>/attachments`: where a session's attached files live inside the DIST.
pub fn attachment_dir(data_dir: &Path, session_id: &str) -> Result<PathBuf, ScopeError> {
    let ok = !session_id.is_empty()
        && session_id.len() <= 64
        && session_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !ok {
        return Err(ScopeError::BadSessionId);
    }
    Ok(data_dir.join("coding").join(session_id).join("attachments"))
}

/// A file name that is safe to create inside an attachment folder.
pub fn safe_attachment_name(name: &str) -> Result<String, ScopeError> {
    let name = name.trim();
    let bad = name.is_empty()
        || name.len() > MAX_NAME_BYTES
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0', ':'])
        || name.chars().any(char::is_control);
    if bad {
        return Err(ScopeError::BadName);
    }
    Ok(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(host: &str) -> Scope {
        Scope::Project {
            name: "app".into(),
            path: "/work/app".into(),
            host_id: host.into(),
        }
    }

    #[test]
    fn projects_inside_the_distribution_or_with_traversal_are_refused() {
        let dist = Path::new("/Volumes/USB/DIST");
        assert_eq!(
            validate_project(Path::new("/Volumes/USB/Projects/a"), Some(dist)),
            Ok(())
        );
        assert_eq!(
            validate_project(Path::new("/Volumes/USB/DIST/apps"), Some(dist)),
            Err(ScopeError::InsideDistribution)
        );
        assert_eq!(
            validate_project(Path::new("rel/a"), None),
            Err(ScopeError::NotAbsolute)
        );
        assert_eq!(
            validate_project(Path::new("/a/../b"), None),
            Err(ScopeError::Traversal)
        );
    }

    #[test]
    fn another_host_or_missing_grant_needs_a_new_grant() {
        let granted = [PathBuf::from("/work")];
        assert_eq!(reconfirm(&Scope::Projectless, "h1", &[]), Reconfirm::Ready);
        assert_eq!(reconfirm(&project("h1"), "h1", &granted), Reconfirm::Ready);
        assert_eq!(reconfirm(&project("h1"), "h2", &granted), Reconfirm::GrantAgain);
        assert_eq!(reconfirm(&project("h1"), "h1", &[]), Reconfirm::GrantMissing);
    }

    #[test]
    fn attachment_paths_cannot_escape() {
        let dir = attachment_dir(Path::new("/dist/data"), "abc-1").unwrap();
        assert!(dir.ends_with("coding/abc-1/attachments"));
        assert_eq!(
            attachment_dir(Path::new("/d"), "../x"),
            Err(ScopeError::BadSessionId)
        );
        assert_eq!(safe_attachment_name("notes.md").as_deref(), Ok("notes.md"));
        for bad in ["", "..", "a/b", "a\\b", "c:x", "a\nb"] {
            assert_eq!(safe_attachment_name(bad), Err(ScopeError::BadName), "{bad:?}");
        }
    }
}
