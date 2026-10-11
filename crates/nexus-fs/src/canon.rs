//! Path canonicalization for permission checks (PERMISSIONS §3 steps 1-2).
//!
//! Every decision is made on a *canonical* path: absolute, symlinks resolved, `..` gone. For a path that
//! does not exist yet (a file about to be created) the nearest existing ancestor is canonicalized and the
//! remaining components are appended, and any `..` in that remainder is rejected.

use std::path::{Component, Path, PathBuf};

/// A path that went through [`canonicalize`]. The only way to build one, so engine APIs that take it
/// cannot be handed a raw string by accident.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CanonicalPath(PathBuf);

impl CanonicalPath {
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl AsRef<Path> for CanonicalPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// Why a path could not be made canonical.
#[derive(Debug, thiserror::Error)]
pub enum CanonError {
    #[error("path is not absolute: {0}")]
    NotAbsolute(PathBuf),
    #[error("path contains `..` after a component that does not exist: {0}")]
    ParentInMissingPart(PathBuf),
    #[error("path is empty")]
    Empty,
    #[error("broken symbolic link: {0}")]
    BrokenLink(PathBuf),
    #[error("cannot resolve {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn canon_existing(p: &Path) -> std::io::Result<PathBuf> {
    // dunce strips the `\\?\` prefix on Windows and is plain `std::fs::canonicalize` elsewhere.
    dunce::canonicalize(p)
}

/// Make `path` canonical. It must be absolute.
pub fn canonicalize(path: &Path) -> Result<CanonicalPath, CanonError> {
    if path.as_os_str().is_empty() {
        return Err(CanonError::Empty);
    }
    if !path.is_absolute() {
        return Err(CanonError::NotAbsolute(path.to_owned()));
    }

    // Walk up to the longest prefix that exists (a symlink counts as existing, even a dangling one).
    let mut existing = path.to_owned();
    let mut remainder: Vec<std::ffi::OsString> = Vec::new();
    while std::fs::symlink_metadata(&existing).is_err() {
        match existing.file_name() {
            Some(name) => remainder.push(name.to_owned()),
            // `..` has no file_name: it can only be in a part that does not exist.
            None => {
                if matches!(existing.components().next_back(), Some(Component::ParentDir)) {
                    return Err(CanonError::ParentInMissingPart(path.to_owned()));
                }
                return Err(CanonError::Io {
                    path: path.to_owned(),
                    source: std::io::Error::from(std::io::ErrorKind::NotFound),
                });
            }
        }
        if !existing.pop() {
            return Err(CanonError::Io {
                path: path.to_owned(),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            });
        }
    }

    let mut canon = match canon_existing(&existing) {
        Ok(c) => c,
        // lstat worked but resolving failed: a dangling link or a loop.
        Err(_) if std::fs::symlink_metadata(&existing).is_ok() => {
            return Err(CanonError::BrokenLink(existing));
        }
        Err(source) => {
            return Err(CanonError::Io {
                path: existing,
                source,
            });
        }
    };
    for comp in remainder.into_iter().rev() {
        // After a missing component `..`, `.` and empty names are all unsafe.
        let c = Path::new(&comp);
        if c.components().any(|x| !matches!(x, Component::Normal(_))) || comp.is_empty() {
            return Err(CanonError::ParentInMissingPart(path.to_owned()));
        }
        canon.push(comp);
    }
    Ok(CanonicalPath(canon))
}

/// Component-wise "is `path` inside (or equal to) `root`". Never a string-prefix test:
/// `/a/bc` is not within `/a/b`. On Windows the comparison ignores case.
pub fn is_within(path: &Path, root: &Path) -> bool {
    #[cfg(windows)]
    {
        let norm = |p: &Path| -> Vec<String> {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
                .collect()
        };
        let (p, r) = (norm(path), norm(root));
        p.len() >= r.len() && p[..r.len()] == r[..]
    }
    #[cfg(not(windows))]
    {
        path.starts_with(root)
    }
}

/// Is `path` a direct child of `dir` (exactly one component below it)?
pub fn is_direct_child(path: &Path, dir: &Path) -> bool {
    path.parent().is_some_and(|p| same_path(p, dir))
}

/// Path equality, case-insensitive on Windows.
pub fn same_path(a: &Path, b: &Path) -> bool {
    is_within(a, b) && is_within(b, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(d.path()).unwrap();
        (d, root)
    }

    #[test]
    fn relative_and_empty_paths_are_rejected() {
        assert!(matches!(
            canonicalize(Path::new("a/b")),
            Err(CanonError::NotAbsolute(_))
        ));
        assert!(matches!(canonicalize(Path::new("")), Err(CanonError::Empty)));
        assert!(matches!(
            canonicalize(Path::new("./x")),
            Err(CanonError::NotAbsolute(_))
        ));
    }

    #[test]
    fn existing_paths_are_canonical_and_dots_are_resolved() {
        let (_d, root) = tmp();
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/b/f.txt"), "x").unwrap();
        let p = root.join("a/b/../b/./f.txt");
        assert_eq!(canonicalize(&p).unwrap().as_path(), root.join("a/b/f.txt"));
    }

    #[test]
    fn missing_leaf_under_an_existing_dir_is_appended() {
        let (_d, root) = tmp();
        std::fs::create_dir(root.join("a")).unwrap();
        let c = canonicalize(&root.join("a/new/deeper/file.txt")).unwrap();
        assert_eq!(c.as_path(), root.join("a/new/deeper/file.txt"));
    }

    #[test]
    fn parent_dir_in_the_missing_part_is_rejected() {
        let (_d, root) = tmp();
        std::fs::create_dir(root.join("a")).unwrap();
        for bad in [
            "a/missing/../../escape",
            "a/missing/..",
            "a/../missing/../x/../..",
        ] {
            // `a/../missing` resolves through existing `a/..` only up to the existing prefix.
            let r = canonicalize(&root.join(bad));
            if let Ok(c) = &r {
                // Anything that does resolve must still be inside the temp root (no escape via `..`).
                assert!(c.as_path().starts_with(&root), "{bad} -> {c:?}");
            }
        }
        assert!(matches!(
            canonicalize(&root.join("a/missing/../../escape")),
            Err(CanonError::ParentInMissingPart(_))
        ));
        assert!(matches!(
            canonicalize(&root.join("a/missing/..")),
            Err(CanonError::ParentInMissingPart(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_resolved_to_their_targets() {
        let (_d, root) = tmp();
        std::fs::create_dir_all(root.join("inside")).unwrap();
        std::fs::create_dir_all(root.join("outside")).unwrap();
        std::fs::write(root.join("outside/secret.txt"), "s").unwrap();
        std::os::unix::fs::symlink(root.join("outside"), root.join("inside/link")).unwrap();
        let c = canonicalize(&root.join("inside/link/secret.txt")).unwrap();
        assert_eq!(c.as_path(), root.join("outside/secret.txt"));
        // A not-yet-existing file below a symlinked dir is placed beyond the link, not under it.
        let n = canonicalize(&root.join("inside/link/new.txt")).unwrap();
        assert_eq!(n.as_path(), root.join("outside/new.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlinks_are_refused() {
        let (_d, root) = tmp();
        std::os::unix::fs::symlink(root.join("nowhere"), root.join("dangling")).unwrap();
        assert!(matches!(
            canonicalize(&root.join("dangling")),
            Err(CanonError::BrokenLink(_))
        ));
        assert!(matches!(
            canonicalize(&root.join("dangling/child")),
            Err(CanonError::BrokenLink(_))
        ));
    }

    #[test]
    fn within_is_component_wise() {
        assert!(is_within(Path::new("/a/b/c"), Path::new("/a/b")));
        assert!(is_within(Path::new("/a/b"), Path::new("/a/b")));
        assert!(
            !is_within(Path::new("/a/bc"), Path::new("/a/b")),
            "string prefix must not match"
        );
        assert!(!is_within(Path::new("/a"), Path::new("/a/b")));
        assert!(is_direct_child(Path::new("/a/b/c"), Path::new("/a/b")));
        assert!(!is_direct_child(Path::new("/a/b/c/d"), Path::new("/a/b")));
        assert!(!is_direct_child(Path::new("/a/b"), Path::new("/a/b")));
    }

    #[cfg(windows)]
    #[test]
    fn windows_comparison_ignores_case() {
        assert!(is_within(
            Path::new(r"C:\Users\Me\Docs"),
            Path::new(r"c:\users\me")
        ));
        assert!(same_path(Path::new(r"C:\A"), Path::new(r"c:\a")));
    }
}
