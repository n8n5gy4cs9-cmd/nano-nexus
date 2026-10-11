//! Naming rules engine (ANALYZER §5.5): per-language, per-kind conventions and the rules that flag bad
//! names for symbols, files, and folders. Pure functions: nothing here touches the disk or a model.

use std::collections::{BTreeMap, HashMap};

use heck::{ToKebabCase, ToLowerCamelCase, ToShoutySnakeCase, ToSnakeCase, ToUpperCamelCase};
use serde::{Deserialize, Serialize};

use super::index::{CodeIndex, Flags, SymbolId};
use super::lang::Lang;
use super::model::SymbolKind;

/// A naming style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Case {
    /// `snake_case`
    Snake,
    /// `camelCase`
    Camel,
    /// `PascalCase`
    Pascal,
    /// `SCREAMING_SNAKE`
    Screaming,
    /// `kebab-case`
    Kebab,
    /// `_camelCase` (C# private fields)
    UnderscoreCamel,
    /// Lower-case words with no separators (`pricing`), Go and Java packages
    Flat,
}

impl Case {
    /// Label for prompts (`snake_case`).
    pub fn label(self) -> &'static str {
        match self {
            Case::Snake => "snake_case",
            Case::Camel => "camelCase",
            Case::Pascal => "PascalCase",
            Case::Screaming => "SCREAMING_SNAKE_CASE",
            Case::Kebab => "kebab-case",
            Case::UnderscoreCamel => "_camelCase",
            Case::Flat => "lowercase",
        }
    }

    /// The JSON-schema `pattern` the proposal validator uses for names in this style.
    pub fn pattern(self) -> &'static str {
        match self {
            Case::Snake => "^[a-z][a-z0-9]*(_[a-z0-9]+)*$",
            Case::Camel => "^[a-z][a-zA-Z0-9]*$",
            Case::Pascal => "^[A-Z][a-zA-Z0-9]*$",
            Case::Screaming => "^[A-Z][A-Z0-9]*(_[A-Z0-9]+)*$",
            Case::Kebab => "^[a-z][a-z0-9]*(-[a-z0-9]+)*$",
            Case::UnderscoreCamel => "^_[a-z][a-zA-Z0-9]*$",
            Case::Flat => "^[a-z][a-z0-9]*$",
        }
    }

    /// Whether `name` is written in this style.
    pub fn matches(self, name: &str) -> bool {
        let ascii_alnum = |s: &str| s.chars().all(|c| c.is_ascii_alphanumeric());
        match self {
            Case::Snake => {
                !name.is_empty()
                    && name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                    && name.split('_').all(|p| {
                        !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                    })
            }
            Case::Screaming => {
                !name.is_empty()
                    && name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                    && name.split('_').all(|p| {
                        !p.is_empty() && p.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                    })
            }
            Case::Camel => name.chars().next().is_some_and(|c| c.is_ascii_lowercase()) && ascii_alnum(name),
            Case::Pascal => name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && ascii_alnum(name),
            Case::Kebab => {
                !name.is_empty()
                    && name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                    && name.split('-').all(|p| {
                        !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                    })
            }
            Case::UnderscoreCamel => name.strip_prefix('_').is_some_and(|r| Case::Camel.matches(r)),
            Case::Flat => {
                name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                    && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            }
        }
    }

    /// Convert `name` to this style (words are split on case changes and separators).
    pub fn convert(self, name: &str) -> String {
        let lead: String = name.chars().take_while(|c| *c == '_').collect();
        let core = name.trim_start_matches('_');
        match self {
            Case::Snake => format!("{lead}{}", core.to_snake_case()),
            Case::Camel => core.to_lower_camel_case(),
            Case::Pascal => core.to_upper_camel_case(),
            Case::Screaming => core.to_shouty_snake_case(),
            Case::Kebab => core.to_kebab_case(),
            Case::UnderscoreCamel => format!("_{}", core.to_lower_camel_case()),
            Case::Flat => core.to_snake_case().replace('_', ""),
        }
    }
}

/// The styles accepted for a kind, best first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Convention {
    pub accepted: Vec<Case>,
}

impl Convention {
    fn of(accepted: &[Case]) -> Convention {
        Convention {
            accepted: accepted.to_vec(),
        }
    }

    /// The style proposals use.
    pub fn primary(&self) -> Case {
        self.accepted.first().copied().unwrap_or(Case::Snake)
    }

    pub fn accepts(&self, name: &str) -> bool {
        self.accepted.iter().any(|c| c.matches(name))
    }
}

/// Style of file and folder names in a project (JS/TS follow the majority).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectStyle {
    pub js_files: Case,
    pub js_folders: Case,
}

impl Default for ProjectStyle {
    fn default() -> Self {
        Self {
            js_files: Case::Kebab,
            js_folders: Case::Kebab,
        }
    }
}

/// How serious a finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Low,
    Medium,
    High,
}

/// One finding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Issue {
    /// `case.mismatch`, `len.too_short`, `meaningless`, `hungarian`, `abbrev`, `numbered`, `bool.prefix`,
    /// `inconsistent.synonym`, `file.case`, `file.mismatch`, `file.generic`
    pub rule: String,
    pub severity: Severity,
    pub detail: String,
}

fn issue(rule: &str, severity: Severity, detail: impl Into<String>) -> Issue {
    Issue {
        rule: rule.to_owned(),
        severity,
        detail: detail.into(),
    }
}

/// What the rules need to know about one symbol.
#[derive(Clone, Debug)]
pub struct NameCtx<'a> {
    pub lang: Lang,
    pub kind: SymbolKind,
    pub name: &'a str,
    pub exported: bool,
    /// `private` / `public` / … from the syntax.
    pub visibility: Option<&'a str>,
    /// The definition's first line (used to find a boolean type).
    pub signature: &'a str,
    /// The file the symbol lives in is a `.tsx`/`.jsx` file.
    pub jsx_file: bool,
}

/// Tunable lists.
#[derive(Clone, Debug, Default)]
pub struct RuleOptions {
    /// Abbreviations that are fine in this project (added to the built-in allow-list).
    pub abbrev_allow: Vec<String>,
}

/// The convention for a symbol kind in a language.
pub fn convention(c: &NameCtx<'_>) -> Convention {
    use Case::*;
    use SymbolKind as K;
    match (c.lang, c.kind) {
        (Lang::Python, K::Class | K::Interface | K::Enum) => Convention::of(&[Pascal]),
        (Lang::Python, K::Constant | K::EnumMember) => Convention::of(&[Screaming]),
        (Lang::Python, _) => Convention::of(&[Snake]),

        (Lang::JavaScript | Lang::TypeScript | Lang::Tsx, K::Class | K::Interface | K::Enum) => {
            Convention::of(&[Pascal])
        }
        (Lang::JavaScript | Lang::TypeScript | Lang::Tsx, K::Constant) => Convention::of(&[Screaming, Camel]),
        (Lang::JavaScript | Lang::TypeScript | Lang::Tsx, K::EnumMember) => {
            Convention::of(&[Pascal, Screaming])
        }
        (Lang::JavaScript | Lang::TypeScript | Lang::Tsx, K::Function) if c.jsx_file => {
            Convention::of(&[Camel, Pascal])
        }
        (Lang::JavaScript | Lang::TypeScript | Lang::Tsx, K::Variable) if c.jsx_file => {
            Convention::of(&[Camel, Pascal])
        }
        (Lang::JavaScript | Lang::TypeScript | Lang::Tsx, _) => Convention::of(&[Camel]),

        (Lang::Rust, K::Class | K::Interface | K::Enum | K::EnumMember) => Convention::of(&[Pascal]),
        (Lang::Rust, K::Constant) => Convention::of(&[Screaming]),
        (Lang::Rust, _) => Convention::of(&[Snake]),

        (
            Lang::CSharp,
            K::Class | K::Interface | K::Enum | K::EnumMember | K::Constant | K::Method | K::Function,
        ) => Convention::of(&[Pascal]),
        (Lang::CSharp, K::Field) => {
            if matches!(c.visibility, Some("public" | "internal" | "protected")) {
                Convention::of(&[Pascal])
            } else {
                Convention::of(&[UnderscoreCamel, Camel])
            }
        }
        (Lang::CSharp, _) => Convention::of(&[Camel]),

        (
            Lang::Go,
            K::Class
            | K::Interface
            | K::Enum
            | K::Function
            | K::Method
            | K::Constant
            | K::Field
            | K::Variable
            | K::EnumMember,
        ) => {
            if c.exported {
                Convention::of(&[Pascal])
            } else {
                Convention::of(&[Camel])
            }
        }
        (Lang::Go, _) => Convention::of(&[Camel]),

        (Lang::Java, K::Class | K::Interface | K::Enum) => Convention::of(&[Pascal]),
        (Lang::Java, K::Constant | K::EnumMember) => Convention::of(&[Screaming]),
        (Lang::Java, _) => Convention::of(&[Camel]),
    }
}

const SHORT_OK: &[&str] = &[
    "i", "j", "k", "x", "y", "z", "dt", "id", "ui", "io", "db", "ok", "n", "m", "t", "e", "f", "s", "ip",
    "fs", "os", "re", "tx", "rx", "up", "to",
];
const MEANINGLESS: &[&str] = &[
    "foo", "bar", "baz", "tmp", "temp", "stuff", "thing", "things", "data", "asdf", "obj", "val", "func",
    "doit", "dostuff", "dothing", "blah", "xxx", "test", "var", "lol",
];
const HUNGARIAN: &[&str] = &[
    "str", "sz", "lpsz", "arr", "lst", "obj", "fn", "b", "i", "n", "f", "d", "s", "p",
];
const ABBREV: &[(&str, &str)] = &[
    ("plyr", "player"),
    ("btn", "button"),
    ("mgr", "manager"),
    ("calc", "calculate"),
    ("cnt", "count"),
    ("idx", "index"),
    ("msg", "message"),
    ("usr", "user"),
    ("pwd", "password"),
    ("num", "number"),
    ("arr", "array"),
    ("lst", "list"),
    ("ptr", "pointer"),
    ("val", "value"),
    ("hlth", "health"),
    ("pos", "position"),
    ("dmg", "damage"),
    ("tex", "texture"),
    ("img", "image"),
    ("gfx", "graphics"),
    ("snd", "sound"),
    ("cfg", "config"),
    ("ctrl", "controller"),
    ("proc", "process"),
    ("impl", "implementation"),
    ("addr", "address"),
    ("auth", "authentication"),
];
const ABBREV_OK: &[&str] = &[
    "id", "ctx", "req", "res", "err", "env", "src", "dst", "args", "params", "util", "db", "ui", "io", "ip",
    "url", "uri", "api", "cb", "max", "min", "len", "str", "int", "bool", "vec", "dir", "app", "config",
    "init",
];
const NUMBERED_OK: &[&str] = &[
    "utf", "md", "sha", "base", "int", "uint", "float", "vec", "mat", "rgb", "ipv", "i", "h", "x", "y", "z",
    "u", "v", "f", "s", "t", "a", "b", "c", "d", "p", "q", "w", "e", "k", "n", "m", "l", "r", "o", "g", "j",
    "oauth", "http", "s3", "crc", "des", "aes", "bit", "uint", "iso", "rfc", "v", "py", "es", "web3",
];
const BOOL_ADJECTIVES: &[&str] = &[
    "enabled",
    "disabled",
    "visible",
    "hidden",
    "active",
    "inactive",
    "valid",
    "invalid",
    "dirty",
    "done",
    "ready",
    "open",
    "closed",
    "locked",
    "loading",
    "success",
    "failed",
    "ok",
    "found",
    "empty",
    "checked",
    "selected",
    "verbose",
    "debug",
    "flag",
    "force",
    "recursive",
    "dry_run",
    "dryrun",
    "required",
    "optional",
    "public",
    "private",
    "mutable",
];
const BOOL_PREFIXES: &[&str] = &[
    "is", "has", "can", "should", "did", "will", "was", "are", "do", "does", "allow", "enable", "use",
    "needs", "must", "supports", "include", "skip", "show", "hide", "ignore", "keep", "only", "with",
    "without", "no",
];
const GENERIC_FILES: &[&str] = &[
    "utils", "util", "helpers", "helper", "misc", "common", "stuff", "things", "temp", "tmp", "untitled",
    "new", "test1", "test2", "scratch", "foo", "bar", "other", "various",
];

/// Split a name into lower-case words (snake, camel, and kebab).
pub fn words(name: &str) -> Vec<String> {
    name.to_snake_case()
        .split('_')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

fn is_bool_signature(sig: &str) -> bool {
    let s = sig.to_ascii_lowercase();
    s.contains(": bool")
        || s.contains(": boolean")
        || s.starts_with("bool ")
        || s.contains(" bool ")
        || s.contains("boolean ")
        || s.contains("-> bool")
        || s.ends_with(" bool")
}

/// Check one symbol name against every per-name rule.
pub fn check_name(c: &NameCtx<'_>, opts: &RuleOptions) -> Vec<Issue> {
    let name = c.name;
    let mut out = Vec::new();
    let core = name.trim_start_matches('_');
    if core.is_empty() || (name.starts_with("__") && name.ends_with("__")) {
        return out;
    }
    let conv = convention(c);
    // Case. C# interfaces additionally start with `I`.
    let mut case_ok =
        conv.accepts(name) || (c.lang == Lang::Python && name.starts_with('_') && conv.accepts(core));
    if c.lang == Lang::Go && !core.contains('_') && Case::Camel.matches(&lower_first(core)) {
        case_ok = true;
    }
    if c.lang == Lang::CSharp && c.kind == SymbolKind::Interface {
        let mut ch = name.chars();
        case_ok = ch.next() == Some('I')
            && ch.next().is_some_and(|x| x.is_ascii_uppercase())
            && Case::Pascal.matches(name);
    }
    if c.lang == Lang::Rust && c.kind == SymbolKind::Parameter && name.starts_with('_') {
        case_ok = Case::Snake.matches(core);
    }
    if !case_ok {
        out.push(issue(
            "case.mismatch",
            Severity::Medium,
            format!(
                "`{name}` should be {} ({} {})",
                conv.primary().label(),
                c.lang.name(),
                c.kind.label()
            ),
        ));
    }
    let lower = core.to_ascii_lowercase();
    let squashed: String = lower.chars().filter(|c| *c != '_').collect();
    let stem_no_digits = squashed.trim_end_matches(|c: char| c.is_ascii_digit());
    // Too short.
    if core.chars().count() < 3
        && !SHORT_OK.contains(&lower.as_str())
        && !matches!(c.kind, SymbolKind::Module | SymbolKind::EnumMember)
    {
        out.push(issue(
            "len.too_short",
            if c.kind == SymbolKind::Parameter {
                Severity::Low
            } else {
                Severity::Medium
            },
            format!("`{name}` is too short to say what it is"),
        ));
    }
    // Meaningless.
    if MEANINGLESS.contains(&stem_no_digits) {
        out.push(issue(
            "meaningless",
            Severity::High,
            format!("`{name}` says nothing about what it is"),
        ));
    }
    // Hungarian notation.
    let hungarian = HUNGARIAN.iter().any(|p| {
        core.strip_prefix(p).is_some_and(|rest| {
            rest.chars().next().is_some_and(|ch| ch.is_ascii_uppercase())
                && rest.chars().count() >= 3
                && core.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        })
    }) || ["m_", "g_", "s_"]
        .iter()
        .any(|p| core.starts_with(p) && core.len() > 3 && c.lang != Lang::Rust);
    if hungarian && !matches!(c.lang, Lang::Go if c.kind == SymbolKind::Method) {
        out.push(issue(
            "hungarian",
            Severity::Medium,
            format!("`{name}` encodes its type in the name"),
        ));
    }
    // Abbreviations (word by word).
    let allow: Vec<String> = opts.abbrev_allow.iter().map(|s| s.to_lowercase()).collect();
    for w in words(core) {
        if ABBREV_OK.contains(&w.as_str()) || allow.contains(&w) {
            continue;
        }
        if let Some((_, full)) = ABBREV.iter().find(|(a, _)| *a == w) {
            out.push(issue(
                "abbrev",
                Severity::Low,
                format!("`{w}` could be spelled out: `{full}`"),
            ));
            break;
        }
    }
    // Numbered names.
    let digits = core.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits < core.chars().count() {
        let stem: String = core.chars().take(core.chars().count() - digits).collect();
        let last_word = words(&stem).pop().unwrap_or_default();
        let stem_flat = stem.to_ascii_lowercase().replace('_', "");
        let ok = NUMBERED_OK.contains(&last_word.as_str())
            || NUMBERED_OK.contains(&stem_flat.as_str())
            || stem_flat.chars().count() <= 1;
        if !ok {
            out.push(issue(
                "numbered",
                Severity::Medium,
                format!("`{name}` is numbered instead of described"),
            ));
        }
    }
    // Boolean prefix (only when the type is visible in the definition).
    if matches!(
        c.kind,
        SymbolKind::Variable | SymbolKind::Field | SymbolKind::Parameter | SymbolKind::Constant
    ) && is_bool_signature(c.signature)
    {
        let w = words(core);
        let first = w.first().map(String::as_str).unwrap_or("");
        let ok = BOOL_PREFIXES.contains(&first)
            || BOOL_ADJECTIVES.contains(&squashed.as_str())
            || BOOL_ADJECTIVES.contains(&lower.as_str());
        if !ok {
            out.push(issue(
                "bool.prefix",
                Severity::Low,
                format!(
                    "`{name}` is a boolean: name it `is_{}` or `has_{}`",
                    lower.trim_start_matches('_'),
                    lower.trim_start_matches('_')
                ),
            ));
        }
    }
    out
}

fn lower_first(s: &str) -> String {
    let mut ch = s.chars();
    match ch.next() {
        Some(f) => f.to_lowercase().collect::<String>() + ch.as_str(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------- files and folders

/// Case of a file or folder stem expected for a language.
pub fn file_convention(lang: Lang, folder: bool, style: &ProjectStyle, jsx: bool) -> Convention {
    use Case::*;
    match lang {
        Lang::Python | Lang::Rust => Convention::of(&[Snake]),
        Lang::Go => {
            if folder {
                Convention::of(&[Flat])
            } else {
                Convention::of(&[Snake, Flat])
            }
        }
        Lang::Java => {
            if folder {
                Convention::of(&[Flat])
            } else {
                Convention::of(&[Pascal])
            }
        }
        Lang::CSharp => Convention::of(&[Pascal]),
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx => {
            let base = if folder { style.js_folders } else { style.js_files };
            if jsx && !folder {
                Convention::of(&[base, Pascal])
            } else if folder {
                Convention::of(&[base, Flat])
            } else {
                Convention::of(&[base])
            }
        }
    }
}

/// Majority style of JS/TS file and folder names.
pub fn detect_style(index: &CodeIndex) -> ProjectStyle {
    let mut files: BTreeMap<Case, usize> = BTreeMap::new();
    let mut folders: BTreeMap<Case, usize> = BTreeMap::new();
    let classify = |s: &str| -> Option<Case> {
        // Single lowercase words fit every style: they do not vote.
        if Case::Flat.matches(s) {
            return None;
        }
        [Case::Kebab, Case::Camel, Case::Snake, Case::Pascal]
            .into_iter()
            .find(|c| c.matches(s))
    };
    for f in index
        .files
        .iter()
        .filter(|f| matches!(f.lang, Lang::JavaScript | Lang::TypeScript | Lang::Tsx))
    {
        let name = f.path.rsplit('/').next().unwrap_or(&f.path);
        let stem = name.split('.').next().unwrap_or(name);
        if stem == "index" || f.lang == Lang::Tsx {
            continue;
        }
        if let Some(c) = classify(stem) {
            *files.entry(c).or_default() += 1;
        }
    }
    let js_dirs: std::collections::HashSet<&str> = index
        .files
        .iter()
        .filter(|f| matches!(f.lang, Lang::JavaScript | Lang::TypeScript | Lang::Tsx))
        .filter_map(|f| f.path.rsplit_once('/').map(|(d, _)| d))
        .collect();
    for d in index
        .folders
        .iter()
        .filter(|d| js_dirs.iter().any(|j| j == d || j.starts_with(&format!("{d}/"))))
    {
        if let Some(c) = classify(d.rsplit('/').next().unwrap_or(d)) {
            *folders.entry(c).or_default() += 1;
        }
    }
    let top = |m: &BTreeMap<Case, usize>, default: Case| {
        m.iter()
            .max_by_key(|(c, n)| (**n, std::cmp::Reverse(**c)))
            .map(|(c, _)| *c)
            .unwrap_or(default)
    };
    ProjectStyle {
        js_files: top(&files, Case::Kebab),
        js_folders: top(&folders, Case::Kebab),
    }
}

/// Check a file or folder name (`stem` without extension for files).
pub fn check_path_name(lang: Lang, folder: bool, stem: &str, style: &ProjectStyle, jsx: bool) -> Vec<Issue> {
    let mut out = Vec::new();
    let conv = file_convention(lang, folder, style, jsx);
    let kind = if folder { "folder" } else { "file" };
    if !conv.accepts(stem) {
        out.push(issue(
            "file.case",
            Severity::Medium,
            format!("{kind} `{stem}` should be {}", conv.primary().label()),
        ));
    }
    let flat = stem.to_ascii_lowercase().replace(['_', '-'], "");
    let numbered_new = flat.trim_end_matches(|c: char| c.is_ascii_digit());
    if GENERIC_FILES.contains(&numbered_new) || (flat.starts_with("newfile") || flat.starts_with("untitled"))
    {
        out.push(issue(
            "file.generic",
            Severity::Low,
            format!("{kind} `{stem}` is too generic to tell what is inside"),
        ));
    }
    out
}

/// `file.mismatch`: the file name should match the single main type it holds (Java, C#).
pub fn check_file_matches_type(lang: Lang, stem: &str, top_level_types: &[&str]) -> Option<Issue> {
    if !matches!(lang, Lang::Java | Lang::CSharp) || top_level_types.len() != 1 {
        return None;
    }
    let t = top_level_types[0];
    (t != stem).then(|| {
        issue(
            "file.mismatch",
            Severity::Medium,
            format!("file `{stem}` holds type `{t}`; the file should be named `{t}`"),
        )
    })
}

// ---------------------------------------------------------------- synonyms

const SYNONYMS: &[&[&str]] = &[
    &["get", "fetch", "read", "load", "retrieve", "obtain"],
    &["set", "write", "store", "save", "put", "persist"],
    &["delete", "remove", "erase", "destroy", "drop"],
    &["create", "make", "build", "construct"],
    &["init", "initialize", "setup", "initialise"],
    &["show", "display", "present"],
    &["calc", "compute", "calculate"],
    &["start", "begin", "launch"],
    &["stop", "end", "halt"],
];

/// `inconsistent.synonym`: several verbs of one group are used as function prefixes in the project.
/// Returns an issue for every function that uses a less common verb than the project's favourite.
pub fn synonym_issues(names: &[(SymbolId, &str)]) -> HashMap<SymbolId, Issue> {
    let mut by_group: Vec<HashMap<String, Vec<SymbolId>>> = vec![HashMap::new(); SYNONYMS.len()];
    for (id, name) in names {
        let first = words(name).into_iter().next().unwrap_or_default();
        if let Some(g) = SYNONYMS.iter().position(|grp| grp.contains(&first.as_str())) {
            by_group[g].entry(first).or_default().push(*id);
        }
    }
    let mut out = HashMap::new();
    for g in by_group {
        if g.len() < 2 {
            continue;
        }
        let Some(best) = g
            .iter()
            .max_by_key(|(verb, ids)| (ids.len(), std::cmp::Reverse((*verb).clone())))
            .map(|(v, _)| v.clone())
        else {
            continue;
        };
        for (verb, ids) in &g {
            if *verb == best {
                continue;
            }
            for id in ids {
                out.insert(
                    *id,
                    issue(
                        "inconsistent.synonym",
                        Severity::Low,
                        format!("this project mostly says `{best}`, here it says `{verb}`"),
                    ),
                );
            }
        }
    }
    out
}

// ---------------------------------------------------------------- whole index

/// A symbol with findings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Candidate {
    pub symbol: SymbolId,
    pub convention: Convention,
    pub issues: Vec<Issue>,
    /// A deterministic fix when the only problem is the style (`doStuff` → `do_stuff`).
    pub suggested: Option<String>,
}

/// Find every symbol, file, and folder with at least one naming issue. Reserved symbols are skipped.
pub fn detect(index: &CodeIndex, opts: &RuleOptions) -> Vec<Candidate> {
    let style = detect_style(index);
    let mut out = Vec::new();
    let functions: Vec<(SymbolId, &str)> = index
        .symbols
        .iter()
        .filter(|s| {
            matches!(s.kind, SymbolKind::Function | SymbolKind::Method) && !s.flags.has(Flags::RESERVED)
        })
        .map(|s| (s.id, s.name.as_str()))
        .collect();
    let synonyms = synonym_issues(&functions);

    // Top-level types per file for `file.mismatch`.
    let mut types_in: HashMap<u32, Vec<&str>> = HashMap::new();
    for s in &index.symbols {
        if matches!(
            s.kind,
            SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum
        ) && s.scope.is_empty()
        {
            types_in.entry(s.def.file).or_default().push(s.name.as_str());
        }
    }

    for s in &index.symbols {
        if s.flags.has(Flags::RESERVED) {
            continue;
        }
        match s.kind {
            SymbolKind::Module => continue,
            SymbolKind::File => {
                let Some(lang) = s.lang else { continue };
                let jsx = s.signature.ends_with(".tsx") || s.signature.ends_with(".jsx");
                let mut issues = check_path_name(lang, false, &s.name, &style, jsx);
                if let Some(i) = check_file_matches_type(
                    lang,
                    &s.name,
                    types_in.get(&s.def.file).map(Vec::as_slice).unwrap_or(&[]),
                ) {
                    issues.push(i);
                }
                // `main`, `index`, `mod`, `lib`, `__init__` are conventional names, not findings.
                if matches!(
                    s.name.as_str(),
                    "main" | "index" | "mod" | "lib" | "__init__" | "__main__" | "setup" | "conftest"
                ) {
                    issues.retain(|i| i.rule == "file.mismatch");
                }
                if !issues.is_empty() {
                    let conv = file_convention(lang, false, &style, jsx);
                    let suggested = (issues.iter().all(|i| i.rule == "file.case"))
                        .then(|| conv.primary().convert(&s.name));
                    out.push(Candidate {
                        symbol: s.id,
                        convention: conv,
                        issues,
                        suggested,
                    });
                }
            }
            SymbolKind::Folder => {
                // The language of a folder is the most common language of the code below it.
                let lang = folder_lang(index, &s.signature);
                let Some(lang) = lang else { continue };
                let issues = check_path_name(lang, true, &s.name, &style, false);
                if !issues.is_empty() {
                    let conv = file_convention(lang, true, &style, false);
                    let suggested = (issues.iter().all(|i| i.rule == "file.case"))
                        .then(|| conv.primary().convert(&s.name));
                    out.push(Candidate {
                        symbol: s.id,
                        convention: conv,
                        issues,
                        suggested,
                    });
                }
            }
            _ => {
                let Some(lang) = s.lang else { continue };
                let jsx = index
                    .files
                    .get(s.def.file as usize)
                    .is_some_and(|f| f.path.ends_with(".tsx") || f.path.ends_with(".jsx"));
                let c = NameCtx {
                    lang,
                    kind: s.kind,
                    name: &s.name,
                    exported: s.flags.has(Flags::EXPORTED),
                    visibility: s.visibility.as_deref(),
                    signature: &s.signature,
                    jsx_file: jsx,
                };
                let mut issues = check_name(&c, opts);
                if let Some(i) = synonyms.get(&s.id) {
                    issues.push(i.clone());
                }
                if !issues.is_empty() {
                    let conv = convention(&c);
                    let suggested = (issues.iter().all(|i| i.rule == "case.mismatch"))
                        .then(|| conv.primary().convert(&s.name));
                    out.push(Candidate {
                        symbol: s.id,
                        convention: conv,
                        issues,
                        suggested,
                    });
                }
            }
        }
    }
    out
}

fn folder_lang(index: &CodeIndex, folder: &str) -> Option<Lang> {
    let mut count: BTreeMap<Lang, usize> = BTreeMap::new();
    for f in &index.files {
        if f.path.starts_with(&format!("{folder}/")) {
            *count.entry(f.lang).or_default() += 1;
        }
    }
    count
        .into_iter()
        .max_by_key(|(l, n)| (*n, std::cmp::Reverse(*l)))
        .map(|(l, _)| l)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(lang: Lang, kind: SymbolKind, name: &'a str) -> NameCtx<'a> {
        NameCtx {
            lang,
            kind,
            name,
            exported: false,
            visibility: None,
            signature: "",
            jsx_file: false,
        }
    }

    fn rules(c: &NameCtx<'_>) -> Vec<String> {
        check_name(c, &RuleOptions::default())
            .into_iter()
            .map(|i| i.rule)
            .collect()
    }

    #[test]
    fn case_matching_and_conversion() {
        assert!(
            Case::Snake.matches("calc_total")
                && !Case::Snake.matches("calcTotal")
                && !Case::Snake.matches("Calc")
        );
        assert!(Case::Camel.matches("calcTotal") && !Case::Camel.matches("calc_total"));
        assert!(Case::Pascal.matches("CartItem") && !Case::Pascal.matches("cart_item"));
        assert!(Case::Screaming.matches("TAX_RATE") && Case::Screaming.matches("MAX"));
        assert!(Case::Kebab.matches("my-file") && !Case::Kebab.matches("my_file"));
        assert!(Case::UnderscoreCamel.matches("_total"));
        assert_eq!(Case::Snake.convert("doStuff2"), "do_stuff2");
        assert_eq!(Case::Pascal.convert("cart_item"), "CartItem");
        assert_eq!(Case::Camel.convert("calc_total"), "calcTotal");
        assert_eq!(Case::Screaming.convert("taxRate"), "TAX_RATE");
        assert_eq!(Case::Kebab.convert("Helper_Funcs"), "helper-funcs");
        assert_eq!(Case::Snake.convert("_private"), "_private");
        // Every pattern accepts what `convert` produces.
        for c in [
            Case::Snake,
            Case::Camel,
            Case::Pascal,
            Case::Screaming,
            Case::Kebab,
            Case::UnderscoreCamel,
            Case::Flat,
        ] {
            let out = c.convert("some Mixed_name");
            let re = regex::Regex::new(c.pattern()).unwrap();
            assert!(re.is_match(&out), "{c:?}: {out}");
            assert!(c.matches(&out), "{c:?}: {out}");
        }
    }

    #[test]
    fn symbol_rules_table() {
        use SymbolKind as K;
        // (language, kind, name, rules that must fire, rules that must not)
        type Row<'a> = (Lang, K, &'a str, &'a [&'a str], &'a [&'a str]);
        let table: &[Row<'_>] = &[
            (
                Lang::Python,
                K::Function,
                "calc_total",
                &["abbrev"],
                &["case.mismatch", "len.too_short"],
            ),
            (
                Lang::Python,
                K::Function,
                "doStuff2",
                &["case.mismatch", "meaningless", "numbered"],
                &[],
            ),
            (
                Lang::Python,
                K::Class,
                "cart_item",
                &["case.mismatch"],
                &["abbrev"],
            ),
            (Lang::Python, K::Constant, "TAX_RATE", &[], &["case.mismatch"]),
            (
                Lang::Python,
                K::Variable,
                "tmp",
                &["meaningless"],
                &["case.mismatch"],
            ),
            (Lang::Python, K::Parameter, "p", &["len.too_short"], &[]),
            (Lang::Python, K::Variable, "i", &[], &["len.too_short"]),
            (Lang::Python, K::Variable, "enemy2", &["numbered"], &[]),
            (Lang::Python, K::Variable, "label_3", &["numbered"], &[]),
            (Lang::Python, K::Variable, "utf8", &[], &["numbered"]),
            (
                Lang::Python,
                K::Variable,
                "strName",
                &["hungarian", "case.mismatch"],
                &[],
            ),
            (Lang::Python, K::Variable, "m_score", &["hungarian"], &[]),
            (
                Lang::Python,
                K::Variable,
                "plyr_hp",
                &["abbrev", "len.too_short"][..1],
                &[],
            ),
            (Lang::Python, K::Variable, "bar", &["meaningless"], &[]),
            (
                Lang::JavaScript,
                K::Function,
                "calcTotal",
                &["abbrev"],
                &["case.mismatch"],
            ),
            (
                Lang::TypeScript,
                K::Function,
                "calc_total",
                &["case.mismatch"],
                &[],
            ),
            (Lang::TypeScript, K::Class, "CartItem", &[], &["case.mismatch"]),
            (Lang::TypeScript, K::Constant, "maxItems", &[], &["case.mismatch"]),
            (
                Lang::TypeScript,
                K::Constant,
                "MAX_ITEMS",
                &[],
                &["case.mismatch"],
            ),
            (
                Lang::Rust,
                K::Function,
                "doStuff",
                &["case.mismatch", "meaningless"],
                &[],
            ),
            (Lang::Rust, K::Class, "cart_item", &["case.mismatch"], &[]),
            (Lang::Rust, K::EnumMember, "Ready", &[], &["case.mismatch"]),
            (
                Lang::CSharp,
                K::Method,
                "CalcTotal",
                &["abbrev"],
                &["case.mismatch"],
            ),
            (Lang::CSharp, K::Method, "calc_total", &["case.mismatch"], &[]),
            (Lang::CSharp, K::Interface, "Shape", &["case.mismatch"], &[]),
            (Lang::CSharp, K::Interface, "IShape", &[], &["case.mismatch"]),
            (Lang::CSharp, K::Parameter, "itemCount", &[], &["case.mismatch"]),
            (
                Lang::Go,
                K::Function,
                "CalcTotal",
                &["abbrev"],
                &["case.mismatch"],
            ),
            (Lang::Go, K::Function, "calc_total", &["case.mismatch"], &[]),
            (Lang::Go, K::Variable, "totalPrice", &[], &["case.mismatch"]),
            (Lang::Java, K::Method, "calc_total", &["case.mismatch"], &[]),
            (Lang::Java, K::Constant, "TAX_RATE", &[], &["case.mismatch"]),
            (Lang::Java, K::Class, "cart_item", &["case.mismatch"], &[]),
            (Lang::Java, K::Variable, "tmp", &["meaningless"], &[]),
        ];
        for (lang, kind, name, must, must_not) in table {
            let got = rules(&ctx(*lang, *kind, name));
            for m in *must {
                assert!(
                    got.iter().any(|g| g == m),
                    "{lang:?} {kind:?} {name}: expected {m}, got {got:?}"
                );
            }
            for m in *must_not {
                assert!(
                    !got.iter().any(|g| g == m),
                    "{lang:?} {kind:?} {name}: did not expect {m}, got {got:?}"
                );
            }
        }
    }

    #[test]
    fn dunders_and_lone_underscores_are_left_alone() {
        assert!(rules(&ctx(Lang::Python, SymbolKind::Method, "__init__")).is_empty());
        assert!(rules(&ctx(Lang::Python, SymbolKind::Variable, "_")).is_empty());
        assert!(rules(&ctx(Lang::Python, SymbolKind::Variable, "_cache")).is_empty());
    }

    #[test]
    fn booleans_need_a_prefix_only_when_the_type_is_known() {
        let mut c = ctx(Lang::Python, SymbolKind::Parameter, "visible_to_all");
        c.signature = "def f(visible_to_all: bool)";
        assert!(rules(&c).contains(&"bool.prefix".to_string()));
        c.name = "is_visible";
        assert!(!rules(&c).contains(&"bool.prefix".to_string()));
        c.name = "enabled";
        assert!(
            !rules(&c).contains(&"bool.prefix".to_string()),
            "state adjectives are fine"
        );
        c.name = "visible_to_all";
        c.signature = "def f(visible_to_all)";
        assert!(
            !rules(&c).contains(&"bool.prefix".to_string()),
            "no type, no finding"
        );
    }

    #[test]
    fn abbreviation_allow_list_is_configurable() {
        let c = ctx(Lang::Python, SymbolKind::Variable, "hlth_bar");
        assert!(
            check_name(&c, &RuleOptions::default())
                .iter()
                .any(|i| i.rule == "abbrev")
        );
        let opts = RuleOptions {
            abbrev_allow: vec!["HLTH".into()],
        };
        assert!(!check_name(&c, &opts).iter().any(|i| i.rule == "abbrev"));
    }

    #[test]
    fn file_and_folder_rules_table() {
        let style = ProjectStyle::default();
        let r = |lang, folder, stem: &str, jsx| {
            check_path_name(lang, folder, stem, &style, jsx)
                .into_iter()
                .map(|i| i.rule)
                .collect::<Vec<_>>()
        };
        assert!(r(Lang::Python, false, "Helper_Funcs", false).contains(&"file.case".into()));
        assert!(r(Lang::Python, false, "pricing", false).is_empty());
        assert!(r(Lang::Python, false, "utils", false).contains(&"file.generic".into()));
        assert!(r(Lang::Python, false, "new_file2", false).contains(&"file.generic".into()));
        assert!(r(Lang::Rust, false, "Helpers", false).contains(&"file.case".into()));
        assert!(
            r(Lang::Go, true, "util_funcs", false).contains(&"file.case".into()),
            "go packages are lowercase"
        );
        assert!(r(Lang::Go, true, "pricing", false).is_empty());
        assert!(r(Lang::Java, false, "cart_item", false).contains(&"file.case".into()));
        assert!(r(Lang::Java, false, "CartItem", false).is_empty());
        assert!(r(Lang::CSharp, false, "calc_helpers", false).contains(&"file.case".into()));
        assert!(r(Lang::TypeScript, false, "Helper_Funcs", false).contains(&"file.case".into()));
        assert!(r(Lang::TypeScript, false, "helper-funcs", false).is_empty());
        assert!(
            r(Lang::Tsx, false, "Button", true).is_empty(),
            "components may be PascalCase"
        );
        assert!(check_file_matches_type(Lang::Java, "Util", &["Calc"]).is_some());
        assert!(check_file_matches_type(Lang::Java, "Calc", &["Calc"]).is_none());
        assert!(check_file_matches_type(Lang::Java, "Util", &["A", "B"]).is_none());
        assert!(check_file_matches_type(Lang::Python, "x", &["A"]).is_none());
    }

    #[test]
    fn synonym_verbs_are_made_consistent() {
        let names = [
            (SymbolId(0), "get_hp"),
            (SymbolId(1), "get_mana"),
            (SymbolId(2), "get_gold"),
            (SymbolId(3), "fetch_health"),
            (SymbolId(4), "read_life"),
            (SymbolId(5), "render"),
        ];
        let out = synonym_issues(&names);
        assert_eq!(out.len(), 2);
        assert!(out.contains_key(&SymbolId(3)) && out.contains_key(&SymbolId(4)));
        assert!(out[&SymbolId(3)].detail.contains("`get`"));
    }
}
