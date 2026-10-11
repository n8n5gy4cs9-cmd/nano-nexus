#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The naming rules over the fixture projects (T09.08).

use std::path::{Path, PathBuf};

use nexus_analyze::code::CodeIndex;
use nexus_analyze::code::index::{IndexOptions, build_index};
use nexus_analyze::code::rules::{RuleOptions, detect};
use tokio_util::sync::CancellationToken;

fn index(name: &str) -> CodeIndex {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    build_index(&p, &IndexOptions::default(), None, &CancellationToken::new()).unwrap()
}

/// `kind name -> rules` for every candidate.
fn found(idx: &CodeIndex) -> Vec<(String, Vec<String>, Option<String>)> {
    detect(idx, &RuleOptions::default())
        .into_iter()
        .map(|c| {
            let s = idx.symbol(c.symbol).unwrap();
            (
                format!("{} {}", s.kind.label(), s.name),
                c.issues.into_iter().map(|i| i.rule).collect(),
                c.suggested,
            )
        })
        .collect()
}

fn get<'a>(
    v: &'a [(String, Vec<String>, Option<String>)],
    key: &str,
) -> &'a (String, Vec<String>, Option<String>) {
    v.iter().find(|(k, ..)| k == key).unwrap_or_else(|| {
        panic!(
            "{key} not flagged; flagged: {:?}",
            v.iter().map(|x| &x.0).collect::<Vec<_>>()
        )
    })
}

#[test]
fn python_fixture_flags_symbols_files_and_leaves_reserved_alone() {
    let idx = index("py_app");
    let v = found(&idx);
    assert!(
        get(&v, "class cart_item")
            .1
            .contains(&"case.mismatch".to_string())
    );
    assert_eq!(get(&v, "class cart_item").2.as_deref(), Some("CartItem"));
    let d = get(&v, "method doStuff2");
    assert!(
        d.1.contains(&"meaningless".into())
            && d.1.contains(&"numbered".into())
            && d.1.contains(&"case.mismatch".into())
    );
    assert!(get(&v, "variable tmp").1.contains(&"meaningless".into()));
    let f = get(&v, "file Helper_Funcs");
    assert_eq!(f.1, ["file.case"]);
    assert_eq!(f.2.as_deref(), Some("helper_funcs"));
    // Reserved things never show up.
    for n in ["function main", "method __init__", "function test_total"] {
        assert!(!v.iter().any(|(k, ..)| k == n), "{n} must not be flagged");
    }
    assert!(!v.iter().any(|(k, ..)| k == "constant TAX_RATE"));
}

#[test]
fn other_fixtures() {
    let ts = found(&index("ts_app"));
    assert!(get(&ts, "class cart_item").1.contains(&"case.mismatch".into()));
    assert!(get(&ts, "file Helper_Funcs").1.contains(&"file.case".into()));
    assert!(
        !ts.iter().any(|(k, ..)| k == "file Button"),
        "tsx components may be PascalCase"
    );

    let go = found(&index("go_app"));
    assert!(get(&go, "folder util_funcs").1.contains(&"file.case".into()));
    assert!(get(&go, "method doStuff2").1.contains(&"meaningless".into()));

    let java = found(&index("java_app"));
    assert!(get(&java, "file cart_item").1.contains(&"file.case".into()));
    assert!(get(&java, "class cart_item").1.contains(&"case.mismatch".into()));

    let rs = found(&index("rust_app"));
    assert!(get(&rs, "file Helpers").1.contains(&"file.case".into()));
    assert!(get(&rs, "method doStuff2").1.contains(&"case.mismatch".into()));

    let cs = found(&index("cs_app"));
    assert!(get(&cs, "class cart_item").1.contains(&"case.mismatch".into()));
    assert!(get(&cs, "file calc_helpers").1.contains(&"file.case".into()));
    let m = get(&cs, "method doStuff2");
    assert!(m.1.contains(&"meaningless".into()));
}
