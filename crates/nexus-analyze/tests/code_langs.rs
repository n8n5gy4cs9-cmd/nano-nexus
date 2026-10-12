#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The P1 languages (T09.18): Kotlin, Swift, PHP, Ruby, C, C++ and Lua each index their fixture project:
//! definitions, cross-file references, import resolution and reserved names.

use std::path::{Path, PathBuf};

use nexus_analyze::code::index::{CodeIndex, Flags, IndexOptions, RefKind, build_index};
use nexus_analyze::code::{Symbol, SymbolKind};
use tokio_util::sync::CancellationToken;

fn index(name: &str) -> CodeIndex {
    let root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    build_index(&root, &IndexOptions::default(), None, &CancellationToken::new()).unwrap()
}

fn find<'a>(idx: &'a CodeIndex, name: &str, kind: SymbolKind) -> Vec<&'a Symbol> {
    idx.symbols
        .iter()
        .filter(|s| s.name == name && s.kind == kind)
        .collect()
}

/// Paths of the files that hold a code or import reference to the symbol.
fn files_of(idx: &CodeIndex, s: &Symbol) -> Vec<String> {
    let mut v: Vec<String> = s
        .refs
        .iter()
        .filter(|r| matches!(r.kind, RefKind::Code | RefKind::Import))
        .map(|r| idx.path_of(r.loc.file).unwrap().to_owned())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// The symbol is defined once and referenced from the listed files.
fn assert_used_across(fixture: &str, name: &str, kind: SymbolKind, expect: &[&str]) {
    let idx = index(fixture);
    let found = find(&idx, name, kind);
    assert_eq!(found.len(), 1, "{fixture}: {kind:?} {name} x{}", found.len());
    assert_eq!(files_of(&idx, found[0]), expect, "{fixture}: {name}");
    assert!(found[0].refs.iter().any(|r| r.is_def));
}

#[test]
fn kotlin_function_and_class() {
    assert_used_across(
        "kt_app",
        "calcTotal",
        SymbolKind::Function,
        &["src/shop/Main.kt", "src/shop/Pricing.kt"],
    );
    let idx = index("kt_app");
    assert_eq!(find(&idx, "cart_item", SymbolKind::Class).len(), 1);
    let main = find(&idx, "main", SymbolKind::Function);
    assert!(main[0].flags.has(Flags::ENTRY_POINT) && main[0].flags.has(Flags::RESERVED));
}

#[test]
fn swift_function_and_struct() {
    assert_used_across(
        "swift_app",
        "calcTotal",
        SymbolKind::Function,
        &["Sources/Pricing.swift", "Sources/main.swift"],
    );
    let idx = index("swift_app");
    assert_eq!(find(&idx, "cart_item", SymbolKind::Class).len(), 1);
}

#[test]
fn php_function_class_and_require() {
    assert_used_across(
        "php_app",
        "calcTotal",
        SymbolKind::Function,
        &["main.php", "src/pricing.php"],
    );
    let idx = index("php_app");
    assert_eq!(find(&idx, "cart_item", SymbolKind::Class).len(), 1);
    let ctor = find(&idx, "__construct", SymbolKind::Method);
    assert!(ctor[0].flags.has(Flags::RESERVED));
    let file = find(&idx, "pricing", SymbolKind::File);
    assert!(
        file[0].refs.iter().any(|r| r.kind == RefKind::Import),
        "require_once must reference the file"
    );
}

#[test]
fn ruby_function_class_and_require_relative() {
    assert_used_across(
        "rb_app",
        "calc_total",
        SymbolKind::Method,
        &["lib/pricing.rb", "main.rb"],
    );
    let idx = index("rb_app");
    assert_eq!(find(&idx, "CartItem", SymbolKind::Class).len(), 1);
    assert!(
        find(&idx, "initialize", SymbolKind::Method)[0]
            .flags
            .has(Flags::RESERVED)
    );
    let file = find(&idx, "pricing", SymbolKind::File);
    assert!(file[0].refs.iter().any(|r| r.kind == RefKind::Import));
}

#[test]
fn c_function_struct_and_include() {
    assert_used_across(
        "c_app",
        "calc_total",
        SymbolKind::Function,
        &["main.c", "util/pricing.c", "util/pricing.h"],
    );
    let idx = index("c_app");
    assert!(
        find(&idx, "main", SymbolKind::Function)[0]
            .flags
            .has(Flags::ENTRY_POINT)
    );
    let header = find(&idx, "pricing", SymbolKind::File);
    assert!(
        header
            .iter()
            .any(|f| f.refs.iter().any(|r| r.kind == RefKind::Import)),
        "#include \"util/pricing.h\" must reference the header"
    );
}

#[test]
fn cpp_method_namespace_and_include() {
    let idx = index("cpp_app");
    let m = find(&idx, "calcTotal", SymbolKind::Method);
    assert!(!m.is_empty());
    let places: Vec<String> = m.iter().flat_map(|s| files_of(&idx, s)).collect();
    assert!(places.iter().any(|p| p == "main.cpp"), "{places:?}");
    assert!(places.iter().any(|p| p == "util/pricing.cpp"), "{places:?}");
    assert!(!find(&idx, "shop", SymbolKind::Module).is_empty());
    assert!(
        find(&idx, "main", SymbolKind::Function)[0]
            .flags
            .has(Flags::ENTRY_POINT)
    );
}

#[test]
fn lua_function_and_require() {
    let idx = index("lua_app");
    let f = find(&idx, "calc_total", SymbolKind::Method);
    assert_eq!(f.len(), 1);
    assert_eq!(files_of(&idx, f[0]), ["main.lua", "util/pricing.lua"]);
    let file = find(&idx, "pricing", SymbolKind::File);
    assert!(file[0].refs.iter().any(|r| r.kind == RefKind::Import));
}
