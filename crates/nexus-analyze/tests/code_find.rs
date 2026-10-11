#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Find: search, usages, overview (T09.11).

use std::path::Path;

use nexus_analyze::code::find::{MatchKind, find_symbols, overview, render_find, usages};
use nexus_analyze::code::index::{IndexOptions, RefKind, build_index};
use nexus_analyze::code::{CodeIndex, SymbolKind};
use tokio_util::sync::CancellationToken;

fn index(name: &str) -> CodeIndex {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    build_index(&p, &IndexOptions::default(), None, &CancellationToken::new()).unwrap()
}

#[test]
fn calc_total_usages_are_grouped_by_kind() {
    for fx in ["py_app", "ts_app", "rust_app", "go_app", "java_app"] {
        let idx = index(fx);
        let found = find_symbols(&idx, "calc_total", 5);
        let Some(f) = found.first() else { continue };
        if f.name != "calc_total" {
            continue;
        }
        assert_eq!(f.matched, MatchKind::Exact, "{fx}");
        assert!(f.usages.total() > 0, "{fx}: {f:?}");
    }
    let idx = index("py_app");
    let f = &find_symbols(&idx, "calc_total", 5)[0];
    assert_eq!(f.kind, SymbolKind::Function);
    assert!(f.usages.code >= 1 && f.usages.import >= 1, "{:?}", f.usages);
    let u = usages(&idx, f.symbol);
    assert!(u.iter().any(|u| u.is_def));
    assert!(u.iter().any(|u| u.kind == RefKind::Import && u.path == "main.py"));
    assert!(
        u.windows(2)
            .all(|w| (&w[0].path, w[0].line) <= (&w[1].path, w[1].line))
    );
}

#[test]
fn match_order_and_files() {
    let idx = index("py_app");
    let r = find_symbols(&idx, "pricing", 10);
    assert!(
        r.iter()
            .any(|f| f.kind == SymbolKind::File && f.path == "util/pricing.py")
    );
    assert_eq!(r[0].matched, MatchKind::Exact);
    let r = find_symbols(&idx, "calc", 10);
    assert!(r.iter().all(|f| f.matched != MatchKind::Exact));
    assert!(r[0].matched <= r[r.len() - 1].matched);
    assert!(find_symbols(&idx, "  ", 5).is_empty());
}

#[test]
fn overview_and_render() {
    let idx = index("py_app");
    let o = overview(&idx, 3);
    assert_eq!(o.languages[0].0, "python");
    assert!(o.files >= 3 && o.largest_files.len() <= 3);
    assert!(
        o.most_imported.iter().any(|f| f.path == "util/pricing.py"),
        "{o:?}"
    );
    let text = render_find(&idx, "calc_total", 4000);
    assert!(text.contains("function calc_total"), "{text}");
    assert!(text.contains("main.py"), "{text}");
    let short = render_find(&idx, "calc_total", 120);
    assert!(short.len() < 400 && short.contains("more usages"), "{short}");
    assert!(render_find(&idx, "zzzzqq", 100).starts_with("No symbol"));
}
