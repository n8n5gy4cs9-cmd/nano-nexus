#![allow(clippy::unwrap_used, clippy::expect_used)]
//! T19.09: nothing the user typed, the model wrote, or a tool returned may reach a snapshot, a replay
//! frame, a debug print or JSON. The fixtures are full of `CANARY_` strings in every text position.

use std::fs;
use std::path::{Path, PathBuf};

use nexus_observe::claude::ClaudeParser;
use nexus_observe::codex::CodexParser;
use nexus_observe::model::LineParser;
use nexus_observe::{Observer, ObserverConfig, Source};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn lines(p: &str) -> Vec<String> {
    fs::read_to_string(fixtures().join(p))
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn fixtures_really_contain_canaries() {
    let all: String = [
        "claude/session.jsonl",
        "claude/subagent.jsonl",
        "codex/rollout.jsonl",
    ]
    .iter()
    .flat_map(|f| lines(f))
    .collect();
    assert!(all.matches("CANARY_").count() > 25);
}

#[test]
fn parsers_never_emit_text() {
    let mut cc = ClaudeParser::new();
    let mut cx = CodexParser::new("f");
    let mut dump = String::new();
    for f in ["claude/session.jsonl", "claude/subagent.jsonl"] {
        for l in lines(f) {
            dump += &format!("{:?}", cc.parse_line(&l, 0));
        }
    }
    for l in lines("codex/rollout.jsonl") {
        dump += &format!("{:?}", cx.parse_line(&l, 0));
    }
    assert!(!dump.contains("CANARY"), "{dump}");
    assert!(!dump.contains("sk-ant"), "{dump}");
    assert!(
        dump.contains("login.ts"),
        "edit-tool paths are the one allowed string"
    );
    assert!(
        dump.contains("Edit") && dump.contains("shell"),
        "tool names are kept"
    );
}

fn fake_home() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let proj = d.path().join(".claude/projects/-work-demo-app");
    fs::create_dir_all(proj.join("cc-1/subagents")).unwrap();
    fs::copy(fixtures().join("claude/session.jsonl"), proj.join("cc-1.jsonl")).unwrap();
    fs::copy(
        fixtures().join("claude/subagent.jsonl"),
        proj.join("cc-1/subagents/agent-ag7.jsonl"),
    )
    .unwrap();
    let day = d.path().join(".codex/sessions/2026/10/19");
    fs::create_dir_all(&day).unwrap();
    fs::copy(
        fixtures().join("codex/rollout.jsonl"),
        day.join("rollout-2026-10-19T10-01-00-0199aaaa-bbbb-cccc-dddd-eeeeffff0000.jsonl"),
    )
    .unwrap();
    d
}

/// 2026-10-19T10:05:00Z
const NOW: i64 = 1_792_404_300_000;

fn observer(home: &Path, claude: bool, codex: bool) -> Observer {
    Observer::new(ObserverConfig {
        window_hours: 6,
        nanonexus: true,
        claude_root: claude.then(|| home.join(".claude")),
        codex_root: codex.then(|| home.join(".codex")),
        demo: false,
    })
}

#[test]
fn snapshots_replay_frames_and_json_are_canary_free() {
    let home = fake_home();
    let mut o = observer(home.path(), true, true);
    let (snap, _) = o.poll(NOW);
    assert_eq!(o.tracked_files(), 3);
    assert_eq!(snap.totals.agents, 3, "main + subagent + codex");
    assert_eq!(snap.sources[Source::ClaudeCode as usize].agents, 2);
    assert_eq!(snap.sources[Source::Codex as usize].agents, 1);
    // Tokens: Claude m1 counted once (3+45... final max 120 out, 3 in, 200 write) + m2 + subagent.
    let cc = snap.agents.iter().find(|a| a.id == "claude_code:cc-1").unwrap();
    assert_eq!(cc.tokens.input, 3 + 4);
    assert_eq!(cc.tokens.output, 120 + 30);
    assert_eq!(cc.tokens.cache_write, 200);
    assert_eq!(cc.errors, 1, "the failed Bash call");
    let cx = snap.agents.iter().find(|a| a.id == "codex:cx-1").unwrap();
    assert_eq!(cx.tokens.cache_read, 1500);
    assert_eq!(cx.tokens.input, 500);
    assert_eq!(cx.errors, 1, "exit code 1");

    let mut dump = format!("{snap:?}{}", serde_json::to_string(&snap).unwrap());
    let (a, b) = o.replay_range().expect("backfilled");
    let mut t = a;
    while t <= b {
        let f = o.frame_at(t).unwrap();
        dump += &serde_json::to_string(&f).unwrap();
        t += 60_000;
    }
    assert!(!dump.contains("CANARY"), "{dump}");
    assert!(!dump.contains("sk-ant"));
    assert!(!dump.contains("rm -rf"));
    assert!(!dump.contains("example.invalid"));
}

#[test]
fn the_same_file_from_two_tools_is_a_conflict_without_leaking_content() {
    let home = fake_home();
    let mut o = observer(home.path(), true, true);
    // Claude edits /work/demo-app/src/login.ts at 10:00:04; Codex patches src/login.ts (relative to the
    // same cwd) at 10:01:06. Two minutes after the first touch they are still in conflict.
    let (snap, _) = o.poll(NOW - 180_000);
    assert_eq!(snap.conflicts.len(), 1, "{:?}", snap.conflicts);
    assert_eq!(snap.conflicts[0].path_label, "src/login.ts");
}

#[test]
fn switched_off_sources_are_never_opened() {
    let home = fake_home();
    let mut o = observer(home.path(), false, false);
    let (snap, _) = o.poll(NOW);
    assert_eq!(o.tracked_files(), 0);
    assert_eq!(snap.totals.agents, 0);
    // Only one on: only that one.
    let mut o = observer(home.path(), false, true);
    let (snap, _) = o.poll(NOW);
    assert_eq!(o.tracked_files(), 1);
    assert_eq!(snap.sources[Source::ClaudeCode as usize].agents, 0);
    assert_eq!(snap.sources[Source::Codex as usize].agents, 1);
    // Switching a source off forgets it at once.
    o.apply_config(ObserverConfig {
        window_hours: 6,
        nanonexus: true,
        ..Default::default()
    });
    let (snap, _) = o.poll(NOW + 1000);
    assert_eq!(snap.totals.agents, 0);
    assert_eq!(o.tracked_files(), 0);
}
