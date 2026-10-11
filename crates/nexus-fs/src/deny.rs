//! The deny list (PERMISSIONS §3): locations that are never readable or writable, whatever the grants.
//! Built-in entries cannot be removed; users can add their own.

use std::path::{Path, PathBuf};

use crate::canon::{canonicalize, is_within};

/// One protected location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DenyRule {
    pub root: PathBuf,
    pub builtin: bool,
    /// Why it is protected (shown in the "Protected locations" list).
    pub label: String,
}

impl DenyRule {
    fn builtin(root: PathBuf, label: &str) -> Self {
        Self {
            root,
            builtin: true,
            label: label.to_owned(),
        }
    }

    pub fn user(root: PathBuf, label: impl Into<String>) -> Self {
        Self {
            root,
            builtin: false,
            label: label.into(),
        }
    }

    pub fn matches(&self, canonical: &Path) -> bool {
        is_within(canonical, &self.root)
    }
}

/// macOS entries.
pub fn macos_rules(home: &Path) -> Vec<DenyRule> {
    let mut v = Vec::new();
    for (rel, why) in [
        (".ssh", "SSH keys"),
        (".gnupg", "GPG keys"),
        (".aws", "AWS credentials"),
        (".config/gcloud", "Google Cloud credentials"),
        (".kube", "Kubernetes credentials"),
        (".docker/config.json", "Docker credentials"),
        (".netrc", "network passwords"),
        ("Library/Keychains", "Keychain"),
        ("Library/Cookies", "browser cookies"),
        ("Library/Mail", "Mail"),
        ("Library/Messages", "Messages"),
        ("Library/Application Support/com.apple.TCC", "privacy database"),
        ("Library/Safari", "Safari data"),
    ] {
        v.push(DenyRule::builtin(home.join(rel), why));
    }
    for (abs, why) in [
        ("/System", "system files"),
        ("/usr", "system files"),
        ("/bin", "system files"),
        ("/sbin", "system files"),
        ("/private/etc", "system configuration"),
        ("/private/var/db", "system databases"),
        ("/Library/Keychains", "system Keychain"),
    ] {
        v.push(DenyRule::builtin(PathBuf::from(abs), why));
    }
    v
}

/// Windows entries. `system_root` is `C:\Windows`; `program_files` are the Program Files folders.
pub fn windows_rules(
    home: &Path,
    appdata: &Path,
    system_root: &Path,
    program_files: &[PathBuf],
) -> Vec<DenyRule> {
    let mut v = vec![
        DenyRule::builtin(system_root.to_owned(), "Windows system files"),
        DenyRule::builtin(
            appdata.join("Microsoft").join("Credentials"),
            "Windows credentials",
        ),
        DenyRule::builtin(home.join(".ssh"), "SSH keys"),
        DenyRule::builtin(home.join(".gnupg"), "GPG keys"),
        DenyRule::builtin(home.join(".aws"), "AWS credentials"),
    ];
    for pf in program_files {
        v.push(DenyRule::builtin(pf.clone(), "installed programs"));
    }
    v
}

/// Linux entries.
pub fn linux_rules(home: &Path) -> Vec<DenyRule> {
    let mut v = Vec::new();
    for (abs, why) in [
        ("/etc", "system configuration"),
        ("/usr", "system files"),
        ("/bin", "system files"),
        ("/sbin", "system files"),
        ("/boot", "boot files"),
        ("/proc", "system information"),
        ("/sys", "system information"),
    ] {
        v.push(DenyRule::builtin(PathBuf::from(abs), why));
    }
    for (rel, why) in [
        (".ssh", "SSH keys"),
        (".gnupg", "GPG keys"),
        (".aws", "AWS credentials"),
        (".config/gcloud", "Google Cloud credentials"),
        (".kube", "Kubernetes credentials"),
    ] {
        v.push(DenyRule::builtin(home.join(rel), why));
    }
    v
}

/// The built-in list for the OS this binary runs on, plus the app's own data folder (only internal code
/// paths may touch the database and journal).
pub fn builtin_rules(home: &Path, app_data_dir: &Path) -> Vec<DenyRule> {
    let mut v = if cfg!(target_os = "macos") {
        macos_rules(home)
    } else if cfg!(windows) {
        let appdata = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Roaming"));
        let sysroot = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let pf: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)"]
            .iter()
            .filter_map(|k| std::env::var_os(k).map(PathBuf::from))
            .collect();
        windows_rules(home, &appdata, &sysroot, &pf)
    } else {
        linux_rules(home)
    };
    v.push(DenyRule::builtin(app_data_dir.to_owned(), "NanoNexus's own data"));
    v
}

/// A set of deny rules. Roots that exist are stored in canonical form (so `/etc` on macOS also protects
/// `/private/etc`), and the original spelling is kept as well.
#[derive(Clone, Debug, Default)]
pub struct DenyList {
    rules: Vec<DenyRule>,
}

impl DenyList {
    pub fn new(rules: impl IntoIterator<Item = DenyRule>) -> Self {
        let mut l = Self::default();
        for r in rules {
            l.insert(r);
        }
        l
    }

    fn insert(&mut self, rule: DenyRule) {
        if let Ok(c) = canonicalize(&rule.root)
            && c.as_path() != rule.root
        {
            self.rules.push(DenyRule {
                root: c.into_path_buf(),
                ..rule.clone()
            });
        }
        self.rules.push(rule);
    }

    /// Add a user entry (idempotent for the same path).
    pub fn add_user(&mut self, root: PathBuf, label: impl Into<String>) {
        if !self.rules.iter().any(|r| r.root == root) {
            self.insert(DenyRule::user(root, label));
        }
    }

    /// Remove a *user* entry. Built-ins are never removed. Returns whether something was removed.
    pub fn remove_user(&mut self, root: &Path) -> bool {
        let before = self.rules.len();
        self.rules
            .retain(|r| r.builtin || !crate::canon::same_path(&r.root, root));
        self.rules.len() != before
    }

    /// The first rule that covers `canonical`.
    pub fn find(&self, canonical: &Path) -> Option<&DenyRule> {
        self.rules.iter().find(|r| r.matches(canonical))
    }

    pub fn rules(&self) -> &[DenyRule] {
        &self.rules
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_os_lists_cover_the_documented_locations() {
        let home = Path::new("/Users/me");
        let mac = macos_rules(home);
        for p in [
            "/Users/me/.ssh",
            "/Users/me/Library/Keychains",
            "/System",
            "/private/etc",
            "/Users/me/.docker/config.json",
        ] {
            assert!(mac.iter().any(|r| r.root == Path::new(p)), "{p}");
        }
        let lin = linux_rules(Path::new("/home/me"));
        for p in ["/etc", "/proc", "/home/me/.ssh", "/home/me/.gnupg"] {
            assert!(lin.iter().any(|r| r.root == Path::new(p)), "{p}");
        }
        let win = windows_rules(
            Path::new(r"C:\Users\me"),
            Path::new(r"C:\Users\me\AppData\Roaming"),
            Path::new(r"C:\Windows"),
            &[
                PathBuf::from(r"C:\Program Files"),
                PathBuf::from(r"C:\Program Files (x86)"),
            ],
        );
        assert!(win.iter().any(|r| r.root == Path::new(r"C:\Program Files (x86)")));
        assert!(win.iter().any(|r| r.root.ends_with("Credentials")));
        assert!(win.iter().all(|r| r.builtin));
    }

    #[test]
    fn app_data_is_always_protected() {
        let rules = builtin_rules(
            Path::new("/home/me"),
            Path::new("/home/me/.local/share/dev.nanonexus.app"),
        );
        assert!(rules.iter().any(|r| r.root.ends_with("dev.nanonexus.app")));
    }

    #[test]
    fn matching_is_component_wise_and_includes_the_root_itself() {
        let l = DenyList::new([DenyRule::builtin(PathBuf::from("/home/me/.ssh"), "keys")]);
        assert!(l.find(Path::new("/home/me/.ssh")).is_some());
        assert!(l.find(Path::new("/home/me/.ssh/id_ed25519")).is_some());
        assert!(
            l.find(Path::new("/home/me/.sshd")).is_none(),
            "string prefix must not match"
        );
        assert!(l.find(Path::new("/home/me")).is_none());
    }

    #[test]
    fn user_entries_can_be_added_and_removed_but_builtins_cannot() {
        let mut l = DenyList::new([DenyRule::builtin(PathBuf::from("/etc"), "system")]);
        l.add_user(PathBuf::from("/data/private"), "my secrets");
        l.add_user(PathBuf::from("/data/private"), "again");
        assert_eq!(l.rules().iter().filter(|r| !r.builtin).count(), 1, "idempotent");
        assert!(l.find(Path::new("/data/private/x")).is_some());
        assert!(!l.remove_user(Path::new("/etc")), "built-ins stay");
        assert!(l.find(Path::new("/etc/passwd")).is_some());
        assert!(l.remove_user(Path::new("/data/private")));
        assert!(l.find(Path::new("/data/private/x")).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn existing_roots_are_also_stored_canonically() {
        // A symlinked deny root also protects what it points to.
        let d = tempfile::tempdir().unwrap();
        let real = dunce::canonicalize(d.path()).unwrap().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = d.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let l = DenyList::new([DenyRule::builtin(link, "via link")]);
        assert!(l.find(&real.join("file")).is_some());
    }
}
