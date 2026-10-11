//! Language detection and parser setup (spike S4: grammar pins in the workspace `Cargo.toml`).

use std::path::Path;

use serde::{Deserialize, Serialize};
use tree_sitter::{Language, Parser, Tree};

/// A source language Rename Forge understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Rust,
    CSharp,
    Go,
    Java,
}

impl Lang {
    pub const ALL: [Lang; 8] = [
        Lang::Python,
        Lang::JavaScript,
        Lang::TypeScript,
        Lang::Tsx,
        Lang::Rust,
        Lang::CSharp,
        Lang::Go,
        Lang::Java,
    ];

    /// Language of a file by its extension (`jsx` parses with the JavaScript grammar, which includes JSX).
    pub fn from_path(path: &Path) -> Option<Lang> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "py" | "pyi" => Lang::Python,
            "js" | "mjs" | "cjs" | "jsx" => Lang::JavaScript,
            "ts" | "mts" | "cts" => Lang::TypeScript,
            "tsx" => Lang::Tsx,
            "rs" => Lang::Rust,
            "cs" => Lang::CSharp,
            "go" => Lang::Go,
            "java" => Lang::Java,
            _ => return None,
        })
    }

    /// Stable lowercase name (used for the queries folder and the UI).
    pub fn name(self) -> &'static str {
        match self {
            Lang::Python => "python",
            Lang::JavaScript => "javascript",
            Lang::TypeScript => "typescript",
            Lang::Tsx => "tsx",
            Lang::Rust => "rust",
            Lang::CSharp => "csharp",
            Lang::Go => "go",
            Lang::Java => "java",
        }
    }

    /// The tree-sitter grammar.
    pub fn grammar(self) -> Language {
        match self {
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            Lang::Go => tree_sitter_go::LANGUAGE.into(),
            Lang::Java => tree_sitter_java::LANGUAGE.into(),
        }
    }

    /// Parse `source` (UTF-8 bytes). `None` only if the grammar cannot be loaded.
    pub fn parse(self, source: &[u8]) -> Option<Tree> {
        let mut parser = Parser::new();
        parser.set_language(&self.grammar()).ok()?;
        parser.parse(source, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLES: [(Lang, &str); 8] = [
        (
            Lang::Python,
            "import os\nclass A:\n    def f(self, x):\n        return x + 1\n",
        ),
        (
            Lang::JavaScript,
            "import {a} from './a.js';\nexport function f(x) { return <div>{x}</div>; }\n",
        ),
        (
            Lang::TypeScript,
            "import {a} from './a';\nexport interface I { n: number }\nexport const f = (x: I): number => x.n;\n",
        ),
        (Lang::Tsx, "export const C = ({n}: {n: number}) => <b>{n}</b>;\n"),
        (
            Lang::Rust,
            "mod m;\nuse crate::m::f;\npub struct S { pub n: u32 }\nimpl S { pub fn get(&self) -> u32 { self.n } }\n",
        ),
        (
            Lang::CSharp,
            "using System;\nnamespace N { public class C { public int F(int x) => x; } }\n",
        ),
        (
            Lang::Go,
            "package main\nimport \"fmt\"\nfunc F(x int) int { fmt.Println(x); return x }\n",
        ),
        (
            Lang::Java,
            "package p;\nimport java.util.List;\npublic class C { int f(int x) { return x; } }\n",
        ),
    ];

    #[test]
    fn every_grammar_loads_and_parses_without_errors() {
        for (lang, src) in SAMPLES {
            let tree = lang
                .parse(src.as_bytes())
                .unwrap_or_else(|| panic!("{} did not load", lang.name()));
            assert!(
                !tree.root_node().has_error(),
                "{} sample has parse errors",
                lang.name()
            );
        }
    }

    #[test]
    fn detects_languages_by_extension() {
        assert_eq!(Lang::from_path(Path::new("a/b.PY")), Some(Lang::Python));
        assert_eq!(Lang::from_path(Path::new("x.tsx")), Some(Lang::Tsx));
        assert_eq!(Lang::from_path(Path::new("x.jsx")), Some(Lang::JavaScript));
        assert_eq!(Lang::from_path(Path::new("x.txt")), None);
        assert_eq!(Lang::from_path(Path::new("Makefile")), None);
    }
}
