#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The symbol index on the fixture projects (T09.04).

use std::path::{Path, PathBuf};

use nexus_analyze::code::index::{CodeIndex, IndexOptions, RefKind, build_index};
use nexus_analyze::code::{Symbol, SymbolKind};
use tokio_util::sync::CancellationToken;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn index(name: &str) -> CodeIndex {
    build_index(
        &fixture(name),
        &IndexOptions::default(),
        None,
        &CancellationToken::new(),
    )
    .unwrap()
}

fn only<'a>(idx: &'a CodeIndex, name: &str, kind: SymbolKind) -> &'a Symbol {
    let v: Vec<_> = idx
        .symbols
        .iter()
        .filter(|s| s.name == name && s.kind == kind)
        .collect();
    assert_eq!(
        v.len(),
        1,
        "expected one {kind:?} named {name}, found {}",
        v.len()
    );
    v[0]
}

/// `path:line` of every code and import reference (strings, config, docs and comments are checked separately).
fn places(idx: &CodeIndex, s: &Symbol) -> Vec<String> {
    s.refs
        .iter()
        .filter(|r| matches!(r.kind, RefKind::Code | RefKind::Import))
        .map(|r| format!("{}:{}", idx.path_of(r.loc.file).unwrap(), r.loc.line))
        .collect()
}

#[test]
fn python_calc_total_has_every_reference_and_no_extras() {
    let idx = index("py_app");
    let s = only(&idx, "calc_total", SymbolKind::Function);
    assert_eq!(
        places(&idx, s),
        [
            "main.py:2",
            "main.py:9",
            "tests/test_pricing.py:1",
            "tests/test_pricing.py:5",
            "util/pricing.py:14"
        ],
    );
    assert!(
        s.refs
            .iter()
            .any(|r| r.is_def && idx.files[r.loc.file as usize].path == "util/pricing.py")
    );
    assert!(s.refs.iter().any(|r| r.kind == RefKind::Import));
    assert!(s.refs.iter().all(|r| !r.ambiguous));
}

#[test]
fn python_kinds_scopes_and_locals() {
    let idx = index("py_app");
    let class = only(&idx, "cart_item", SymbolKind::Class);
    assert_eq!(class.refs.len(), 6, "{:?}", places(&idx, class));
    let m = only(&idx, "doStuff2", SymbolKind::Method);
    assert_eq!(m.scope, "cart_item");
    assert!(m.signature.starts_with("def doStuff2"));
    only(&idx, "main", SymbolKind::Function);
    // `total` exists as a local in two different functions: two symbols, never merged.
    let totals: Vec<_> = idx
        .symbols
        .iter()
        .filter(|s| s.name == "total" && s.kind == SymbolKind::Variable)
        .collect();
    assert_eq!(totals.len(), 2);
    let in_calc = totals.iter().find(|s| s.scope == "calc_total").unwrap();
    // def, `total += …`, `return total`
    assert_eq!(in_calc.refs.len(), 3, "{:?}", places(&idx, in_calc));
    assert!(
        in_calc
            .refs
            .iter()
            .all(|r| idx.files[r.loc.file as usize].path == "util/pricing.py")
    );
    // `self.name` is the class field; the constructor parameter `name` is a separate symbol.
    let field = only(&idx, "name", SymbolKind::Field);
    assert_eq!(field.scope, "cart_item");
    assert!(
        idx.symbols
            .iter()
            .any(|s| s.name == "name" && s.kind == SymbolKind::Parameter)
    );
    let tax = only(&idx, "TAX_RATE", SymbolKind::Constant);
    assert_eq!(tax.refs.len(), 3, "{:?}", places(&idx, tax));
}

#[test]
fn files_and_folders_are_symbols() {
    let idx = index("py_app");
    let f = only(&idx, "Helper_Funcs", SymbolKind::File);
    assert_eq!(f.signature, "Helper_Funcs.py");
    let d = only(&idx, "util", SymbolKind::Folder);
    assert_eq!(d.signature, "util");
    assert!(idx.folders.contains(&"tests".to_owned()));
}

#[test]
fn typescript_references_include_shorthand_and_type_queries() {
    let idx = index("ts_app");
    let s = only(&idx, "calcTotal", SymbolKind::Function);
    let p = places(&idx, s);
    assert!(p.contains(&"src/lib/pricing.ts:11".into()), "{p:?}");
    assert!(p.contains(&"src/index.ts:1".into()), "{p:?}");
    assert!(p.contains(&"src/index.ts:5".into()), "shorthand property: {p:?}");
    assert!(
        p.contains(&"src/index.ts:9".into()),
        "typeof calcTotal and the call: {p:?}"
    );
    // The string "calcTotal" is not a code reference; it is a StringLiteral one.
    assert!(s.refs.iter().any(|r| r.kind == RefKind::StringLiteral));
    let class = only(&idx, "cart_item", SymbolKind::Class);
    assert!(class.refs.len() >= 4, "{:?}", places(&idx, class));
    // Constructor parameter properties are fields.
    assert!(
        idx.symbols
            .iter()
            .any(|s| s.name == "price" && s.kind == SymbolKind::Field)
    );
}

#[test]
fn go_java_csharp_find_member_calls_through_the_package() {
    let go = index("go_app");
    let s = only(&go, "CalcTotal", SymbolKind::Function);
    assert_eq!(places(&go, s), ["main.go:12", "pricing/pricing.go:15"]);
    let java = index("java_app");
    let s = only(&java, "calcTotal", SymbolKind::Method);
    assert_eq!(
        places(&java, s),
        [
            "src/main/java/shop/Main.java:9",
            "src/main/java/shop/pricing/Calc.java:6"
        ]
    );
    let cs = index("cs_app");
    let s = only(&cs, "CalcTotal", SymbolKind::Method);
    let p = places(&cs, s);
    assert!(
        p.contains(&"Pricing/calc_helpers.cs:15".into())
            && p.contains(&"Program.cs:11".into())
            && p.contains(&"Program.cs:12".into()),
        "{p:?}"
    );
}

#[test]
fn indexes_round_trip_and_are_deterministic() {
    let a = index("py_app");
    let b = index("py_app");
    assert_eq!(a, b);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("cache/index.bin");
    a.save(&file).unwrap();
    assert_eq!(CodeIndex::load(&file).unwrap(), a);
    std::fs::write(&file, b"garbage").unwrap();
    assert!(CodeIndex::load(&file).is_err());
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

#[test]
fn incremental_reindex_only_parses_changed_files() {
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(&fixture("py_app"), tmp.path());
    let cancel = CancellationToken::new();
    let opts = IndexOptions::default();
    let first = build_index(tmp.path(), &opts, None, &cancel).unwrap();
    assert_eq!(first.reparsed as usize, first.files.len());

    let again = build_index(tmp.path(), &opts, Some(&first), &cancel).unwrap();
    assert_eq!(again.reparsed, 0);
    assert_eq!(again.symbols, first.symbols);

    std::fs::write(
        tmp.path().join("Helper_Funcs.py"),
        "def format_money(value):\n    return str(value)\n",
    )
    .unwrap();
    let third = build_index(tmp.path(), &opts, Some(&again), &cancel).unwrap();
    assert_eq!(third.reparsed, 1);
    assert!(third.symbols.iter().any(|s| s.name == "format_money"));
    assert!(!third.symbols.iter().any(|s| s.name == "fmt_money"));
}

#[test]
fn cancel_stops_indexing() {
    let cancel = CancellationToken::new();
    cancel.cancel();
    let r = build_index(&fixture("py_app"), &IndexOptions::default(), None, &cancel);
    assert!(matches!(r, Err(nexus_analyze::code::IndexError::Cancelled)));
}

fn named(idx: &CodeIndex, name: &str, kind: SymbolKind, scope_or_sig: &str) -> Vec<String> {
    let s = idx
        .symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind && s.signature == scope_or_sig)
        .unwrap_or_else(|| panic!("no {kind:?} {name} ({scope_or_sig})"));
    places(idx, s)
}

#[test]
fn imports_name_files_and_folders() {
    let py = index("py_app");
    assert_eq!(
        named(&py, "pricing", SymbolKind::File, "util/pricing.py"),
        ["main.py:2", "main.py:3", "main.py:10", "tests/test_pricing.py:1"]
    );
    assert_eq!(
        named(&py, "util", SymbolKind::Folder, "util"),
        ["main.py:2", "main.py:3", "tests/test_pricing.py:1"]
    );
    assert_eq!(
        named(&py, "Helper_Funcs", SymbolKind::File, "Helper_Funcs.py"),
        ["main.py:4"]
    );

    let ts = index("ts_app");
    // `@lib/pricing` goes through tsconfig `paths`; `../lib/Helper_Funcs` is relative.
    assert_eq!(
        named(&ts, "pricing", SymbolKind::File, "src/lib/pricing.ts"),
        ["src/index.ts:1"]
    );
    assert_eq!(
        named(&ts, "Helper_Funcs", SymbolKind::File, "src/lib/Helper_Funcs.ts"),
        ["src/index.ts:2", "src/ui/Button.tsx:1"]
    );
    assert_eq!(
        named(&ts, "lib", SymbolKind::Folder, "src/lib"),
        ["src/index.ts:2", "src/ui/Button.tsx:1"]
    );
    assert_eq!(
        named(&ts, "Button", SymbolKind::File, "src/ui/Button.tsx"),
        ["src/index.ts:3"]
    );

    let rs = index("rust_app");
    assert_eq!(
        named(&rs, "pricing", SymbolKind::File, "src/pricing.rs"),
        ["src/main.rs:2", "src/main.rs:4"]
    );
    assert_eq!(
        named(&rs, "Helpers", SymbolKind::File, "src/Helpers.rs"),
        ["src/main.rs:1", "src/main.rs:5"]
    );
    let s = only(&rs, "calc_total", SymbolKind::Function);
    assert_eq!(
        places(&rs, s),
        [
            "src/main.rs:4",
            "src/main.rs:9",
            "src/main.rs:19",
            "src/pricing.rs:15"
        ]
    );

    let go = index("go_app");
    assert_eq!(
        named(&go, "pricing", SymbolKind::Folder, "pricing"),
        ["main.go:6", "main.go:11", "main.go:12", "main.go:13"]
    );
    assert_eq!(
        named(&go, "util_funcs", SymbolKind::Folder, "util_funcs"),
        ["main.go:7", "main.go:13"]
    );

    let java = index("java_app");
    let pricing = named(&java, "pricing", SymbolKind::Folder, "src/main/java/shop/pricing");
    assert_eq!(
        pricing,
        [
            "src/main/java/shop/Main.java:3",
            "src/main/java/shop/Main.java:4",
            "src/main/java/shop/pricing/Calc.java:1",
            "src/main/java/shop/pricing/cart_item.java:1"
        ]
    );
}

#[test]
fn tsconfig_comments_and_trailing_commas_are_tolerated() {
    use nexus_analyze::code::imports::strip_jsonc;
    let v: serde_json::Value = serde_json::from_str(&strip_jsonc(
        "{\n // c\n \"a\": [1, 2,], /* x */ \"u\": \"http://x//y\",\n}",
    ))
    .unwrap();
    assert_eq!(v["u"], "http://x//y");
    assert_eq!(v["a"][1], 2);
}

fn refs_by_kind(idx: &CodeIndex, s: &Symbol, kind: RefKind) -> Vec<String> {
    s.refs
        .iter()
        .filter(|r| r.kind == kind)
        .map(|r| {
            format!(
                "{}:{}{}",
                idx.path_of(r.loc.file).unwrap(),
                r.loc.line,
                if r.ambiguous { "?" } else { "" }
            )
        })
        .collect()
}

#[test]
fn config_string_and_doc_references() {
    let py = index("py_app");
    let f = only(&py, "calc_total", SymbolKind::Function);
    assert_eq!(refs_by_kind(&py, f, RefKind::ConfigFile), ["pyproject.toml:6"]);
    assert_eq!(
        refs_by_kind(&py, f, RefKind::StringLiteral),
        ["main.py:10"],
        "getattr is a recognised call position"
    );
    assert_eq!(refs_by_kind(&py, f, RefKind::Doc), ["README.md:1"]);
    let pricing = py
        .symbols
        .iter()
        .find(|s| s.name == "pricing" && s.kind == SymbolKind::File)
        .unwrap();
    assert_eq!(
        refs_by_kind(&py, pricing, RefKind::ConfigFile),
        ["pyproject.toml:6"]
    );
    assert_eq!(refs_by_kind(&py, pricing, RefKind::Doc), ["README.md:1"]);
    let util = only(&py, "util", SymbolKind::Folder);
    assert_eq!(refs_by_kind(&py, util, RefKind::ConfigFile), ["pyproject.toml:6"]);

    let ts = index("ts_app");
    let f = only(&ts, "calcTotal", SymbolKind::Function);
    assert_eq!(
        refs_by_kind(&ts, f, RefKind::StringLiteral),
        ["src/index.ts:9"],
        "registry[\"calcTotal\"]"
    );
    let src = ts
        .symbols
        .iter()
        .find(|s| s.name == "src" && s.kind == SymbolKind::Folder)
        .unwrap();
    let c = refs_by_kind(&ts, src, RefKind::ConfigFile);
    assert_eq!(
        c,
        [
            "package.json:4",
            "package.json:5",
            "tsconfig.json:5",
            "tsconfig.json:7"
        ]
    );
    let lib = ts
        .symbols
        .iter()
        .find(|s| s.name == "lib" && s.kind == SymbolKind::Folder)
        .unwrap();
    assert_eq!(refs_by_kind(&ts, lib, RefKind::ConfigFile), ["tsconfig.json:5"]);

    let cs = index("cs_app");
    let p = only(&cs, "Program", SymbolKind::Class);
    assert_eq!(refs_by_kind(&cs, p, RefKind::ConfigFile), ["Shop.csproj:5"]);
    let helpers = cs
        .symbols
        .iter()
        .find(|s| s.name == "calc_helpers" && s.kind == SymbolKind::File)
        .unwrap();
    assert_eq!(refs_by_kind(&cs, helpers, RefKind::ConfigFile), ["Shop.csproj:8"]);

    let java = index("java_app");
    let m = only(&java, "Main", SymbolKind::Class);
    assert!(refs_by_kind(&java, m, RefKind::ConfigFile).contains(&"pom.xml:7".into()));
    let g = java
        .symbols
        .iter()
        .find(|s| s.name == "calcTotal" && s.kind == SymbolKind::Method)
        .unwrap();
    assert_eq!(
        refs_by_kind(&java, g, RefKind::StringLiteral),
        ["src/main/java/shop/Main.java:10"]
    );
}

#[test]
fn substrings_and_plain_strings_are_not_references() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("a.py"),
        "def calc_total():\n    pass\n\nx = \"calc_total\"\ny = \"calc_total_2\"\n# calc_total is used\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("cfg.toml"),
        "k = \"a:calc_total\"\nother = \"calc_total_2\"\nsame = \"mycalc_total\"\n",
    )
    .unwrap();
    let idx = build_index(
        tmp.path(),
        &IndexOptions::default(),
        None,
        &CancellationToken::new(),
    )
    .unwrap();
    let f = only(&idx, "calc_total", SymbolKind::Function);
    // A bare equal string outside a lookup call is only a *possible* reference.
    assert_eq!(refs_by_kind(&idx, f, RefKind::StringLiteral), ["a.py:4?"]);
    assert_eq!(refs_by_kind(&idx, f, RefKind::ConfigFile), ["cfg.toml:1"]);
    assert_eq!(refs_by_kind(&idx, f, RefKind::Comment), ["a.py:6"]);
}
