//! Reserved names (ANALYZER §5.4): language keywords and builtins, entry points, dunder/special
//! methods, tests, overrides, and the per-project ignore list. A symbol that matches is flagged
//! `RESERVED` and never proposed for renaming. Keywords and builtins also feed the proposal validator
//! (a new name must not be one).

use std::collections::{HashMap, HashSet};

use super::index::{CodeIndex, Flags, RefKind, SymbolId};
use super::lang::Lang;
use super::model::SymbolKind;

fn list(text: &'static str) -> HashSet<&'static str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

fn keywords_text(lang: Lang) -> &'static str {
    match lang {
        Lang::Python => include_str!("../../queries/python/keywords.txt"),
        Lang::JavaScript => include_str!("../../queries/javascript/keywords.txt"),
        Lang::TypeScript | Lang::Tsx => include_str!("../../queries/typescript/keywords.txt"),
        Lang::Rust => include_str!("../../queries/rust/keywords.txt"),
        Lang::CSharp => include_str!("../../queries/csharp/keywords.txt"),
        Lang::Go => include_str!("../../queries/go/keywords.txt"),
        Lang::Java => include_str!("../../queries/java/keywords.txt"),
        Lang::Kotlin => include_str!("../../queries/kotlin/keywords.txt"),
        Lang::Swift => include_str!("../../queries/swift/keywords.txt"),
        Lang::Php => include_str!("../../queries/php/keywords.txt"),
        Lang::Ruby => include_str!("../../queries/ruby/keywords.txt"),
        Lang::C => include_str!("../../queries/c/keywords.txt"),
        Lang::Cpp => include_str!("../../queries/cpp/keywords.txt"),
        Lang::Lua => include_str!("../../queries/lua/keywords.txt"),
    }
}

fn globals_text(lang: Lang) -> &'static str {
    match lang {
        Lang::Python => include_str!("../../queries/python/globals.txt"),
        Lang::JavaScript => include_str!("../../queries/javascript/globals.txt"),
        Lang::TypeScript | Lang::Tsx => include_str!("../../queries/typescript/globals.txt"),
        Lang::Rust => include_str!("../../queries/rust/globals.txt"),
        Lang::CSharp => include_str!("../../queries/csharp/globals.txt"),
        Lang::Go => include_str!("../../queries/go/globals.txt"),
        Lang::Java => include_str!("../../queries/java/globals.txt"),
        Lang::Kotlin => include_str!("../../queries/kotlin/globals.txt"),
        Lang::Swift => include_str!("../../queries/swift/globals.txt"),
        Lang::Php => include_str!("../../queries/php/globals.txt"),
        Lang::Ruby => include_str!("../../queries/ruby/globals.txt"),
        Lang::C => include_str!("../../queries/c/globals.txt"),
        Lang::Cpp => include_str!("../../queries/cpp/globals.txt"),
        Lang::Lua => include_str!("../../queries/lua/globals.txt"),
    }
}

/// Whether `name` is a keyword of `lang`.
pub fn is_keyword(lang: Lang, name: &str) -> bool {
    list(keywords_text(lang)).contains(name)
}

/// Whether `name` is a builtin or well-known standard-library name of `lang`.
pub fn is_builtin(lang: Lang, name: &str) -> bool {
    list(globals_text(lang)).contains(name)
}

/// Names the language or its runtime calls by name (special methods, lifecycle hooks).
fn special_names(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Python => &[],
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx => &[
            "constructor",
            "toString",
            "valueOf",
            "toJSON",
            "then",
            "catch",
            "finally",
            "render",
            "componentDidMount",
            "componentDidUpdate",
            "componentWillUnmount",
            "shouldComponentUpdate",
            "getDerivedStateFromProps",
            "getSnapshotBeforeUpdate",
            "ngOnInit",
            "ngOnDestroy",
            "ngOnChanges",
        ],
        Lang::Rust => &[
            "fmt",
            "drop",
            "deref",
            "deref_mut",
            "from",
            "into",
            "default",
            "clone",
            "eq",
            "cmp",
            "partial_cmp",
            "next",
            "hash",
            "try_from",
            "as_ref",
            "as_mut",
            "borrow",
            "index",
            "index_mut",
            "add",
            "sub",
            "mul",
            "div",
            "neg",
            "not",
            "from_str",
            "to_string",
            "poll",
            "serialize",
            "deserialize",
        ],
        Lang::CSharp => &[
            "Dispose",
            "ToString",
            "Equals",
            "GetHashCode",
            "CompareTo",
            "GetEnumerator",
            "MoveNext",
            "Current",
            "Finalize",
            "Clone",
            "Close",
            "Main",
            "Start",
            "Update",
            "Awake",
            "OnEnable",
            "OnDisable",
        ],
        Lang::Go => &[
            "String",
            "Error",
            "Len",
            "Less",
            "Swap",
            "Read",
            "Write",
            "Close",
            "ServeHTTP",
            "MarshalJSON",
            "UnmarshalJSON",
            "Unwrap",
        ],
        Lang::Kotlin => &[
            "toString",
            "equals",
            "hashCode",
            "compareTo",
            "iterator",
            "invoke",
            "get",
            "set",
            "getValue",
            "setValue",
            "contains",
            "plus",
            "minus",
            "times",
            "div",
            "rem",
            "not",
            "close",
            "run",
            "next",
            "hasNext",
        ],
        Lang::Swift => &[
            "description",
            "debugDescription",
            "hash",
            "encode",
            "body",
            "makeBody",
            "viewDidLoad",
            "viewWillAppear",
            "viewDidAppear",
            "applicationDidFinishLaunching",
            "next",
            "makeIterator",
            "callAsFunction",
            "init",
            "deinit",
        ],
        Lang::Php => &[
            "__construct",
            "__destruct",
            "__call",
            "__callStatic",
            "__get",
            "__set",
            "__isset",
            "__unset",
            "__sleep",
            "__wakeup",
            "__serialize",
            "__unserialize",
            "__toString",
            "__invoke",
            "__set_state",
            "__clone",
            "__debugInfo",
            "jsonSerialize",
            "getIterator",
            "count",
            "current",
            "key",
            "next",
            "rewind",
            "valid",
            "offsetGet",
            "offsetSet",
            "offsetExists",
            "offsetUnset",
        ],
        Lang::Ruby => &[
            "initialize",
            "to_s",
            "to_str",
            "to_a",
            "to_h",
            "to_i",
            "to_proc",
            "inspect",
            "each",
            "call",
            "method_missing",
            "respond_to_missing?",
            "hash",
            "eql?",
            "equal?",
            "coerce",
            "included",
            "extended",
            "inherited",
            "prepended",
            "setup",
            "teardown",
            "<=>",
            "==",
            "===",
            "[]",
            "[]=",
            "<<",
            "+",
            "-",
            "*",
            "/",
        ],
        Lang::C => &[],
        Lang::Cpp => &[
            "begin",
            "end",
            "size",
            "swap",
            "what",
            "operator()",
            "operator[]",
            "operator==",
            "operator<",
            "operator<<",
            "operator>>",
        ],
        Lang::Lua => &[
            "__index",
            "__newindex",
            "__call",
            "__tostring",
            "__eq",
            "__lt",
            "__le",
            "__len",
            "__concat",
            "__gc",
            "__close",
            "__add",
            "__sub",
            "__mul",
            "__div",
            "__mod",
            "__unm",
            "new",
        ],
        Lang::Java => &[
            "toString",
            "equals",
            "hashCode",
            "clone",
            "finalize",
            "compareTo",
            "iterator",
            "close",
            "run",
            "call",
            "get",
            "accept",
            "apply",
            "test",
            "compare",
            "hasNext",
            "next",
            "getMessage",
        ],
    }
}

fn entry_names(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Python => &["main", "__main__"],
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx => &[],
        Lang::Rust => &["main"],
        Lang::CSharp => &["Main"],
        Lang::Go => &["main", "init"],
        Lang::Java => &["main"],
        Lang::Kotlin | Lang::C | Lang::Cpp => &["main"],
        Lang::Swift | Lang::Php | Lang::Ruby | Lang::Lua => &[],
    }
}

/// One line of the per-project ignore list: a name (`*` wildcards allowed) or `path:<prefix>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IgnoreRule {
    Name(String),
    Path(String),
}

impl IgnoreRule {
    pub fn parse(line: &str) -> Option<IgnoreRule> {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            return None;
        }
        Some(match l.strip_prefix("path:") {
            Some(p) => IgnoreRule::Path(p.trim().trim_end_matches('/').to_owned()),
            None => IgnoreRule::Name(l.to_owned()),
        })
    }

    fn matches(&self, name: &str, path: &str) -> bool {
        match self {
            IgnoreRule::Name(p) => glob(p, name),
            IgnoreRule::Path(p) => path == p || path.starts_with(&format!("{p}/")),
        }
    }
}

/// `*` matches any run of characters.
fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text;
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() {
            continue;
        }
        match rest.find(p) {
            Some(at) if i > 0 || at == 0 || parts[0].is_empty() => rest = &rest[at + p.len()..],
            _ => return false,
        }
    }
    parts.last().is_none_or(|l| l.is_empty()) || text.ends_with(parts.last().copied().unwrap_or(""))
}

/// Parse a multi-line ignore list.
pub fn parse_ignore(text: &str) -> Vec<IgnoreRule> {
    text.lines().filter_map(IgnoreRule::parse).collect()
}

fn is_dunder(name: &str) -> bool {
    name.len() > 4 && name.starts_with("__") && name.ends_with("__")
}

fn is_test_path(path: &str) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path);
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
    file.ends_with("_test.go")
        || stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
        || file.contains(".test.")
        || file.contains(".spec.")
        || path
            .split('/')
            .any(|d| matches!(d, "tests" | "test" | "__tests__" | "spec"))
}

/// Base type names from a class signature (`class A(B, C):`, `class A extends B implements C`, `class A : B, IC`).
fn bases(lang: Lang, sig: &str) -> Vec<String> {
    let words = |s: &str| -> Vec<String> {
        s.split([',', ' ', '<', '>', '(', ')', ':', '{'])
            .map(|w| w.rsplit('.').next().unwrap_or(w).trim().to_owned())
            .filter(|w| {
                !w.is_empty()
                    && !matches!(
                        w.as_str(),
                        "extends"
                            | "implements"
                            | "public"
                            | "class"
                            | "where"
                            | "private"
                            | "protected"
                            | "virtual"
                    )
            })
            .collect()
    };
    match lang {
        Lang::Python => sig
            .find('(')
            .map(|i| words(sig[i..].split(')').next().unwrap_or("")))
            .unwrap_or_default(),
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx | Lang::Java => {
            let mut out = Vec::new();
            for kw in ["extends ", "implements "] {
                if let Some(i) = sig.find(kw) {
                    let tail = &sig[i + kw.len()..];
                    let end = tail
                        .find(" implements ")
                        .or_else(|| tail.find(" extends "))
                        .unwrap_or(tail.len());
                    out.extend(words(&tail[..end]));
                }
            }
            out
        }
        Lang::CSharp | Lang::Kotlin | Lang::Swift | Lang::Cpp => {
            sig.split_once(':').map(|(_, t)| words(t)).unwrap_or_default()
        }
        Lang::Ruby => sig.split_once('<').map(|(_, t)| words(t)).unwrap_or_default(),
        Lang::Php => {
            let mut out = Vec::new();
            for kw in ["extends ", "implements "] {
                if let Some(i) = sig.find(kw) {
                    out.extend(words(
                        sig[i + kw.len()..].split(" implements ").next().unwrap_or(""),
                    ));
                }
            }
            out
        }
        Lang::C | Lang::Lua => Vec::new(),
        Lang::Rust | Lang::Go => Vec::new(),
    }
}

/// Flag reserved, entry-point, test, override, and dynamic-reference symbols.
pub fn apply(index: &mut CodeIndex, ignore: &[IgnoreRule]) {
    // Methods per class name, for override detection through the project's own hierarchy.
    let mut methods_of: HashMap<String, HashSet<String>> = HashMap::new();
    let mut class_bases: HashMap<String, Vec<String>> = HashMap::new();
    for s in &index.symbols {
        let Some(lang) = s.lang else { continue };
        match s.kind {
            SymbolKind::Method => {
                if let Some(c) = s.scope.rsplit("::").next() {
                    methods_of.entry(c.to_owned()).or_default().insert(s.name.clone());
                }
            }
            SymbolKind::Class | SymbolKind::Interface => {
                class_bases
                    .entry(s.name.clone())
                    .or_default()
                    .extend(bases(lang, &s.signature));
            }
            _ => {}
        }
    }
    let inherited = |class: &str, method: &str| -> bool {
        let mut seen = HashSet::new();
        let mut stack: Vec<&str> = class_bases
            .get(class)
            .map(|v| v.iter().map(String::as_str).collect())
            .unwrap_or_default();
        while let Some(b) = stack.pop() {
            if !seen.insert(b) {
                continue;
            }
            if methods_of.get(b).is_some_and(|m| m.contains(method)) {
                return true;
            }
            if let Some(more) = class_bases.get(b) {
                stack.extend(more.iter().map(String::as_str));
            }
        }
        false
    };

    let test_attr: HashSet<(u32, u32)> = index
        .facts
        .iter()
        .enumerate()
        .flat_map(|(fi, f)| {
            f.defs
                .iter()
                .filter(|d| d.test_attr)
                .map(move |d| (fi as u32, d.name_span.start))
        })
        .collect();

    for i in 0..index.symbols.len() {
        let (kind, lang, name, scope, file, start) = {
            let s = &index.symbols[i];
            (
                s.kind,
                s.lang,
                s.name.clone(),
                s.scope.clone(),
                s.def.file,
                s.def.start,
            )
        };
        if matches!(kind, SymbolKind::File | SymbolKind::Folder) {
            // Only the ignore list reserves files and folders.
            let path = index.symbols[i].signature.clone();
            if ignore.iter().any(|r| r.matches(&name, &path)) {
                index.symbols[i].flags.set(Flags::RESERVED);
            }
            continue;
        }
        let Some(lang) = lang else { continue };
        let path = index
            .files
            .get(file as usize)
            .map(|f| f.path.clone())
            .unwrap_or_default();
        let top_level = scope.is_empty();
        let mut flags = Flags::default();
        let callable = matches!(kind, SymbolKind::Function | SymbolKind::Method);
        if callable && entry_names(lang).contains(&name.as_str()) && (top_level || lang != Lang::Python) {
            flags.set(Flags::ENTRY_POINT);
        }
        if is_dunder(&name) && lang == Lang::Python {
            flags.set(Flags::ENTRY_POINT);
        }
        let in_test_file = is_test_path(&path);
        let test_named = match lang {
            Lang::Python => {
                (callable && name.starts_with("test"))
                    || (kind == SymbolKind::Class && name.starts_with("Test"))
            }
            Lang::Go => {
                callable
                    && top_level
                    && ["Test", "Benchmark", "Example", "Fuzz"]
                        .iter()
                        .any(|p| name.starts_with(p))
                    && path.ends_with("_test.go")
            }
            _ => false,
        };
        if test_attr.contains(&(file, start)) || (test_named && (in_test_file || lang == Lang::Python)) {
            flags.set(Flags::TEST);
        }
        if callable && special_names(lang).contains(&name.as_str()) && kind == SymbolKind::Method {
            flags.set(Flags::OVERRIDE);
        }
        if kind == SymbolKind::Method
            && let Some(class) = scope.rsplit("::").next()
            && inherited(class, &name)
        {
            flags.set(Flags::OVERRIDE);
        }
        if matches!(kind, SymbolKind::Method | SymbolKind::Function) && is_dunder(&name) {
            flags.set(Flags::ENTRY_POINT);
        }
        if index.symbols[i]
            .refs
            .iter()
            .any(|r| r.kind == RefKind::StringLiteral)
        {
            flags.set(Flags::DYNAMIC_REFS);
        }
        let reserved = flags.has(Flags::ENTRY_POINT)
            || flags.has(Flags::TEST)
            || flags.has(Flags::OVERRIDE)
            || ignore.iter().any(|r| r.matches(&name, &path));
        if reserved {
            flags.set(Flags::RESERVED);
        }
        index.symbols[i].flags.0 |= flags.0;
    }
}

/// Symbols that a rename must never touch.
pub fn is_reserved(index: &CodeIndex, id: SymbolId) -> bool {
    index.symbol(id).is_some_and(|s| s.flags.has(Flags::RESERVED))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_and_builtin_lists_load() {
        assert!(is_keyword(Lang::Python, "lambda"));
        assert!(is_keyword(Lang::Rust, "impl"));
        assert!(is_keyword(Lang::Go, "defer"));
        assert!(is_builtin(Lang::Python, "print"));
        assert!(is_builtin(Lang::Java, "String"));
        assert!(!is_builtin(Lang::Python, "calc_total"));
        for l in Lang::ALL {
            assert!(list(keywords_text(l)).len() > 20, "{} keywords", l.name());
            assert!(list(globals_text(l)).len() > 20, "{} globals", l.name());
        }
    }

    #[test]
    fn ignore_globs() {
        assert!(glob("handle*", "handleClick"));
        assert!(!glob("handle*", "onhandle"));
        assert!(glob("*Props", "ButtonProps"));
        assert!(glob("a*c", "abbbc"));
        assert!(!glob("a*c", "abbbd"));
        assert!(glob("exact", "exact") && !glob("exact", "exactly"));
        let rules = parse_ignore("# c\nhandle*\npath:vendor/\n");
        assert_eq!(rules.len(), 2);
        assert!(rules[1].matches("x", "vendor/a/b.py") && !rules[1].matches("x", "vendorx/a.py"));
    }

    #[test]
    fn base_class_parsing() {
        assert_eq!(bases(Lang::Python, "class A(B, mod.C):"), ["B", "C"]);
        assert_eq!(
            bases(Lang::Java, "public class A extends B implements C, D"),
            ["B", "C", "D"]
        );
        assert_eq!(bases(Lang::CSharp, "public class A : B, IC"), ["B", "IC"]);
    }
}
