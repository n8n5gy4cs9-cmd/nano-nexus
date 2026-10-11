#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Reserved names, entry points, tests, overrides, and the ignore list (T09.07).

use std::path::{Path, PathBuf};

use nexus_analyze::code::SymbolKind;
use nexus_analyze::code::index::{CodeIndex, Flags, IndexOptions, build_index};
use tokio_util::sync::CancellationToken;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn index_with(path: &Path, opts: IndexOptions) -> CodeIndex {
    build_index(path, &opts, None, &CancellationToken::new()).unwrap()
}

fn flags(idx: &CodeIndex, name: &str, kind: SymbolKind) -> Flags {
    let v: Vec<_> = idx
        .symbols
        .iter()
        .filter(|s| s.name == name && s.kind == kind)
        .collect();
    assert_eq!(v.len(), 1, "{name}");
    v[0].flags
}

#[test]
fn entry_points_dunders_and_tests_are_reserved() {
    let py = index_with(&fixture("py_app"), IndexOptions::default());
    let main = flags(&py, "main", SymbolKind::Function);
    assert!(main.has(Flags::ENTRY_POINT) && main.has(Flags::RESERVED));
    assert!(flags(&py, "__init__", SymbolKind::Method).has(Flags::RESERVED));
    let t = flags(&py, "test_total", SymbolKind::Function);
    assert!(t.has(Flags::TEST) && t.has(Flags::RESERVED));
    for n in ["calc_total", "cart_item", "doStuff2", "fmt_money"] {
        let kind = if n == "cart_item" {
            SymbolKind::Class
        } else if n == "doStuff2" {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };
        assert!(
            !flags(&py, n, kind).has(Flags::RESERVED),
            "{n} must stay renameable"
        );
    }
    let calc = flags(&py, "calc_total", SymbolKind::Function);
    assert!(
        calc.has(Flags::PUBLIC_API) && calc.has(Flags::DYNAMIC_REFS),
        "exported, and named by a getattr string"
    );

    let rs = index_with(&fixture("rust_app"), IndexOptions::default());
    assert!(flags(&rs, "main", SymbolKind::Function).has(Flags::ENTRY_POINT));
    assert!(
        flags(&rs, "total_works", SymbolKind::Function).has(Flags::TEST),
        "#[test]"
    );
    assert!(!flags(&rs, "doStuff2", SymbolKind::Method).has(Flags::RESERVED));

    let go = index_with(&fixture("go_app"), IndexOptions::default());
    assert!(flags(&go, "main", SymbolKind::Function).has(Flags::ENTRY_POINT));
    assert!(
        flags(&go, "CalcTotal", SymbolKind::Function).has(Flags::PUBLIC_API),
        "capitalised = exported"
    );
    assert!(!flags(&go, "doStuff2", SymbolKind::Method).has(Flags::PUBLIC_API));

    let java = index_with(&fixture("java_app"), IndexOptions::default());
    assert!(flags(&java, "main", SymbolKind::Method).has(Flags::ENTRY_POINT));
    let cs = index_with(&fixture("cs_app"), IndexOptions::default());
    assert!(flags(&cs, "Main", SymbolKind::Method).has(Flags::ENTRY_POINT));
}

#[test]
fn methods_that_override_are_reserved() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("shapes.py"),
        "class Shape:\n    def area(self):\n        return 0\n\n    def describe(self):\n        return 'shape'\n\nclass Square(Shape):\n    def area(self):\n        return 4\n\n    def side_length(self):\n        return 2\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("A.java"),
        "public class A extends Base {\n    @Override\n    public void run() {}\n    public void other() {}\n}\nclass Base { void run() {} }\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("t.ts"),
        "class C { toString() { return ''; } name() { return 'x'; } }\n",
    )
    .unwrap();
    let idx = index_with(tmp.path(), IndexOptions::default());
    let by_scope = |name: &str, scope: &str| {
        idx.symbols
            .iter()
            .find(|s| s.name == name && s.scope == scope && s.kind == SymbolKind::Method)
            .unwrap()
            .flags
    };
    assert!(
        by_scope("area", "Square").has(Flags::OVERRIDE),
        "overrides Shape.area"
    );
    assert!(!by_scope("area", "Shape").has(Flags::OVERRIDE));
    assert!(!by_scope("side_length", "Square").has(Flags::RESERVED));
    assert!(by_scope("run", "A").has(Flags::OVERRIDE));
    assert!(!by_scope("other", "A").has(Flags::RESERVED));
    assert!(by_scope("toString", "C").has(Flags::RESERVED));
    assert!(!by_scope("name", "C").has(Flags::RESERVED));
}

#[test]
fn the_ignore_list_reserves_names_and_paths() {
    let opts = IndexOptions {
        ignore: vec!["calc*".into(), "path:tests".into(), "# comment".into()],
        ..IndexOptions::default()
    };
    let py = index_with(&fixture("py_app"), opts);
    assert!(flags(&py, "calc_total", SymbolKind::Function).has(Flags::RESERVED));
    assert!(
        py.symbols
            .iter()
            .filter(|s| s.name == "test_total")
            .all(|s| s.flags.has(Flags::RESERVED))
    );
    let t = py
        .symbols
        .iter()
        .find(|s| s.name == "tests" && s.kind == SymbolKind::Folder)
        .unwrap();
    assert!(t.flags.has(Flags::RESERVED));
    assert!(!flags(&py, "fmt_money", SymbolKind::Function).has(Flags::RESERVED));
}
