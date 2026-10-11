//! Strings mode (M09, T09.13, ANALYZER §5.8): find user-facing string literals, keep their
//! placeholders safe, export the key/text table, and rewrite code to `t("KEY")`.

use std::collections::HashSet;
use std::sync::OnceLock;

use nexus_core::changes::{ChangeSet, FileOp, TextEdit};
use nexus_core::ids::ChangeSetId;
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::index::CodeIndex;
use super::lang::Lang;
use super::propose::Sources;

/// Argument names and call names whose string is shown to people.
const UI_NAMES: &[&str] = &[
    "text",
    "label",
    "title",
    "message",
    "msg",
    "caption",
    "tooltip",
    "placeholder",
    "description",
    "hint",
    "header",
    "heading",
    "subtitle",
    "button",
    "buttontext",
    "settext",
    "setlabel",
    "settitle",
    "setmessage",
    "settooltip",
    "setplaceholder",
    "alert",
    "confirm",
    "showmessage",
    "toast",
    "notify",
    "notification",
    "alt",
    "aria-label",
    "arialabel",
    "drawtext",
    "addlabel",
    "addbutton",
];
/// Calls that already translate their argument.
const I18N_CALLS: &[&str] = &[
    "t",
    "gettext",
    "ngettext",
    "_",
    "tr",
    "i18n",
    "translate",
    "formatmessage",
    "$t",
];
/// Calls whose strings are for developers.
const LOG_CALLS: &[&str] = &[
    "print", "println", "eprintln", "printf", "log", "debug", "info", "warn", "warning", "error", "trace",
    "fatal", "panic", "assert", "console", "logger", "write", "writeln", "format", "getattr", "setattr",
    "hasattr", "require", "import", "open",
];

/// A user-facing string literal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct StringItem {
    /// Position in the extraction result.
    pub id: u32,
    pub file: u32,
    pub path: String,
    pub line: u32,
    /// Byte range of the whole literal, quotes included.
    pub start: u32,
    pub end: u32,
    /// The text between the quotes, exactly as written (escapes intact).
    pub text: String,
    pub placeholders: Vec<String>,
    /// The argument or call name that makes it user-facing: `label`, `setText`, `t`.
    pub context: String,
    /// The text already goes through a translation call.
    pub localized: bool,
}

fn placeholder_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"%(?:\([A-Za-z_]\w*\))?[-+#0]*\d*(?:\.\d+)?[sdifuxXeEgGcr]|\{[A-Za-z0-9_.]*(?::[^{}]*)?\}|\[/?(?:b|i|u|s|color|size|url|font)(?:=[^\]]*)?\]|\\[nrt]",
        )
        .ok()
    })
    .as_ref()
}

/// Placeholders in order of appearance: `%s %d %0.2f %(name)s {name} {0} {0:.2f} [b] [/b] \n \t`.
pub fn placeholders(text: &str) -> Vec<String> {
    placeholder_re()
        .map(|r| r.find_iter(text).map(|m| m.as_str().to_owned()).collect())
        .unwrap_or_default()
}

/// Same placeholders, same number of times (order may change in translations).
pub fn same_placeholders(a: &str, b: &str) -> bool {
    let (mut x, mut y) = (placeholders(a), placeholders(b));
    x.sort();
    y.sort();
    x == y
}

/// Split a literal into its body, or `None` when it is interpolated (f-strings, `${}`, `$"…"`) or raw-hash.
fn body_of(lit: &str) -> Option<&str> {
    let prefix_len = lit.find(['"', '\'', '`'])?;
    let (prefix, rest) = lit.split_at(prefix_len);
    if prefix.contains(['f', 'F', '$']) {
        return None;
    }
    if prefix
        .chars()
        .any(|c| !matches!(c, 'r' | 'R' | 'b' | 'B' | 'u' | 'U' | '@'))
    {
        return None;
    }
    let q = rest.chars().next()?;
    let triple: String = std::iter::repeat_n(q, 3).collect();
    let inner = if rest.len() >= 6 && rest.starts_with(&triple) && rest.ends_with(&triple) {
        &rest[3..rest.len() - 3]
    } else if rest.len() >= 2 && rest.ends_with(q) {
        &rest[1..rest.len() - 1]
    } else {
        return None;
    };
    if q == '`' && inner.contains("${") {
        return None;
    }
    Some(inner)
}

/// The identifier right before `:` or `=` that precedes the literal (`label: "x"`, `title="x"`).
fn key_before(src: &str, start: usize) -> Option<String> {
    let head = src.get(..start)?.trim_end();
    let head = head.strip_suffix([':', '='])?;
    if head.ends_with(['=', '!', '<', '>', ':']) {
        return None;
    }
    let head = head.trim_end().trim_end_matches(['"', '\'']);
    let ident: String = head
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '$'))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    (!ident.is_empty()).then_some(ident)
}

/// Extract the user-facing string literals of every code file.
pub fn extract_strings(index: &CodeIndex) -> Vec<StringItem> {
    let sources = Sources::new(index);
    let mut out: Vec<StringItem> = Vec::new();
    for (fi, facts) in index.facts.iter().enumerate() {
        let Some(file) = index.file(fi as u32) else {
            continue;
        };
        let Some(src) = sources.text(fi as u32) else {
            continue;
        };
        for s in &facts.strings {
            let Some(inner) = body_of(&s.text) else { continue };
            if !inner.chars().any(char::is_alphabetic) || inner.trim().chars().count() < 2 {
                continue;
            }
            let call = s.ctx.as_deref().map(str::to_lowercase);
            if call.as_deref().is_some_and(|c| LOG_CALLS.contains(&c)) {
                continue;
            }
            let key = key_before(&src, s.span.start as usize).map(|k| k.to_lowercase());
            let (context, localized) = match (&call, &key) {
                (Some(c), _) if I18N_CALLS.contains(&c.as_str()) => (c.clone(), true),
                (Some(c), _) if UI_NAMES.contains(&c.as_str()) => (c.clone(), false),
                (_, Some(k)) if UI_NAMES.contains(&k.as_str()) => (k.clone(), false),
                _ => continue,
            };
            out.push(StringItem {
                id: 0,
                file: fi as u32,
                path: file.path.clone(),
                line: s.span.line,
                start: s.span.start,
                end: s.span.end,
                text: inner.to_owned(),
                placeholders: placeholders(inner),
                context,
                localized,
            });
        }
    }
    out.sort_by(|a, b| (&a.path, a.start).cmp(&(&b.path, b.start)));
    for (i, it) in out.iter_mut().enumerate() {
        it.id = i as u32;
    }
    out
}

/// Check a proposal against its item. `max_len` counts characters; `used_keys` holds keys taken so far.
pub fn validate(
    item: &StringItem,
    improved: Option<&str>,
    key: Option<&str>,
    max_len: Option<usize>,
    used_keys: &HashSet<String>,
) -> Result<(), String> {
    if let Some(t) = improved {
        if t.trim().is_empty() {
            return Err("the improved text is empty".into());
        }
        if !same_placeholders(&item.text, t) {
            return Err(format!(
                "placeholders must stay exactly as in the original ({})",
                if item.placeholders.is_empty() {
                    "none".to_owned()
                } else {
                    item.placeholders.join(" ")
                }
            ));
        }
        if let Some(m) = max_len
            && t.chars().count() > m
        {
            return Err(format!("longer than {m} characters"));
        }
    }
    if let Some(k) = key {
        if !is_key(k) {
            return Err(format!("`{k}` is not a CONSTANT_CASE key"));
        }
        if used_keys.contains(k) {
            return Err(format!("the key `{k}` is already used"));
        }
    }
    Ok(())
}

/// `CONSTANT_CASE` of 1 to 64 characters (`MENU_START_GAME`).
pub fn is_key(k: &str) -> bool {
    k.len() <= 64
        && k.starts_with(|c: char| c.is_ascii_uppercase())
        && k.split('_')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
}

/// A key made from the first words of the text (`Start game` → `START_GAME`), when the model gives none.
pub fn fallback_key(text: &str) -> String {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(4)
        .map(|w| {
            w.to_ascii_uppercase()
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect();
    let key = words.join("_");
    if key.is_empty() || !key.starts_with(|c: char| c.is_ascii_uppercase()) {
        format!("TEXT_{key}").trim_end_matches('_').to_owned()
    } else {
        key
    }
}

/// One row of the exported table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Row {
    pub key: String,
    pub text: String,
    pub original: String,
    pub path: String,
    pub line: u32,
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

/// `key,text,original,file,line` with a header row.
pub fn export_csv(rows: &[Row]) -> String {
    let mut out = String::from("key,text,original,file,line\n");
    for r in rows {
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            csv_field(&r.key),
            csv_field(&r.text),
            csv_field(&r.original),
            csv_field(&r.path),
            r.line
        ));
    }
    out
}

/// A flat `{ "KEY": "text" }` object for i18n libraries (keys in table order).
pub fn export_json(rows: &[Row]) -> String {
    let mut map = serde_json::Map::new();
    for r in rows {
        map.insert(r.key.clone(), serde_json::Value::String(unescape(&r.text)));
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(map)).unwrap_or_else(|_| "{}".into())
}

/// Undo the source-level escapes that JSON would double (`\"`, `\n`, `\'`).
fn unescape(s: &str) -> String {
    s.replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\\"", "\"")
        .replace("\\'", "'")
}

/// Rewrite the literals of `rows` (item, key) to `func("KEY")`. Items that are already localized are skipped.
pub fn plan_key_rewrite(
    index: &CodeIndex,
    rows: &[(StringItem, String)],
    func: &str,
    now_ms: i64,
) -> Result<ChangeSet, String> {
    let sources = Sources::new(index);
    let mut ops: Vec<FileOp> = Vec::new();
    let mut by_file: std::collections::BTreeMap<u32, Vec<&(StringItem, String)>> = Default::default();
    for r in rows.iter().filter(|(i, _)| !i.localized) {
        by_file.entry(r.0.file).or_default().push(r);
    }
    for (file, mut list) in by_file {
        list.sort_by_key(|(i, _)| i.start);
        let src = sources.text(file).ok_or("a file could not be read")?;
        let f = index.file(file).ok_or("unknown file")?;
        if blake3::hash(src.as_bytes()).to_hex().to_string() != f.hash {
            return Err(format!(
                "`{}` changed since it was indexed; re-scan first",
                f.path
            ));
        }
        let jsx = f.path.ends_with(".tsx") || f.path.ends_with(".jsx");
        let mut edits = Vec::new();
        let mut last = 0u32;
        for (item, key) in list {
            if item.start < last || src.get(item.start as usize..item.end as usize).is_none() {
                continue;
            }
            last = item.end;
            let before = src.as_bytes();
            let attr = jsx
                && f.lang != Lang::Python
                && item.start >= 2
                && before[item.start as usize - 1] == b'='
                && (before[item.start as usize - 2].is_ascii_alphanumeric()
                    || before[item.start as usize - 2] == b'-');
            let call = format!("{func}(\"{key}\")");
            edits.push(TextEdit {
                start: item.start,
                end: item.end,
                new_text: if attr { format!("{{{call}}}") } else { call },
            });
        }
        if edits.is_empty() {
            continue;
        }
        let mut p = std::path::PathBuf::from(&index.root);
        for part in f.path.split('/') {
            p.push(part);
        }
        ops.push(FileOp::Edit {
            path: p.to_string_lossy().into_owned(),
            base_hash: f.hash.clone(),
            edits,
        });
    }
    if ops.is_empty() {
        return Err("nothing to rewrite".into());
    }
    Ok(ChangeSet {
        id: ChangeSetId::new(),
        title: format!("Use {func}(\"KEY\") for {} strings", rows.len()),
        origin: "rename".into(),
        ops,
        created_at: now_ms,
    })
}
