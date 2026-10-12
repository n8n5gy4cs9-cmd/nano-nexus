//! Running the per-language tags queries (`queries/<lang>/tags.scm`).
//!
//! Capture names: `def.<kind>` (the whole definition) with `name` (its identifier), `ref`,
//! `ref.member` (name after a `.`), `import.module`, `import.name`, `string`.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::OnceLock;

use tree_sitter::{Query, QueryCursor, StreamingIterator, Tree};

use super::lang::Lang;
use super::model::SymbolKind;

const PY: &str = include_str!("../../queries/python/tags.scm");
const JS: &str = include_str!("../../queries/javascript/tags.scm");
const TS: &str = include_str!("../../queries/typescript/tags.scm");
const RS: &str = include_str!("../../queries/rust/tags.scm");
const CS: &str = include_str!("../../queries/csharp/tags.scm");
const GO: &str = include_str!("../../queries/go/tags.scm");
const JAVA: &str = include_str!("../../queries/java/tags.scm");
const KOTLIN: &str = include_str!("../../queries/kotlin/tags.scm");
const SWIFT: &str = include_str!("../../queries/swift/tags.scm");
const PHP: &str = include_str!("../../queries/php/tags.scm");
const RUBY: &str = include_str!("../../queries/ruby/tags.scm");
const C: &str = include_str!("../../queries/c/tags.scm");
const CPP: &str = include_str!("../../queries/cpp/tags.scm");
const LUA: &str = include_str!("../../queries/lua/tags.scm");

fn source(lang: Lang) -> &'static str {
    match lang {
        Lang::Python => PY,
        Lang::JavaScript => JS,
        Lang::TypeScript | Lang::Tsx => TS,
        Lang::Rust => RS,
        Lang::CSharp => CS,
        Lang::Go => GO,
        Lang::Java => JAVA,
        Lang::Kotlin => KOTLIN,
        Lang::Swift => SWIFT,
        Lang::Php => PHP,
        Lang::Ruby => RUBY,
        Lang::C => C,
        Lang::Cpp => CPP,
        Lang::Lua => LUA,
    }
}

/// What one capture means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagKind {
    /// A definition. `name` is the identifier range; `node` the whole definition.
    Def(SymbolKind),
    /// Any identifier (also matches definitions and members; the index de-duplicates).
    Ref,
    /// The name after `.`, `->`, `::`-less member access.
    Member,
    /// A module specifier (`"./a"`, `util.pricing`, `crate::x`).
    ImportModule,
    /// An imported name.
    ImportName,
    /// A string literal (whole node, quotes included).
    Str,
}

/// One tag found in a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub kind: TagKind,
    /// The captured node (for a definition: the whole definition).
    pub node: Range<usize>,
    /// For definitions: the identifier. Otherwise equal to `node`.
    pub name: Range<usize>,
}

fn compiled(lang: Lang) -> Result<&'static Query, String> {
    static CACHE: OnceLock<BTreeMap<Lang, Result<Query, String>>> = OnceLock::new();
    let map = CACHE.get_or_init(|| {
        Lang::ALL
            .iter()
            .map(|&l| {
                let q = Query::new(&l.grammar(), source(l)).map_err(|e| format!("{}: {e}", l.name()));
                (l, q)
            })
            .collect()
    });
    match map.get(&lang) {
        Some(Ok(q)) => Ok(q),
        Some(Err(e)) => Err(e.clone()),
        None => Err(format!("no query for {}", lang.name())),
    }
}

/// Run the tags query. Sorted by start byte, then by node size (outer first).
pub fn run(lang: Lang, tree: &Tree, src: &[u8]) -> Result<Vec<Tag>, String> {
    let query = compiled(lang)?;
    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut out = Vec::new();
    let mut matches = cursor.matches(query, tree.root_node(), src);
    while let Some(m) = matches.next() {
        let mut def: Option<(SymbolKind, Range<usize>)> = None;
        let mut name: Option<Range<usize>> = None;
        for c in m.captures() {
            let cname = names.get(c.index as usize).copied().unwrap_or("");
            let range = c.node.byte_range();
            if let Some(kind) = cname.strip_prefix("def.").and_then(SymbolKind::from_capture) {
                def = Some((kind, range));
                continue;
            }
            match cname {
                "name" => name = Some(range),
                "ref" => out.push(Tag {
                    kind: TagKind::Ref,
                    node: range.clone(),
                    name: range,
                }),
                "ref.member" => out.push(Tag {
                    kind: TagKind::Member,
                    node: range.clone(),
                    name: range,
                }),
                "import.module" => out.push(Tag {
                    kind: TagKind::ImportModule,
                    node: range.clone(),
                    name: range,
                }),
                "import.name" => out.push(Tag {
                    kind: TagKind::ImportName,
                    node: range.clone(),
                    name: range,
                }),
                "string" => out.push(Tag {
                    kind: TagKind::Str,
                    node: range.clone(),
                    name: range,
                }),
                _ => {}
            }
        }
        if let (Some((kind, node)), Some(name)) = (def, name) {
            out.push(Tag {
                kind: TagKind::Def(kind),
                node,
                name,
            });
        }
    }
    out.sort_by(|a, b| {
        (a.node.start, std::cmp::Reverse(a.node.end), a.name.start).cmp(&(
            b.node.start,
            std::cmp::Reverse(b.node.end),
            b.name.start,
        ))
    });
    out.dedup();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_query_compiles() {
        for lang in Lang::ALL {
            compiled(lang).unwrap_or_else(|e| panic!("{e}"));
        }
    }
}
