#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Strings mode: extraction, placeholders, export, `t("KEY")` rewrite (T09.13).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use nexus_analyze::code::CodeIndex;
use nexus_analyze::code::index::{IndexOptions, build_index};
use nexus_analyze::code::strings::*;
use nexus_core::changes::FileOp;
use tokio_util::sync::CancellationToken;

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

fn project() -> (tempfile::TempDir, PathBuf, CodeIndex) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/strings_app");
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap().join("p");
    copy_dir(&src, &root);
    let idx = build_index(&root, &IndexOptions::default(), None, &CancellationToken::new()).unwrap();
    (tmp, root, idx)
}

#[test]
fn placeholders_are_extracted_in_order() {
    assert_eq!(
        placeholders("%s has %d (%0.2f) {name} {0} {0:.2f} %(who)s [b]x[/b]\\n 100%"),
        [
            "%s", "%d", "%0.2f", "{name}", "{0}", "{0:.2f}", "%(who)s", "[b]", "[/b]", "\\n"
        ]
    );
    assert!(placeholders("Plain text, 100% sure").is_empty());
    assert!(same_placeholders("Hi %s, %d", "%d items for %s"));
    assert!(!same_placeholders("Hi %s", "Hi"));
    assert!(!same_placeholders("{a} {a}", "{a}"));
}

#[test]
fn extracts_only_user_facing_strings() {
    let (_t, _r, idx) = project();
    let items = extract_strings(&idx);
    let texts: Vec<&str> = items.iter().map(|i| i.text.as_str()).collect();
    for want in [
        "Start game",
        "Score: %d points",
        "Hello",
        "Press <b>Enter</b> to continue\\n",
        "Say \"hi\"",
        "Settings",
        "Delete {name}?",
        "Save {name}",
        "Saved!",
    ] {
        assert!(texts.contains(&want), "missing {want:?} in {texts:?}");
    }
    for no in [
        "starting up",
        "score is %d",
        "fast",
        "danger",
        "opening dialog",
        "../i18n",
    ] {
        assert!(!texts.contains(&no), "{no:?} is not user-facing");
    }
    assert!(
        !texts.iter().any(|t| t.starts_with("Hi ")),
        "f-strings are skipped"
    );
    let hello = items.iter().find(|i| i.text == "Hello").unwrap();
    assert!(hello.localized);
    let score = items.iter().find(|i| i.text.starts_with("Score")).unwrap();
    assert_eq!(score.placeholders, ["%d"]);
    assert_eq!(score.context, "settext");
    assert!(items.iter().enumerate().all(|(i, it)| it.id == i as u32));
}

#[test]
fn validation() {
    let (_t, _r, idx) = project();
    let items = extract_strings(&idx);
    let score = items.iter().find(|i| i.text.starts_with("Score")).unwrap();
    let none = HashSet::new();
    assert!(validate(score, Some("Points: %d"), Some("HUD_SCORE"), Some(20), &none).is_ok());
    assert!(
        validate(score, Some("Points"), None, None, &none)
            .unwrap_err()
            .contains("placeholders")
    );
    assert!(
        validate(score, Some("Your total score: %d points"), None, Some(10), &none)
            .unwrap_err()
            .contains("longer")
    );
    assert!(validate(score, None, Some("hud_score"), None, &none).is_err());
    let used: HashSet<String> = ["HUD_SCORE".to_owned()].into();
    assert!(
        validate(score, None, Some("HUD_SCORE"), None, &used)
            .unwrap_err()
            .contains("already used")
    );
    assert!(is_key("MENU_START_GAME") && !is_key("MENU__X") && !is_key("1ABC") && !is_key("A_"));
    assert_eq!(fallback_key("Start game now, please!"), "START_GAME_NOW_PLEASE");
    assert_eq!(fallback_key("42 apples"), "TEXT_42_APPLES");
}

#[test]
fn exports() {
    let rows = vec![
        Row {
            key: "A".into(),
            text: "Say \"hi\", ok".into(),
            original: "hi".into(),
            path: "a.py".into(),
            line: 3,
        },
        Row {
            key: "B".into(),
            text: "Line\\nTwo".into(),
            original: "x".into(),
            path: "b.py".into(),
            line: 4,
        },
    ];
    let csv = export_csv(&rows);
    assert!(csv.starts_with("key,text,original,file,line\n"));
    assert!(csv.contains("A,\"Say \"\"hi\"\", ok\",hi,a.py,3"), "{csv}");
    let json: serde_json::Value = serde_json::from_str(&export_json(&rows)).unwrap();
    assert_eq!(json["B"], "Line\nTwo");
}

#[test]
fn rewrite_to_t_calls() {
    let (_t, root, idx) = project();
    let items = extract_strings(&idx);
    let pick = |t: &str, k: &str| (items.iter().find(|i| i.text == t).unwrap().clone(), k.to_owned());
    let rows = vec![
        pick("Start game", "MENU_START_GAME"),
        pick("Save {name}", "DIALOG_SAVE"),
        pick("Hello", "GREETING"),
    ];
    let cs = plan_key_rewrite(&idx, &rows, "t", 0).unwrap();
    assert_eq!(cs.ops.len(), 2, "the localized string is left alone");
    for op in &cs.ops {
        let FileOp::Edit { path, edits, .. } = op else {
            panic!()
        };
        let mut b = std::fs::read(path).unwrap();
        for e in edits.iter().rev() {
            b.splice(e.start as usize..e.end as usize, e.new_text.bytes());
        }
        std::fs::write(path, b).unwrap();
    }
    let py = std::fs::read_to_string(root.join("app.py")).unwrap();
    assert!(py.contains("Button(label=t(\"MENU_START_GAME\"))"), "{py}");
    let tsx = std::fs::read_to_string(root.join("ui/dialog.tsx")).unwrap();
    assert!(tsx.contains("label={t(\"DIALOG_SAVE\")}"), "{tsx}");
}
