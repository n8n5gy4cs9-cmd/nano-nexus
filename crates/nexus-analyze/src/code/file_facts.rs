//! Per-file facts: definitions with their scope, identifier occurrences, imports, string literals.
//! Computed from one parse and cached by content hash, so re-indexing only touches changed files.

use serde::{Deserialize, Serialize};
use tree_sitter::Node;

use super::lang::Lang;
use super::model::SymbolKind;
use super::tags::{self, TagKind};

/// A span of text in a file (byte offsets into the UTF-8 source).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub line: u32,
}

/// One definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefFact {
    pub kind: SymbolKind,
    pub name: String,
    pub name_span: Span,
    /// The whole definition (for previews and containment).
    pub node: (u32, u32),
    /// Enclosing container names, outermost first (`["Cart", "total"]`).
    pub scope: Vec<String>,
    /// Range in which a local (variable/parameter) is visible; `None` for members and top-level items.
    pub visible: Option<(u32, u32)>,
    pub signature: String,
    pub doc: Option<String>,
    /// True when the syntax says it overrides/implements something (`override`, `@Override`, trait impl).
    pub overrides: bool,
    /// A test attribute/annotation is on it (`#[test]`, `@Test`, `[Fact]`).
    #[serde(default)]
    pub test_attr: bool,
    /// Visibility text found in the syntax: `pub`, `public`, `export`, `private`, …
    pub visibility: Option<String>,
}

/// An identifier occurrence that is not a definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefFact {
    pub name: String,
    pub span: Span,
    /// Name after a `.` (member access).
    pub member: bool,
    /// For member access: the text of the receiver (`self`, `pricing`, `items[0]`…), when it is simple.
    pub object: Option<String>,
    /// From an import statement.
    pub import: bool,
    /// Enclosing functions/classes of the occurrence, outermost first: (kind, name, start, end).
    pub enclosing: Vec<Encl>,
}

/// An enclosing function or class.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Encl {
    pub class: bool,
    pub name: String,
    pub start: u32,
    pub end: u32,
}

/// A text occurrence with its raw text (imports, strings).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextFact {
    pub span: Span,
    pub text: String,
    /// Strings only: the function the literal is an argument of (`getattr`), or `[]` for an index.
    #[serde(default)]
    pub ctx: Option<String>,
}

/// One import statement: its module specifier and the names it brings in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportFact {
    pub module: TextFact,
    pub names: Vec<TextFact>,
}

/// Everything the index remembers about one parsed file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFacts {
    pub defs: Vec<DefFact>,
    pub refs: Vec<RefFact>,
    pub import_modules: Vec<TextFact>,
    pub import_names: Vec<TextFact>,
    pub imports: Vec<ImportFact>,
    pub strings: Vec<TextFact>,
    pub comments: Vec<TextFact>,
    pub package: Option<String>,
}

fn func_kinds(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Python => &["function_definition", "lambda"],
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx => &[
            "function_declaration",
            "generator_function_declaration",
            "function_expression",
            "function",
            "arrow_function",
            "method_definition",
        ],
        Lang::Rust => &["function_item", "closure_expression"],
        Lang::CSharp => &[
            "method_declaration",
            "constructor_declaration",
            "local_function_statement",
            "lambda_expression",
            "accessor_declaration",
        ],
        Lang::Go => &["function_declaration", "method_declaration", "func_literal"],
        Lang::Java => &[
            "method_declaration",
            "constructor_declaration",
            "lambda_expression",
        ],
    }
}

fn class_kinds(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Python => &["class_definition"],
        Lang::JavaScript => &["class_declaration", "class"],
        Lang::TypeScript | Lang::Tsx => &[
            "class_declaration",
            "class",
            "abstract_class_declaration",
            "interface_declaration",
            "enum_declaration",
        ],
        Lang::Rust => &[
            "impl_item",
            "trait_item",
            "struct_item",
            "enum_item",
            "union_item",
        ],
        Lang::CSharp => &[
            "class_declaration",
            "struct_declaration",
            "interface_declaration",
            "enum_declaration",
            "record_declaration",
        ],
        Lang::Go => &["type_spec"],
        Lang::Java => &[
            "class_declaration",
            "interface_declaration",
            "enum_declaration",
            "record_declaration",
        ],
    }
}

fn ns_kinds(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Rust => &["mod_item"],
        Lang::CSharp => &["namespace_declaration", "file_scoped_namespace_declaration"],
        _ => &[],
    }
}

fn text<'a>(n: Node<'_>, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[n.byte_range()]).unwrap_or("")
}

fn line_of(n: Node<'_>) -> u32 {
    n.start_position().row as u32 + 1
}

fn span_of(n: Node<'_>) -> Span {
    Span {
        start: n.start_byte() as u32,
        end: n.end_byte() as u32,
        line: line_of(n),
    }
}

/// Name of a container node (class, function, impl, namespace).
fn container_name(n: Node<'_>, src: &[u8]) -> String {
    let field = if n.kind() == "impl_item" { "type" } else { "name" };
    let raw = n
        .child_by_field_name(field)
        .map(|c| text(c, src).to_owned())
        .unwrap_or_default();
    // `Foo<T>` -> `Foo`
    raw.split('<').next().unwrap_or("").trim().to_owned()
}

/// Enclosing functions/classes/namespaces of `node`, outermost first.
fn enclosing(node: Node<'_>, lang: Lang, src: &[u8]) -> Vec<(Kind, String, u32, u32)> {
    let (fk, ck, nk) = (func_kinds(lang), class_kinds(lang), ns_kinds(lang));
    let mut out = Vec::new();
    let mut cur = node.parent();
    while let Some(n) = cur {
        let k = n.kind();
        let kind = if fk.contains(&k) {
            Some(Kind::Func)
        } else if ck.contains(&k) {
            Some(Kind::Class)
        } else if nk.contains(&k) {
            Some(Kind::Ns)
        } else {
            None
        };
        if let Some(kind) = kind {
            out.push((
                kind,
                container_name(n, src),
                n.start_byte() as u32,
                n.end_byte() as u32,
            ));
        }
        cur = n.parent();
    }
    out.reverse();
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Func,
    Class,
    Ns,
}

fn is_all_caps(name: &str) -> bool {
    name.chars().any(|c| c.is_ascii_uppercase())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// First line of the definition, trimmed.
fn signature(n: Node<'_>, src: &[u8]) -> String {
    let t = text(n, src);
    let line = t.lines().next().unwrap_or("").trim();
    let line = line.trim_end_matches(['{', ':']).trim();
    line.chars().take(160).collect()
}

fn is_comment(n: Node<'_>) -> bool {
    n.kind().contains("comment")
}

/// The comment right above a definition, or a Python docstring.
fn doc_for(def_node: Node<'_>, lang: Lang, src: &[u8]) -> Option<String> {
    if lang == Lang::Python
        && let Some(body) = def_node.child_by_field_name("body")
        && let Some(first) = body.named_child(0)
        && first.kind() == "expression_statement"
        && let Some(s) = first.named_child(0)
        && s.kind() == "string"
    {
        let t = text(s, src).trim_matches(['"', '\'']).trim();
        return (!t.is_empty()).then(|| t.chars().take(300).collect());
    }
    // A decorator/export wrapper sits between the comment and the definition.
    let mut start = def_node;
    while let Some(p) = start.parent() {
        if matches!(
            p.kind(),
            "export_statement" | "decorated_definition" | "attribute_item"
        ) {
            start = p;
        } else {
            break;
        }
    }
    let mut lines = Vec::new();
    let mut want_row = start.start_position().row;
    let mut prev = start.prev_sibling();
    while let Some(p) = prev {
        if is_comment(p) && p.end_position().row + 1 >= want_row {
            lines.push(text(p, src).trim().to_owned());
            want_row = p.start_position().row;
            prev = p.prev_sibling();
        } else {
            break;
        }
    }
    lines.reverse();
    let joined = lines.join(" ");
    (!joined.is_empty()).then(|| joined.chars().take(300).collect())
}

const SELF_NAMES: &[&str] = &["self", "cls", "this", "Self", "base", "super"];

/// The receiver of a member access whose member identifier is `member`.
fn receiver(member: Node<'_>, src: &[u8]) -> Option<String> {
    let parent = member.parent()?;
    for field in [
        "object",
        "value",
        "operand",
        "expression",
        "scope",
        "path",
        "qualifier",
    ] {
        if let Some(r) = parent.child_by_field_name(field)
            && r.id() != member.id()
        {
            let t = text(r, src);
            return (t.len() <= 80 && !t.contains('\n')).then(|| t.to_owned());
        }
    }
    None
}

/// Visibility keyword among the direct modifier children of `n`.
fn modifiers_of(n: Node<'_>, src: &[u8]) -> Option<String> {
    let mut found = None;
    let mut c = n.walk();
    for ch in n.children(&mut c) {
        if matches!(
            ch.kind(),
            "visibility_modifier" | "modifier" | "modifiers" | "accessibility_modifier"
        ) {
            let t = text(ch, src);
            for w in ["pub", "public", "private", "protected", "internal"] {
                if t.split_whitespace()
                    .any(|x| x == w || x.starts_with(&format!("{w}(")))
                {
                    found = Some(w.to_owned());
                }
            }
        }
    }
    found
}

fn visibility_of(def: Node<'_>, src: &[u8]) -> Option<String> {
    if let Some(v) = modifiers_of(def, src) {
        return Some(v);
    }
    let p = def.parent()?;
    if p.kind() == "export_statement" {
        return Some("export".into());
    }
    // C#/Java field declarators keep modifiers on the declaration above.
    p.parent()
        .and_then(|gp| modifiers_of(gp, src))
        .or_else(|| modifiers_of(p, src))
}

fn has_override(def: Node<'_>, lang: Lang, src: &[u8]) -> bool {
    let t = text(def, src);
    let head = t.lines().next().unwrap_or("");
    match lang {
        Lang::CSharp => head.split_whitespace().any(|w| w == "override"),
        Lang::Java => {
            // `@Override` is a sibling annotation inside `modifiers`.
            let mut c = def.walk();
            def.children(&mut c)
                .any(|ch| ch.kind() == "modifiers" && text(ch, src).contains("@Override"))
        }
        Lang::TypeScript | Lang::Tsx => head.split_whitespace().any(|w| w == "override"),
        Lang::Rust => {
            // A method inside `impl Trait for Type` implements the trait.
            let mut cur = def.parent();
            while let Some(n) = cur {
                if n.kind() == "impl_item" {
                    return n.child_by_field_name("trait").is_some();
                }
                if n.kind() == "trait_item" {
                    return false;
                }
                cur = n.parent();
            }
            false
        }
        _ => false,
    }
}

fn has_test_attr(def: Node<'_>, lang: Lang, src: &[u8]) -> bool {
    let words = [
        "test",
        "Test",
        "Fact",
        "Theory",
        "TestMethod",
        "TestCase",
        "ParameterizedTest",
        "RepeatedTest",
    ];
    let has = |t: &str| {
        t.split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .any(|w| words.contains(&w))
    };
    match lang {
        Lang::Rust => {
            let mut prev = def.prev_sibling();
            while let Some(p) = prev {
                match p.kind() {
                    "attribute_item" => {
                        if has(text(p, src)) {
                            return true;
                        }
                    }
                    k if k.contains("comment") => {}
                    _ => break,
                }
                prev = p.prev_sibling();
            }
            false
        }
        Lang::Java => {
            let mut c = def.walk();
            def.children(&mut c)
                .any(|ch| ch.kind() == "modifiers" && has(text(ch, src)))
        }
        Lang::CSharp => {
            let mut c = def.walk();
            def.children(&mut c)
                .any(|ch| ch.kind() == "attribute_list" && has(text(ch, src)))
        }
        _ => false,
    }
}

/// Parse and extract the facts of one file. `None` if the grammar or query fails.
pub fn extract(lang: Lang, src: &[u8]) -> Option<FileFacts> {
    let tree = lang.parse(src)?;
    let root = tree.root_node();
    let tags = tags::run(lang, &tree, src).ok()?;

    let mut defs: Vec<DefFact> = Vec::new();
    let mut def_names = std::collections::HashSet::new();
    for t in tags.iter().filter(|t| matches!(t.kind, TagKind::Def(_))) {
        let TagKind::Def(mut kind) = t.kind else { continue };
        let Some(name_node) = root.descendant_for_byte_range(t.name.start, t.name.end) else {
            continue;
        };
        let name = text(name_node, src).to_owned();
        if name.is_empty() || !def_names.insert((t.name.start, t.name.end)) {
            continue;
        }
        let def_node = root
            .descendant_for_byte_range(t.node.start, t.node.end)
            .unwrap_or(name_node);
        let mut def_node = def_node;
        // The smallest node can be a child of the definition when ranges coincide; climb to the node
        // with exactly the captured range.
        while def_node.byte_range() != t.node
            && let Some(p) = def_node.parent()
        {
            def_node = p;
        }
        let encl = enclosing(def_node, lang, src);
        let innermost = encl.iter().rev().find(|e| e.0 != Kind::Ns);
        let in_class = innermost.is_some_and(|e| e.0 == Kind::Class);
        let in_func = innermost.is_some_and(|e| e.0 == Kind::Func);

        // Kind refinement.
        match kind {
            SymbolKind::Function if in_class => kind = SymbolKind::Method,
            SymbolKind::Variable if in_class => kind = SymbolKind::Field,
            SymbolKind::Class if lang == Lang::Go => {
                if def_node
                    .child_by_field_name("type")
                    .is_some_and(|c| c.kind() == "interface_type")
                {
                    kind = SymbolKind::Interface;
                }
            }
            SymbolKind::Parameter
                if matches!(lang, Lang::TypeScript | Lang::Tsx)
                    && text(def_node, src)
                        .split_whitespace()
                        .next()
                        .is_some_and(|w| matches!(w, "public" | "private" | "protected" | "readonly")) =>
            {
                kind = SymbolKind::Field;
            }
            _ => {}
        }
        if kind == SymbolKind::Variable && !in_func {
            let head = text(def_node.parent().unwrap_or(def_node), src);
            let decl: String = head.chars().take(60).collect();
            let const_decl = match lang {
                Lang::CSharp => decl.contains("const ") || decl.contains("static readonly"),
                Lang::Java => decl.contains("final"),
                Lang::JavaScript | Lang::TypeScript | Lang::Tsx => {
                    decl.trim_start().starts_with("const") && is_all_caps(&name)
                }
                _ => false,
            };
            if const_decl || is_all_caps(&name) {
                kind = SymbolKind::Constant;
            }
        }
        if kind == SymbolKind::Field && in_class && is_all_caps(&name) && lang != Lang::Rust {
            kind = SymbolKind::Constant;
        }

        // Scope: names of enclosing containers. Python `self.x` fields belong to the class.
        let mut scope: Vec<String> = encl
            .iter()
            .map(|e| e.1.clone())
            .filter(|n| !n.is_empty())
            .collect();
        let mut visible = None;
        if lang == Lang::Python && kind == SymbolKind::Field {
            let last_class = encl.iter().rposition(|e| e.0 == Kind::Class);
            if let Some(i) = last_class {
                scope = encl[..=i]
                    .iter()
                    .map(|e| e.1.clone())
                    .filter(|n| !n.is_empty())
                    .collect();
            }
        } else if matches!(
            kind,
            SymbolKind::Variable | SymbolKind::Parameter | SymbolKind::Constant
        ) && in_func
        {
            visible = innermost.map(|e| (e.2, e.3));
        }
        // A function's own parameters live in the function's scope, not the outer one.
        let line = line_of(name_node);
        defs.push(DefFact {
            kind,
            name,
            name_span: Span {
                start: t.name.start as u32,
                end: t.name.end as u32,
                line,
            },
            node: (t.node.start as u32, t.node.end as u32),
            scope,
            visible,
            signature: signature(def_node, src),
            doc: match kind {
                SymbolKind::Variable | SymbolKind::Parameter => None,
                _ => doc_for(def_node, lang, src),
            },
            test_attr: has_test_attr(def_node, lang, src),
            overrides: matches!(
                kind,
                SymbolKind::Method | SymbolKind::Function | SymbolKind::Field
            ) && has_override(def_node, lang, src),
            visibility: visibility_of(def_node, src),
        });
    }
    defs.sort_by_key(|d| (d.name_span.start, d.name_span.end));

    // Identifier occurrences (not definitions). Member/import tags beat plain refs on the same range.
    let mut refs: Vec<RefFact> = Vec::new();
    let mut by_range: std::collections::BTreeMap<(usize, usize), (u8, RefFact)> =
        std::collections::BTreeMap::new();
    for t in &tags {
        let (rank, member, import) = match t.kind {
            TagKind::Ref => (1u8, false, false),
            TagKind::Member => (2, true, false),
            TagKind::ImportName => (3, false, true),
            _ => continue,
        };
        let key = (t.name.start, t.name.end);
        if def_names.contains(&key) {
            continue;
        }
        if by_range.get(&key).is_some_and(|(r, _)| *r >= rank) {
            continue;
        }
        let Some(n) = root.descendant_for_byte_range(t.name.start, t.name.end) else {
            continue;
        };
        let obj = if member { receiver(n, src) } else { None };
        let encl: Vec<Encl> = enclosing(n, lang, src)
            .into_iter()
            .filter(|e| e.0 != Kind::Ns)
            .map(|e| Encl {
                class: e.0 == Kind::Class,
                name: e.1,
                start: e.2,
                end: e.3,
            })
            .collect();
        by_range.insert(
            key,
            (
                rank,
                RefFact {
                    name: text(n, src).to_owned(),
                    span: span_of(n),
                    member,
                    object: obj,
                    import,
                    enclosing: encl,
                },
            ),
        );
    }
    refs.extend(by_range.into_values().map(|(_, r)| r));

    let texts = |kind: TagKind| -> Vec<TextFact> {
        tags.iter()
            .filter(|t| t.kind == kind)
            .filter_map(|t| {
                let n = root.descendant_for_byte_range(t.name.start, t.name.end)?;
                Some(TextFact {
                    span: span_of(n),
                    text: text(n, src).to_owned(),
                    ctx: if kind == TagKind::Str {
                        string_ctx(n, src)
                    } else {
                        None
                    },
                })
            })
            .collect()
    };
    let import_modules = texts(TagKind::ImportModule);
    let import_names = texts(TagKind::ImportName);
    let imports = group_imports(lang, root, &import_modules, &import_names);
    let strings = texts(TagKind::Str);
    let mut comments = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if is_comment(n) {
            comments.push(TextFact {
                span: span_of(n),
                text: text(n, src).to_owned(),
                ctx: None,
            });
            continue;
        }
        let mut c = n.walk();
        stack.extend(n.children(&mut c));
    }
    comments.sort_by_key(|c| c.span.start);

    let package = package_of(lang, root, src);
    Some(FileFacts {
        defs,
        refs,
        import_modules,
        import_names,
        imports,
        strings,
        comments,
        package,
    })
}

/// The call (or index) a string literal is an argument of.
fn string_ctx(n: Node<'_>, src: &[u8]) -> Option<String> {
    let mut cur = n.parent();
    for _ in 0..4 {
        let p = cur?;
        match p.kind() {
            "subscript" | "subscript_expression" | "element_access_expression" | "index_expression" => {
                return Some("[]".into());
            }
            "call"
            | "call_expression"
            | "invocation_expression"
            | "method_invocation"
            | "macro_invocation" => {
                let f = p
                    .child_by_field_name("function")
                    .or_else(|| p.child_by_field_name("name"))
                    .or_else(|| p.child_by_field_name("macro"))?;
                let t = text(f, src);
                let last = t.rsplit(['.', ':', '>']).next().unwrap_or(t);
                return Some(last.to_owned());
            }
            "assignment" | "expression_statement" | "variable_declarator" | "return_statement" | "pair" => {
                return None;
            }
            _ => cur = p.parent(),
        }
    }
    None
}

/// Attach each imported name to the statement of its module specifier.
fn group_imports(lang: Lang, root: Node<'_>, modules: &[TextFact], names: &[TextFact]) -> Vec<ImportFact> {
    let stmt_kinds: &[&str] = match lang {
        Lang::Python => &["import_statement", "import_from_statement"],
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx => {
            &["import_statement", "export_statement", "call_expression"]
        }
        Lang::Rust => &["use_declaration", "mod_item"],
        Lang::Go => &["import_spec"],
        Lang::Java => &["import_declaration", "package_declaration"],
        Lang::CSharp => &["using_directive"],
    };
    modules
        .iter()
        .map(|m| {
            let range = root
                .descendant_for_byte_range(m.span.start as usize, m.span.end as usize)
                .and_then(|mut n| {
                    loop {
                        if stmt_kinds.contains(&n.kind()) {
                            break Some((n.start_byte() as u32, n.end_byte() as u32));
                        }
                        n = n.parent()?;
                    }
                })
                .unwrap_or((m.span.start, m.span.end));
            ImportFact {
                module: m.clone(),
                names: names
                    .iter()
                    .filter(|n| n.span.start >= range.0 && n.span.end <= range.1)
                    .cloned()
                    .collect(),
            }
        })
        .collect()
}

/// `package x.y;` (Java), `package main` (Go), `namespace A.B` (C#): the package/namespace text.
fn package_of(lang: Lang, root: Node<'_>, src: &[u8]) -> Option<String> {
    let mut c = root.walk();
    for ch in root.named_children(&mut c) {
        match (lang, ch.kind()) {
            (Lang::Java, "package_declaration") => {
                return Some(
                    text(ch, src)
                        .trim_start_matches("package")
                        .trim()
                        .trim_end_matches(';')
                        .trim()
                        .to_owned(),
                );
            }
            (Lang::Go, "package_clause") => {
                return Some(text(ch, src).trim_start_matches("package").trim().to_owned());
            }
            (Lang::CSharp, "namespace_declaration" | "file_scoped_namespace_declaration") => {
                return ch.child_by_field_name("name").map(|n| text(n, src).to_owned());
            }
            _ => {}
        }
    }
    None
}

/// Receiver names that mean "the current instance".
pub fn is_self_name(s: &str) -> bool {
    SELF_NAMES.contains(&s)
}
