#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Verify: checker detection and streamed runs (T09.12).

use nexus_analyze::code::verify::{Checker, detect, run};
use tokio_util::sync::CancellationToken;

fn sh(script: &str) -> Checker {
    Checker {
        id: "t".into(),
        label: "t".into(),
        program: "sh".into(),
        args: vec!["-c".into(), script.into()],
        dir: String::new(),
        available: true,
    }
}

#[test]
fn detects_checkers_from_manifests() {
    let d = tempfile::tempdir().unwrap();
    assert!(detect(d.path()).is_empty());
    for f in [
        "Cargo.toml",
        "tsconfig.json",
        "go.mod",
        "pom.xml",
        "App.csproj",
        "build.gradle.kts",
        "a.py",
    ] {
        std::fs::write(d.path().join(f), "").unwrap();
    }
    let ids: Vec<String> = detect(d.path()).into_iter().map(|c| c.id).collect();
    assert_eq!(ids, ["cargo", "tsc", "python", "dotnet", "go", "maven", "gradle"]);
    let cargo = &detect(d.path())[0];
    assert_eq!(
        (cargo.program.as_str(), cargo.args.clone()),
        ("cargo", vec!["check".to_owned()])
    );
}

#[cfg(unix)]
#[tokio::test]
async fn streams_output_and_reports_failure() {
    let d = tempfile::tempdir().unwrap();
    let mut lines = Vec::new();
    let r = run(
        d.path(),
        &sh("echo hello; echo 'error: boom' >&2; exit 3"),
        &CancellationToken::new(),
        |l| lines.push(l.to_owned()),
    )
    .await;
    assert!(!r.ok && r.exit_code == Some(3) && !r.cancelled && !r.missing);
    assert!(lines.contains(&"hello".to_owned()) && lines.contains(&"error: boom".to_owned()));
    assert_eq!(r.first_errors, vec!["error: boom".to_owned()]);

    let ok = run(d.path(), &sh("echo fine"), &CancellationToken::new(), |_| {}).await;
    assert!(ok.ok && ok.exit_code == Some(0));
}

#[cfg(unix)]
#[tokio::test]
async fn cancel_stops_the_process_and_missing_binary_is_reported() {
    let d = tempfile::tempdir().unwrap();
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        c2.cancel();
    });
    let t = std::time::Instant::now();
    let r = run(d.path(), &sh("sleep 30"), &cancel, |_| {}).await;
    assert!(r.cancelled && !r.ok);
    assert!(t.elapsed() < std::time::Duration::from_secs(10));

    let mut gone = sh("");
    gone.program = "nexus-no-such-checker".into();
    let r = run(d.path(), &gone, &CancellationToken::new(), |_| {}).await;
    assert!(r.missing && !r.ok);
}
