#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Watch deltas and thresholds (T11.05).

use nexus_analyze::report::{DupGroup, FileRef};
use nexus_analyze::watch::*;
use nexus_analyze::{Category, FolderReport, ScanOptions, scan_folder};
use tokio_util::sync::CancellationToken;

const MB: u64 = 1024 * 1024;
const GB: u64 = 1024 * MB;

fn base() -> FolderReport {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
    let mut r = scan_folder(&ScanOptions::new(dir.path()), &|_| {}, &CancellationToken::new()).unwrap();
    r.root = "/home/u/Downloads".into();
    r.scanned_at = 1_000;
    r.totals.bytes = 10 * GB;
    r.totals.files = 100;
    r.largest_files.clear();
    r.duplicates.clear();
    r
}

fn file(rel: &str, bytes: u64) -> FileRef {
    FileRef {
        rel_path: rel.into(),
        bytes,
        modified: Some(5_000),
        category: Category::Other,
    }
}

#[test]
fn unchanged_folder_is_not_significant() {
    let a = base();
    let mut b = a.clone();
    b.scanned_at = 9_000;
    let d = diff_reports(&a, &b);
    assert_eq!(d.bytes_delta, 0);
    assert!(!is_significant(&d));
}

#[test]
fn size_thresholds() {
    let a = base();
    let mut b = a.clone();
    b.totals.bytes += 400 * MB; // 3.9 % of 10 GB, under both rules
    assert!(!is_significant(&diff_reports(&a, &b)));
    b.totals.bytes = a.totals.bytes + 500 * MB;
    assert!(is_significant(&diff_reports(&a, &b)), "500 MB");
    let mut small = a.clone();
    small.totals.bytes = 100 * MB;
    let mut grown = small.clone();
    grown.totals.bytes = 115 * MB; // +15 %
    assert!(is_significant(&diff_reports(&small, &grown)), "10 %");
    let mut tiny = a.clone();
    tiny.totals.bytes = MB;
    let mut tiny2 = tiny.clone();
    tiny2.totals.bytes = 3 * MB;
    assert!(
        !is_significant(&diff_reports(&tiny, &tiny2)),
        "tiny folders do not trigger the percentage rule"
    );
}

#[test]
fn shrinking_counts_too() {
    let a = base();
    let mut b = a.clone();
    b.totals.bytes -= GB;
    let d = diff_reports(&a, &b);
    assert_eq!(d.bytes_delta, -(GB as i64));
    assert!(is_significant(&d));
    assert!(notification_text(&d).0.contains("shrank by"));
}

#[test]
fn new_big_files_and_duplicates() {
    let mut a = base();
    a.largest_files = vec![file("old.iso", 3 * GB)];
    let mut b = a.clone();
    b.totals.bytes += 50 * MB;
    b.largest_files = vec![
        file("old.iso", 3 * GB),
        file("new.dmg", 2 * GB),
        file("small.bin", MB),
    ];
    let d = diff_reports(&a, &b);
    let names: Vec<_> = d.new_big_files.iter().map(|f| f.rel_path.as_str()).collect();
    assert_eq!(names, ["new.dmg", "small.bin"], "existing file is not new");
    assert!(is_significant(&d), "a new 2 GB file");
    let mut older = b.clone();
    older.largest_files[2].modified = Some(10);
    assert_eq!(
        diff_reports(&a, &older).new_big_files.len(),
        1,
        "unmodified since the last scan: not new"
    );

    let mut c = a.clone();
    c.duplicates.push(DupGroup {
        bytes_each: 10 * MB,
        hash: "h1".into(),
        paths: vec!["x".into(), "y".into(), "z".into()],
        confirmed: true,
    });
    let d2 = diff_reports(&a, &c);
    assert_eq!(d2.new_duplicate_groups, 1);
    assert_eq!(d2.new_duplicate_bytes, 20 * MB);
    assert!(is_significant(&d2));
    let mut same = c.clone();
    same.scanned_at = 2;
    assert_eq!(diff_reports(&c, &same).new_duplicate_groups, 0);
}

#[test]
fn text_mentions_folder_and_files() {
    let a = base();
    let mut b = a.clone();
    b.totals.bytes += 3 * GB;
    b.largest_files = vec![file("a.dmg", GB), file("b.dmg", GB)];
    let (title, body) = notification_text(&diff_reports(&a, &b));
    assert_eq!(title, "Downloads grew by 3.0 GB");
    assert!(body.starts_with("2 new big files (2.0 GB)."), "{body}");
}

#[test]
fn schedule_due() {
    let day = 86_400_000;
    assert!(is_due("daily", None, 0));
    assert!(!is_due("daily", Some(0), day - 1));
    assert!(is_due("daily", Some(0), day));
    assert!(!is_due("weekly", Some(0), 6 * day));
    assert!(is_due("weekly", Some(0), 7 * day));
    assert!(!is_due("on_change", None, 10 * day));
}

#[test]
fn debounce_waits_for_quiet() {
    let mut d = Debounce::default();
    assert!(!d.take_if_quiet(0));
    d.event(0);
    d.event(5 * 60_000);
    assert!(!d.take_if_quiet(14 * 60_000));
    assert!(d.take_if_quiet(15 * 60_000));
    assert!(!d.take_if_quiet(60 * 60_000), "fires once");
}

#[test]
fn folder_watcher_reports_changes_and_refuses_home() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    assert!(
        watch_folder(&root, Some(&root), || {}).is_err(),
        "home is refused"
    );
    assert!(
        watch_folder(std::path::Path::new("/"), None, || {}).is_err(),
        "filesystem root is refused"
    );
    let (tx, rx) = std::sync::mpsc::channel();
    let _w = watch_folder(&root, None, move || {
        let _ = tx.send(());
    })
    .unwrap();
    std::fs::write(root.join("new.txt"), "x").unwrap();
    assert!(
        rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok(),
        "an event arrives"
    );
}
