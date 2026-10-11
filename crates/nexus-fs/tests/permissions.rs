//! PERMISSIONS §8: grant matching, symlinks, missing targets, deny precedence, expiry, remote caps.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use nexus_core::fs::{Access, GrantLevel, GrantScope};
use nexus_fs::{DenyList, DenyRule, FsError, MemoryAudit, NewGrant, PermissionEngine};

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    audit: Arc<MemoryAudit>,
    clock: Arc<AtomicI64>,
    engine: PermissionEngine,
}

/// A fake home with `.ssh`, `Downloads`, `Projects/game`, and a protected app-data folder.
fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    for d in [".ssh", "Downloads", "Projects/game", "appdata", "outside"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    std::fs::write(root.join(".ssh/id_ed25519"), "PRIVATE").unwrap();
    std::fs::write(root.join("Downloads/a.txt"), "a").unwrap();
    std::fs::write(root.join("outside/secret.txt"), "s").unwrap();
    let deny = DenyList::new([
        DenyRule::user(root.join(".ssh"), "SSH keys"),
        DenyRule::user(root.join("appdata"), "app data"),
    ]);
    let audit = Arc::new(MemoryAudit::default());
    let clock = Arc::new(AtomicI64::new(1_000));
    let c = clock.clone();
    let engine = PermissionEngine::new(deny, audit.clone()).with_clock(move || c.load(Ordering::SeqCst));
    Env {
        _dir: dir,
        root,
        audit,
        clock,
        engine,
    }
}

fn grant(e: &Env, rel: &str, level: GrantLevel) -> nexus_core::fs::Grant {
    e.engine
        .add_grant(NewGrant {
            root: e.root.join(rel),
            level,
            recursive: true,
            scope: GrantScope::Persistent,
            expires_at: None,
            label: None,
        })
        .unwrap()
}

fn ok(e: &Env, rel: &str, access: Access) -> bool {
    e.engine.check("t", &e.root.join(rel), access).is_ok()
}

#[test]
fn no_grants_means_no_access() {
    let e = env();
    assert!(matches!(
        e.engine.check("t", &e.root.join("Downloads/a.txt"), Access::Read),
        Err(FsError::NoGrant { needed: "read", .. })
    ));
}

#[test]
fn level_decides_which_access_is_allowed() {
    let e = env();
    grant(&e, "Downloads", GrantLevel::Read);
    assert!(ok(&e, "Downloads/a.txt", Access::Read));
    for a in [Access::Write, Access::Create, Access::Move, Access::Trash] {
        assert!(!ok(&e, "Downloads/a.txt", a), "{a:?} must need more than Read");
    }
    grant(&e, "Downloads", GrantLevel::ReadWrite); // replaces the Read grant
    assert!(ok(&e, "Downloads/a.txt", Access::Write));
    assert!(!ok(&e, "Downloads/new.txt", Access::Create));
    grant(&e, "Downloads", GrantLevel::Full);
    for a in [
        Access::Read,
        Access::Write,
        Access::Create,
        Access::Move,
        Access::Trash,
    ] {
        assert!(ok(&e, "Downloads/new.txt", a), "{a:?}");
    }
    assert_eq!(e.engine.grants().len(), 1, "one grant per root");
}

#[test]
fn component_boundaries_not_string_prefixes() {
    let e = env();
    std::fs::create_dir_all(e.root.join("Downloads-old")).unwrap();
    std::fs::write(e.root.join("Downloads-old/x.txt"), "x").unwrap();
    grant(&e, "Downloads", GrantLevel::Read);
    assert!(
        !ok(&e, "Downloads-old/x.txt", Access::Read),
        "/…/Downloads-old is not inside /…/Downloads"
    );
    assert!(ok(&e, "Downloads", Access::Read), "the root itself matches");
}

#[test]
fn non_recursive_grants_only_cover_direct_children() {
    let e = env();
    std::fs::create_dir_all(e.root.join("Downloads/sub")).unwrap();
    std::fs::write(e.root.join("Downloads/sub/deep.txt"), "d").unwrap();
    e.engine
        .add_grant(NewGrant {
            root: e.root.join("Downloads"),
            level: GrantLevel::Read,
            recursive: false,
            scope: GrantScope::Persistent,
            expires_at: None,
            label: None,
        })
        .unwrap();
    assert!(ok(&e, "Downloads", Access::Read));
    assert!(ok(&e, "Downloads/a.txt", Access::Read));
    assert!(ok(&e, "Downloads/sub", Access::Read));
    assert!(!ok(&e, "Downloads/sub/deep.txt", Access::Read));
}

#[test]
fn the_highest_matching_level_wins() {
    let e = env();
    grant(&e, "Projects", GrantLevel::Read);
    grant(&e, "Projects/game", GrantLevel::ReadWrite);
    std::fs::write(e.root.join("Projects/game/main.gd"), "x").unwrap();
    assert!(ok(&e, "Projects/game/main.gd", Access::Write));
    assert!(!ok(&e, "Projects/other.txt", Access::Write));
}

#[test]
fn expired_grants_stop_matching() {
    let e = env();
    e.engine
        .add_grant(NewGrant {
            root: e.root.join("Downloads"),
            level: GrantLevel::Read,
            recursive: true,
            scope: GrantScope::Session,
            expires_at: Some(5_000),
            label: None,
        })
        .unwrap();
    assert!(ok(&e, "Downloads/a.txt", Access::Read));
    e.clock.store(4_999, Ordering::SeqCst);
    assert!(ok(&e, "Downloads/a.txt", Access::Read));
    e.clock.store(5_000, Ordering::SeqCst);
    assert!(!ok(&e, "Downloads/a.txt", Access::Read));
}

#[test]
fn deny_list_beats_a_broader_grant() {
    let e = env();
    grant(&e, "", GrantLevel::Full); // the whole fake home
    assert!(ok(&e, "Downloads/a.txt", Access::Write));
    let err = e
        .engine
        .check("t", &e.root.join(".ssh/id_ed25519"), Access::Read)
        .unwrap_err();
    assert!(matches!(err, FsError::Denied { .. }));
    assert!(!ok(&e, ".ssh", Access::Read), "the denied folder itself too");
    assert!(
        !ok(&e, "appdata/nexus.db", Access::Read),
        "even for paths that do not exist yet"
    );
    assert!(!ok(&e, ".ssh/new_key", Access::Create));
}

#[test]
fn a_grant_cannot_be_created_on_a_protected_folder() {
    let e = env();
    let r = e.engine.add_grant(NewGrant {
        root: e.root.join(".ssh"),
        level: GrantLevel::Read,
        recursive: true,
        scope: GrantScope::Persistent,
        expires_at: None,
        label: None,
    });
    assert!(matches!(r, Err(FsError::Denied { .. })));
    let r = e.engine.add_grant(NewGrant {
        root: e.root.join(".ssh/id_ed25519"),
        level: GrantLevel::Read,
        recursive: false,
        scope: GrantScope::Session,
        expires_at: None,
        label: None,
    });
    assert!(matches!(r, Err(FsError::Denied { .. })));
}

#[cfg(unix)]
#[test]
fn a_symlink_that_escapes_the_grant_is_denied_and_one_that_stays_inside_is_fine() {
    let e = env();
    grant(&e, "Downloads", GrantLevel::Read);
    std::os::unix::fs::symlink(e.root.join("outside"), e.root.join("Downloads/escape")).unwrap();
    std::os::unix::fs::symlink(e.root.join("Downloads/a.txt"), e.root.join("Downloads/alias.txt")).unwrap();
    assert!(
        !ok(&e, "Downloads/escape/secret.txt", Access::Read),
        "link points outside the grant"
    );
    assert!(ok(&e, "Downloads/alias.txt", Access::Read), "link stays inside");
    // A link into a protected folder is denied by the deny list, not merely ungranted.
    grant(&e, "", GrantLevel::Read);
    std::os::unix::fs::symlink(e.root.join(".ssh"), e.root.join("Downloads/keys")).unwrap();
    assert!(matches!(
        e.engine
            .check("t", &e.root.join("Downloads/keys/id_ed25519"), Access::Read),
        Err(FsError::Denied { .. })
    ));
}

#[test]
fn missing_targets_use_the_nearest_existing_ancestor() {
    let e = env();
    grant(&e, "Downloads", GrantLevel::Full);
    assert!(ok(&e, "Downloads/brand/new/file.txt", Access::Create));
    assert!(
        !ok(&e, "Downloads/brand/../../outside/x.txt", Access::Create),
        "`..` after a missing part is rejected"
    );
    assert!(matches!(
        e.engine
            .check("t", &e.root.join("Downloads/brand/../../x"), Access::Create),
        Err(FsError::BadPath(_))
    ));
    assert!(matches!(
        e.engine.check("t", Path::new("relative/path"), Access::Read),
        Err(FsError::BadPath(_))
    ));
}

#[test]
fn remote_cap_limits_the_effective_level() {
    let e = env();
    grant(&e, "Downloads", GrantLevel::Full);
    let p = e.root.join("Downloads/a.txt");
    assert!(
        e.engine
            .check_capped("dev", &p, Access::Read, Some(GrantLevel::Read))
            .is_ok()
    );
    assert!(
        e.engine
            .check_capped("dev", &p, Access::Write, Some(GrantLevel::Read))
            .is_err()
    );
    assert!(e.engine.check_capped("dev", &p, Access::Write, None).is_ok());
}

#[test]
fn every_decision_is_audited_with_the_actor() {
    let e = env();
    grant(&e, "Downloads", GrantLevel::Read);
    let _ = e
        .engine
        .check("folder-scout", &e.root.join("Downloads/a.txt"), Access::Read);
    let _ = e
        .engine
        .check("folder-scout", &e.root.join("outside/secret.txt"), Access::Read);
    let _ = e
        .engine
        .check("editor", &e.root.join(".ssh/id_ed25519"), Access::Read);
    let entries = e.audit.entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(
        (
            entries[0].actor.as_str(),
            entries[0].allowed,
            entries[0].action.as_str()
        ),
        ("folder-scout", true, "read")
    );
    assert!(!entries[1].allowed && entries[1].detail.as_deref().unwrap().contains("grant"));
    assert!(!entries[2].allowed && entries[2].detail.as_deref().unwrap().contains("protected"));
    assert!(entries.iter().all(|x| x.ts == 1_000));
}

#[test]
fn user_deny_entries_apply_immediately() {
    let e = env();
    grant(&e, "Projects", GrantLevel::Read);
    assert!(ok(&e, "Projects/game", Access::Read));
    e.engine
        .add_user_deny(e.root.join("Projects/game"), "client work");
    assert!(!ok(&e, "Projects/game", Access::Read));
    assert!(e.engine.remove_user_deny(&e.root.join("Projects/game")));
    assert!(ok(&e, "Projects/game", Access::Read));
}

#[test]
fn removing_a_grant_removes_access() {
    let e = env();
    let g = grant(&e, "Downloads", GrantLevel::Read);
    assert!(ok(&e, "Downloads/a.txt", Access::Read));
    assert!(e.engine.remove_grant(&g.id));
    assert!(!ok(&e, "Downloads/a.txt", Access::Read));
    assert!(!e.engine.remove_grant(&g.id));
}

#[cfg(windows)]
#[test]
fn windows_paths_compare_case_insensitively() {
    let e = env();
    grant(&e, "Downloads", GrantLevel::Read);
    let upper = e.root.join("DOWNLOADS").join("a.txt");
    assert!(e.engine.check("t", &upper, Access::Read).is_ok());
}

mod properties {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// A sibling whose name merely starts with the granted folder's name is never covered.
        #[test]
        fn string_prefix_siblings_never_match(suffix in "[a-z0-9_-]{1,12}") {
            let e = env();
            grant(&e, "Downloads", GrantLevel::Full);
            let sibling = format!("Downloads{suffix}");
            std::fs::create_dir_all(e.root.join(&sibling)).unwrap();
            let rel = format!("{}/f.txt", sibling);
            prop_assert!(!ok(&e, &rel, Access::Read));
        }

        /// Whatever the level, anything inside a deny root is refused.
        #[test]
        fn deny_always_wins(name in "[a-z0-9_.-]{1,12}", level in 0u8..3) {
            let e = env();
            let level = [GrantLevel::Read, GrantLevel::ReadWrite, GrantLevel::Full][level as usize];
            grant(&e, "", level);
            for a in [Access::Read, Access::Write, Access::Create, Access::Move, Access::Trash] {
                let rel = format!(".ssh/{}", name);
                prop_assert!(!ok(&e, &rel, a));
            }
        }

        /// Plain names created under a Full grant resolve inside the grant.
        #[test]
        fn creates_stay_inside_the_grant(name in "[a-zA-Z0-9][a-zA-Z0-9_-]{0,15}") {
            let e = env();
            grant(&e, "Downloads", GrantLevel::Full);
            let c = e.engine.check("t", &e.root.join("Downloads").join(&name), Access::Create).unwrap();
            prop_assert!(c.as_path().starts_with(e.root.join("Downloads")));
        }
    }
}
