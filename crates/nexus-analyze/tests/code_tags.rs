#![allow(clippy::unwrap_used, clippy::expect_used)]
//! S-expression and capture snapshots of the fixture sources (one test per language, T09.03).
//! A grammar or query change shows up as a snapshot diff. Review it, then `cargo insta accept`.

use std::path::{Path, PathBuf};

use nexus_analyze::code::Lang;
use nexus_analyze::code::tags::{self, TagKind};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn line_of(src: &[u8], byte: usize) -> usize {
    src[..byte].iter().filter(|&&b| b == b'\n').count() + 1
}

fn kind_label(k: TagKind) -> String {
    match k {
        TagKind::Def(s) => format!("def.{}", s.label()),
        TagKind::Ref => "ref".into(),
        TagKind::Member => "member".into(),
        TagKind::ImportModule => "import.module".into(),
        TagKind::ImportName => "import.name".into(),
        TagKind::Str => "string".into(),
    }
}

/// `L<line> <kind> <text>` for every tag, preceded by the tree's S-expression.
fn dump(rel: &str) -> String {
    let path = fixtures().join(rel);
    let src = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let lang = Lang::from_path(&path).unwrap();
    let tree = lang.parse(&src).unwrap();
    let mut out = format!("{}\n\n", tree.root_node().to_sexp());
    for t in tags::run(lang, &tree, &src).unwrap() {
        let text = String::from_utf8_lossy(&src[t.name.clone()]).replace('\n', "⏎");
        let text: String = text.chars().take(40).collect();
        out.push_str(&format!(
            "L{} {} {}\n",
            line_of(&src, t.name.start),
            kind_label(t.kind),
            text
        ));
    }
    out
}

macro_rules! snap {
    ($name:ident, $rel:expr) => {
        #[test]
        fn $name() {
            insta::assert_snapshot!(dump($rel));
        }
    };
}

snap!(python_pricing, "py_app/util/pricing.py");
snap!(python_main, "py_app/main.py");
snap!(typescript_pricing, "ts_app/src/lib/pricing.ts");
snap!(typescript_index, "ts_app/src/index.ts");
snap!(tsx_button, "ts_app/src/ui/Button.tsx");
snap!(rust_pricing, "rust_app/src/pricing.rs");
snap!(rust_main, "rust_app/src/main.rs");
snap!(csharp_calc, "cs_app/Pricing/calc_helpers.cs");
snap!(csharp_program, "cs_app/Program.cs");
snap!(go_pricing, "go_app/pricing/pricing.go");
snap!(go_main, "go_app/main.go");
snap!(
    java_cart_item,
    "java_app/src/main/java/shop/pricing/cart_item.java"
);
snap!(java_main, "java_app/src/main/java/shop/Main.java");
