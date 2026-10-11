//! Internal prompts (`prompts/internal/*.md`): every file renders from its declared variables, declares
//! every variable it uses, stays within the token budget, and its output is snapshotted.

use nexus_agent::{estimate_tokens, internal_names, internal_source, parse_meta, render_full};
use serde_json::{Value, json};

/// Sample values for each prompt's declared variables.
fn fixture(name: &str) -> Value {
    match name {
        "commit_message" => json!({"stats": "2 files changed, 10 insertions(+), 3 deletions(-)", "diff": "diff --git a/a.rs b/a.rs\n+fn x() {}"}),
        "data_profile" => json!({"facts": "rows: 120\ncolumns: id (int), name (text)", "user_question": "Which column has nulls?"}),
        "doc_map" => json!({"doc_title": "Report.pdf", "chunk_index": 2, "chunk_count": 5, "page_start": 3, "page_end": 4, "text": "Revenue grew 12%."}),
        "doc_qa" => json!({"doc_title": "Report.pdf", "chunks": [{"page_start": 3, "page_end": 3, "text": "Revenue grew 12%."}], "question": "How much did revenue grow?"}),
        "doc_reduce" => json!({"doc_title": "Report.pdf", "page_count": 9, "notes": [{"points": [{"text": "Revenue grew", "page": 3}], "numbers": [{"value": "12%", "label": "growth", "page": 3}], "terms": []}], "user_request": "short"}),
        "editor_rewrite" => json!({"action": "simplify", "language": "rust", "file_name": "a.rs", "selection": "let x = 1 + 0;", "context_before": "fn f() {", "context_after": "}", "instructions": ""}),
        "file_classify" => json!({"file_name": "notes.txt", "excerpt": "buy milk"}),
        "file_report_code" => json!({"facts": "language: Rust\nLOC: 120", "excerpt": "fn main() {}"}),
        "folder_report_narrative" => json!({"facts": "files: 10\ntotal: 1.2 MB", "user_goal": ""}),
        "history_summary" => json!({"previous_summary": "", "turns": [{"role": "user", "content": "hi"}, {"role": "assistant", "content": "hello"}]}),
        "log_triage" => json!({"facts": "errors: 4", "user_question": ""}),
        "reask" => json!({"errors": ["missing field `action`"]}),
        "rename_propose" => json!({"lang": "rust", "kind": "function", "convention": "snake_case", "convention_example": "load_file", "glossary": ["load", "file"], "rejected": [{"id": "s1", "name": "x"}], "symbols": ["<sym id=\"s1\">fn ld()</sym>"]}),
        "router" => json!({"tools": [{"signature": "read_file(path)", "description": "Read a text file."}], "granted_roots": ["/Users/me/Downloads"], "home_dir": "/Users/me", "request": "what is in notes.md", "tool_results_so_far": 0}),
        "secrets_summary" => json!({"facts": "findings: 2"}),
        "strings_improve" => json!({"mode": "improve", "tone": "friendly", "items": [{"id": "a", "text": "Err", "context": "button", "placeholders": ["{n}"], "max_len": 20}]}),
        "tidy_classify" => json!({"targets": [{"name": "Images", "description": "photos"}], "files": [{"id": "f1", "name": "a.png", "ext": "png", "size": 10, "modified": "2026-01-01", "sample": ""}]}),
        "title" => json!({"user_text": "How do signals work?", "assistant_text": "They emit events."}),
        other => panic!("no fixture for internal prompt `{other}`; add one"),
    }
}

#[test]
fn every_internal_prompt_renders_within_budget() {
    let names = internal_names();
    assert_eq!(names.len(), 18, "{names:?}");
    for name in names {
        let src = internal_source(&name).unwrap();
        let meta = parse_meta(src);
        assert!(!meta.vars.is_empty(), "{name}: no `vars:` in the meta header");
        // Every declared variable is used.
        for v in &meta.vars {
            assert!(src.split("#}").nth(1).unwrap_or(src).contains(v.as_str()), "{name}: declares `{v}` but never uses it");
        }
        let rendered = render_full(src, &fixture(&name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let sys = estimate_tokens(&rendered.system);
        assert!(sys <= 350, "{name}: system prompt is {sys} tokens (max 350)");
        let ex: u32 = rendered.examples.iter().map(|(u, a)| estimate_tokens(u) + estimate_tokens(a)).sum();
        assert!(ex <= 300, "{name}: examples are {ex} tokens (max 300)");
    }
}

#[test]
fn omitted_optional_variables_render_empty() {
    let mut v = fixture("data_profile");
    v.as_object_mut().unwrap().remove("user_question");
    let r = render_full(internal_source("data_profile").unwrap(), &v).unwrap();
    assert!(!r.user.contains("Question"), "{}", r.user);
}

#[test]
fn undeclared_variables_are_errors() {
    let src = "{# meta\nvars: a\n#}\n=== system ===\nhi {{ b }}\n=== user ===\n{{ a }}";
    assert!(render_full(src, &json!({"a": 1})).is_err());
}

#[test]
fn router_keeps_its_examples_as_turns() {
    let r = render_full(internal_source("router").unwrap(), &fixture("router")).unwrap();
    assert_eq!(r.examples.len(), 2);
    assert!(r.examples[0].1.contains("\"list_dir\""));
    assert!(r.examples[0].1.contains("/Users/me/Downloads"));
    assert!(!r.system.contains("example"));
}

#[test]
fn snapshots() {
    for name in internal_names() {
        let r = render_full(internal_source(&name).unwrap(), &fixture(&name)).unwrap();
        let mut text = format!("--- system ---\n{}\n", r.system);
        for (u, a) in &r.examples {
            text.push_str(&format!("--- example user ---\n{u}\n--- example assistant ---\n{a}\n"));
        }
        text.push_str(&format!("--- user ---\n{}\n", r.user));
        insta::assert_snapshot!(name, text);
    }
}
