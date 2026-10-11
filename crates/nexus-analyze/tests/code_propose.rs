#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The proposal validator and `<sym>` blocks (T09.09).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nexus_analyze::code::index::{IndexOptions, build_index};
use nexus_analyze::code::propose::{Sources, Validator, render_sym};
use nexus_analyze::code::rules::{RuleOptions, detect};
use nexus_analyze::code::{CodeIndex, SymbolKind};
use tokio_util::sync::CancellationToken;

fn index(name: &str) -> CodeIndex {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    build_index(&p, &IndexOptions::default(), None, &CancellationToken::new()).unwrap()
}

fn id(idx: &CodeIndex, name: &str, kind: SymbolKind) -> nexus_analyze::code::SymbolId {
    idx.symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap()
        .id
}

#[test]
fn validator_rules() {
    let idx = index("py_app");
    let v = Validator::new(&idx);
    let none = HashMap::new();
    let m = id(&idx, "doStuff2", SymbolKind::Method);
    assert!(v.check(m, "apply_discount", &none).is_ok());
    assert!(v.check(m, "doStuff2", &none).unwrap_err().contains("same"));
    assert!(v.check(m, "lambda", &none).unwrap_err().contains("keyword"));
    assert!(v.check(m, "print", &none).unwrap_err().contains("builtin"));
    assert!(
        v.check(m, "applyDiscount", &none)
            .unwrap_err()
            .contains("snake_case")
    );
    assert!(
        v.check(m, "price", &none).unwrap_err().contains("already exists"),
        "a field of the same class"
    );
    // The same name in another scope is fine.
    assert!(v.check(m, "fmt_money", &none).is_ok());

    // Top level: a new function name may not equal another top-level name of the file.
    let f = id(&idx, "calc_total", SymbolKind::Function);
    assert!(v.check(f, "TAX_RATE", &none).is_err());
    assert!(v.check(f, "compute_total", &none).is_ok());
    // Names taken by renames accepted earlier count as existing.
    let mut taken = HashMap::new();
    taken.insert(m, "compute_total".to_owned());
    assert!(
        v.check(f, "compute_total", &taken).is_ok(),
        "different scopes (method vs module function)"
    );

    // Locals must not hide a module-level name or an import.
    let tmp = id(&idx, "tmp", SymbolKind::Variable);
    assert!(v.check(tmp, "calc_total", &none).unwrap_err().contains("hide"));
    assert!(v.check(tmp, "scaled", &none).is_ok());
}

#[test]
fn file_and_folder_validation() {
    let idx = index("py_app");
    let v = Validator::new(&idx);
    let none = HashMap::new();
    let f = id(&idx, "Helper_Funcs", SymbolKind::File);
    assert!(v.check(f, "helper_funcs", &none).is_ok());
    assert!(
        v.check(f, "pricing", &none).is_ok(),
        "pricing.py lives in util/, not next to main.py"
    );
    assert!(v.check(f, "main", &none).unwrap_err().contains("already exists"));
    assert!(v.check(f, "Main", &none).unwrap_err().contains("snake_case"));
    assert!(v.check(f, "con", &none).unwrap_err().contains("Windows"));
    assert!(v.check(f, "a/b", &none).is_err());
    let d = id(&idx, "util", SymbolKind::Folder);
    assert!(v.check(d, "tests", &none).unwrap_err().contains("already exists"));
    assert!(v.check(d, "pricing_tools", &none).is_ok());
    // Case-insensitive sibling check: TeSts is the same folder on macOS/Windows.
    assert!(v.check(d, "teSts", &none).is_err());
}

#[test]
fn sym_blocks_show_signature_body_and_usages() {
    let idx = index("py_app");
    let src = Sources::new(&idx);
    let cands = detect(&idx, &RuleOptions::default());
    let c = cands
        .iter()
        .find(|c| idx.symbol(c.symbol).unwrap().name == "doStuff2")
        .unwrap();
    let block = render_sym(&idx, &src, c, "s7");
    assert!(block.starts_with(
        "<sym id=\"s7\" kind=\"method\" lang=\"python\" convention=\"snake_case\" file=\"util/pricing.py\""
    ));
    assert!(block.contains("def doStuff2(self, p)") && block.contains("tmp = self.price * p"));
    assert!(block.ends_with("</sym>"));
    assert!(block.chars().count() <= 430);
    // Files show what they define and who imports them.
    let c = cands
        .iter()
        .find(|c| idx.symbol(c.symbol).unwrap().name == "Helper_Funcs")
        .unwrap();
    let block = render_sym(&idx, &src, c, "s1");
    assert!(
        block.contains("kind=\"file\"")
            && block.contains("defines: fmt_money")
            && block.contains("imported by: main.py")
    );
}
