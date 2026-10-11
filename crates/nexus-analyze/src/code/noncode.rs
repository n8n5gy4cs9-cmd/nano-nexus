//! References outside the code (ANALYZER §5.4): config and manifest files, string lookups such as
//! `getattr(obj, "name")`, and (opt-in at plan time) docs and comments. The scanners only add
//! references to symbols that already exist; they never invent symbols.
//!
//! Matching is token based, never a substring: `calc_total` matches `util.pricing:calc_total` but not
//! `calc_total_2`. Paths are matched segment by segment, so renaming a folder can rewrite every path
//! below it.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::file_facts::FileFacts;
use super::imports::join;
use super::index::{IndexedFile, Location, Ref, RefKind, Symbol, SymbolId};
use super::model::SymbolKind;

/// `Location.file` values at or above this refer to [`OtherFile`]s (config and doc files).
pub const OTHER_BASE: u32 = 1 << 31;

/// A non-code text file that was scanned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct OtherFile {
    pub path: String,
    /// `config` or `doc`.
    pub kind: String,
}

const MAX_OTHER_BYTES: u64 = 512 * 1024;
const MAX_OTHER_FILES: usize = 5000;
const MAX_NOISY_REFS: usize = 200;

/// Classify a file name: `config`, `doc`, or neither.
pub fn other_kind(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext {
        "json" | "toml" | "yaml" | "yml" | "csproj" | "props" | "targets" | "gradle" | "xml" | "cfg"
        | "ini" | "mod" | "sln" | "properties" | "kts" | "lock" => {
            // Lock files and giant generated JSON are noise.
            (!(ext == "lock" || lower == "package-lock.json" || lower.ends_with(".min.json")))
                .then_some("config")
        }
        "md" | "mdx" | "rst" | "txt" | "adoc" => Some("doc"),
        _ => matches!(lower.as_str(), "dockerfile" | "makefile" | "justfile").then_some("config"),
    }
}

/// Decide whether a walk entry is worth scanning.
pub fn wants(size: u64, count: usize) -> bool {
    size <= MAX_OTHER_BYTES && count < MAX_OTHER_FILES
}

/// Everything `scan` needs to know about the project.
pub struct Project<'a> {
    pub root: &'a Path,
    pub files: &'a [IndexedFile],
    pub facts: &'a [FileFacts],
    pub folders: &'a [String],
    pub others: &'a [OtherFile],
}

fn is_noisy_kind(k: SymbolKind) -> bool {
    matches!(k, SymbolKind::Parameter | SymbolKind::File | SymbolKind::Folder)
}

fn is_local(s: &Symbol, facts: &[FileFacts]) -> bool {
    // A local variable/parameter: has a scope that is not a type or module. Approximated by kind + scope.
    matches!(s.kind, SymbolKind::Variable | SymbolKind::Parameter)
        && facts.get(s.def.file as usize).is_some_and(|f| {
            f.defs
                .iter()
                .any(|d| d.name_span.start == s.def.start && d.visible.is_some())
        })
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `(start, end)` of identifier tokens in `text`.
fn idents(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        if is_ident_char(c) {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            out.push((s, i));
        }
    }
    if let Some(s) = start {
        out.push((s, text.len()));
    }
    out
}

fn line_at(text: &str, byte: usize) -> u32 {
    text.as_bytes()[..byte.min(text.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count() as u32
        + 1
}

struct Names<'a> {
    /// Non-local, non-parameter symbols by name.
    code: HashMap<&'a str, Vec<SymbolId>>,
    /// File symbols by path, folder symbols by path.
    files: HashMap<&'a str, SymbolId>,
    folders: HashMap<&'a str, SymbolId>,
    file_stems: HashMap<String, SymbolId>,
}

fn names<'a>(symbols: &'a [Symbol], p: &Project<'a>) -> Names<'a> {
    let mut n = Names {
        code: HashMap::new(),
        files: HashMap::new(),
        folders: HashMap::new(),
        file_stems: HashMap::new(),
    };
    for s in symbols {
        match s.kind {
            SymbolKind::File => {
                n.files.insert(s.signature.as_str(), s.id);
                let stem = s
                    .signature
                    .rsplit_once('.')
                    .map(|(a, _)| a)
                    .unwrap_or(&s.signature);
                n.file_stems.insert(stem.to_owned(), s.id);
            }
            SymbolKind::Folder => {
                n.folders.insert(s.signature.as_str(), s.id);
            }
            _ if is_local(s, p.facts) => {}
            _ => n.code.entry(s.name.as_str()).or_default().push(s.id),
        }
    }
    n
}

struct Out {
    refs: Vec<(SymbolId, Ref)>,
}

impl Out {
    fn add(
        &mut self,
        id: SymbolId,
        file: u32,
        start: usize,
        end: usize,
        line: u32,
        kind: RefKind,
        ambiguous: bool,
    ) {
        self.refs.push((
            id,
            Ref {
                loc: Location {
                    file,
                    start: start as u32,
                    end: end as u32,
                    line,
                },
                kind,
                ambiguous,
                is_def: false,
            },
        ));
    }
}

/// Maximal runs of path-like characters.
fn runs(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        let ok = c.is_alphanumeric() || matches!(c, '_' | '.' | '-' | '/' | '\\' | '@' | ':' | '~' | '$');
        if ok {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            out.push((s, i));
        }
    }
    if let Some(s) = start {
        out.push((s, text.len()));
    }
    out
}

/// Match a `/`-separated path against files and folders, segment by segment.
/// Returns `(segment start, segment end, target)` for every segment that names a folder or file.
fn match_path<'a>(n: &Names<'a>, base: &str, run: &str) -> Vec<(usize, usize, SymbolId, bool)> {
    let norm: String = run.replace('\\', "/");
    let mut lead = 0;
    while norm[lead..].starts_with("./") {
        lead += 2;
    }
    let body = &norm[lead..];
    if body.is_empty() || body.starts_with("../") || body.starts_with('/') {
        return Vec::new();
    }
    let segs: Vec<(usize, usize)> = {
        let mut v = Vec::new();
        let mut pos = 0;
        for piece in body.split('/') {
            if !piece.is_empty() {
                v.push((pos, pos + piece.len()));
            }
            pos += piece.len() + 1;
        }
        v
    };
    for b in [base, ""] {
        let mut cur = b.to_owned();
        let mut hits = Vec::new();
        for (i, (a, e)) in segs.iter().enumerate() {
            let seg = &body[*a..*e];
            let Some(path) = join(&cur, seg) else { break };
            let last = i + 1 == segs.len();
            if let Some(id) = n.folders.get(path.as_str()) {
                hits.push((lead + a, lead + e, *id, false));
            } else if let Some(id) = n.files.get(path.as_str()) {
                let stem_end = seg.rsplit_once('.').map(|(s, _)| s.len()).unwrap_or(seg.len());
                hits.push((lead + a, lead + a + stem_end, *id, true));
                break;
            } else if last && let Some(id) = n.file_stems.get(&path) {
                hits.push((lead + a, lead + e, *id, true));
            } else {
                break;
            }
            cur = path;
        }
        if !hits.is_empty() {
            return hits;
        }
    }
    Vec::new()
}

/// Match a dotted module path (`util.pricing`, `shop.pricing.Calc`) against folders and files.
fn match_dotted<'a>(n: &Names<'a>, base: &str, run: &str) -> Vec<(usize, usize, SymbolId, bool)> {
    let mut segs = Vec::new();
    let mut pos = 0;
    for piece in run.split('.') {
        if !piece.is_empty() {
            segs.push((pos, pos + piece.len()));
        }
        pos += piece.len() + 1;
    }
    if segs.len() < 2 {
        return Vec::new();
    }
    // Python-style roots, then a Java-style suffix match of the first folder segments.
    for root in [base, "", "src"] {
        let mut cur = root.to_owned();
        let mut hits = Vec::new();
        for (a, e) in &segs {
            let seg = &run[*a..*e];
            let Some(path) = join(&cur, seg) else { break };
            if let Some(id) = n.folders.get(path.as_str()) {
                hits.push((*a, *e, *id, false));
            } else if let Some(id) = n.file_stems.get(&path) {
                hits.push((*a, *e, *id, true));
                break;
            } else {
                break;
            }
            cur = path;
        }
        if !hits.is_empty() {
            return hits;
        }
    }
    let mut hits = Vec::new();
    for (take, (a, e)) in segs.iter().enumerate() {
        let want = segs[..=take]
            .iter()
            .map(|(a, e)| &run[*a..*e])
            .collect::<Vec<_>>()
            .join("/");
        let found = n
            .folders
            .iter()
            .find(|(p, _)| **p == want || p.ends_with(&format!("/{want}")));
        match found {
            Some((_, id)) => hits.push((*a, *e, *id, false)),
            None => break,
        }
    }
    hits
}

fn scan_text(n: &Names<'_>, file_id: u32, rel_path: &str, text: &str, is_doc: bool, out: &mut Out) {
    let base = rel_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut claimed: HashSet<(usize, usize)> = HashSet::new();
    for (rs, re) in runs(text) {
        let run = &text[rs..re];
        if run.len() < 3 || run.chars().all(|c| matches!(c, '-' | '.' | ':' | '/')) {
            continue;
        }
        // Split `module:rest` (`util.pricing:calc_total`, `Class::method`), keeping Windows drives out.
        let (modpart, rest_at) = match run.find(':') {
            Some(i) if i > 1 => (&run[..i], Some(i)),
            _ => (run, None),
        };
        let mut hits = Vec::new();
        // A lone word counts as a path only when it is the whole quoted value (`"include": ["src"]`).
        let before = text[..rs].chars().next_back();
        let quoted = matches!(before, Some('"' | '\'')) && text[re..].chars().next() == before;
        if quoted || modpart.contains('/') || modpart.contains('\\') || modpart.contains('.') {
            hits = match_path(n, base, modpart);
            if hits.is_empty() && !modpart.contains('/') && !modpart.contains('\\') {
                hits = match_dotted(n, base, modpart);
            }
        }
        for (a, e, id, _file) in hits {
            claimed.insert((rs + a, rs + e));
            out.add(
                id,
                file_id,
                rs + a,
                rs + e,
                line_at(text, rs + a),
                if is_doc { RefKind::Doc } else { RefKind::ConfigFile },
                false,
            );
        }
        // Names after `:` or `.` inside a qualified run (`mod:func`, `pkg.Class`, `file.ts#name`).
        if !is_doc {
            let tail_start = rest_at.map(|i| i + 1).unwrap_or(0);
            for (a, e) in idents(&run[tail_start..]) {
                let (a, e) = (tail_start + a, tail_start + e);
                let prev = run[..a].chars().next_back();
                let qualified = matches!(prev, Some(':' | '.' | '#'));
                if !qualified || e - a < 3 || claimed.contains(&(rs + a, rs + e)) {
                    continue;
                }
                if let Some(ids) = n.code.get(&run[a..e]) {
                    let ids: Vec<SymbolId> = ids.clone();
                    let ambiguous = ids.len() > 1;
                    for id in ids {
                        out.add(
                            id,
                            file_id,
                            rs + a,
                            rs + e,
                            line_at(text, rs + a),
                            RefKind::ConfigFile,
                            ambiguous,
                        );
                    }
                }
            }
        }
    }
    if is_doc {
        // Prose and code spans: whole words that equal a symbol name (4+ characters, to skip noise).
        for (a, e) in idents(text) {
            if e - a < 4 || claimed.contains(&(a, e)) {
                continue;
            }
            if let Some(ids) = n.code.get(&text[a..e]) {
                let ambiguous = ids.len() > 1;
                for id in ids {
                    out.add(*id, file_id, a, e, line_at(text, a), RefKind::Doc, ambiguous);
                }
            }
        }
    }
}

/// Strings that name a symbol by calling a reflection-style function.
const STRONG_CTX: &[&str] = &[
    "getattr",
    "hasattr",
    "setattr",
    "delattr",
    "import_module",
    "__import__",
    "GetMethod",
    "GetProperty",
    "GetField",
    "GetMember",
    "GetType",
    "forName",
    "getMethod",
    "getDeclaredMethod",
    "getField",
    "getDeclaredField",
    "require",
    "[]",
    "nameof",
    "Reflect",
    "invoke",
    "InvokeMember",
];

/// The text between the quotes of a string literal, with its offset in the literal.
fn inner(lit: &str) -> Option<(usize, &str)> {
    let q = lit.find(['"', '\'', '`'])?;
    let qc = lit[q..].chars().next()?;
    let triple = lit[q..].starts_with(&format!("{qc}{qc}{qc}"));
    let ql = if triple { 3 } else { 1 };
    let start = q + ql;
    let end = lit.len().checked_sub(ql)?;
    (end >= start).then(|| (start, &lit[start..end]))
}

fn scan_strings(
    n: &Names<'_>,
    symbols: &[Symbol],
    files: &[IndexedFile],
    facts: &[FileFacts],
    out: &mut Out,
) {
    for (fi, fa) in facts.iter().enumerate() {
        let lang = files[fi].lang;
        for s in &fa.strings {
            let Some((off, name)) = inner(&s.text) else {
                continue;
            };
            if name.len() < 2
                || !name.chars().all(is_ident_char)
                || name.chars().next().is_some_and(|c| c.is_ascii_digit())
            {
                continue;
            }
            let Some(ids) = n.code.get(name) else { continue };
            let ids: Vec<SymbolId> = ids
                .iter()
                .copied()
                .filter(|id| {
                    symbols[id.0 as usize]
                        .lang
                        .is_some_and(|l| super::index::same_family(l, lang))
                        && !is_noisy_kind(symbols[id.0 as usize].kind)
                })
                .collect();
            if ids.is_empty() {
                continue;
            }
            let strong = s.ctx.as_deref().is_some_and(|c| STRONG_CTX.contains(&c));
            let ambiguous = !strong || ids.len() > 1;
            let start = s.span.start as usize + off;
            for id in ids {
                out.add(
                    id,
                    fi as u32,
                    start,
                    start + name.len(),
                    s.span.line,
                    RefKind::StringLiteral,
                    ambiguous,
                );
            }
        }
    }
}

fn scan_comments(n: &Names<'_>, facts: &[FileFacts], out: &mut Out) {
    for (fi, fa) in facts.iter().enumerate() {
        for c in &fa.comments {
            for (a, e) in idents(&c.text) {
                if e - a < 4 {
                    continue;
                }
                if let Some(ids) = n.code.get(&c.text[a..e]) {
                    let ambiguous = ids.len() > 1;
                    for id in ids {
                        let start = c.span.start as usize + a;
                        out.add(
                            *id,
                            fi as u32,
                            start,
                            start + (e - a),
                            c.span.line,
                            RefKind::Comment,
                            ambiguous,
                        );
                    }
                }
            }
        }
    }
}

/// Add config, string, doc, and comment references to `symbols`.
pub fn scan(p: &Project<'_>, symbols: &mut [Symbol]) {
    let mut out = Out { refs: Vec::new() };
    {
        let n = names(symbols, p);
        scan_strings(&n, symbols, p.files, p.facts, &mut out);
        scan_comments(&n, p.facts, &mut out);
        for (i, o) in p.others.iter().enumerate() {
            let Ok(text) = std::fs::read_to_string(p.root.join(&o.path)) else {
                continue;
            };
            scan_text(
                &n,
                OTHER_BASE + i as u32,
                &o.path,
                &text,
                o.kind == "doc",
                &mut out,
            );
        }
    }
    // Keep noisy kinds bounded per symbol.
    let mut noisy: HashMap<(SymbolId, RefKind), usize> = HashMap::new();
    for (id, r) in out.refs {
        if matches!(r.kind, RefKind::Doc | RefKind::Comment) {
            let c = noisy.entry((id, r.kind)).or_default();
            *c += 1;
            if *c > MAX_NOISY_REFS {
                continue;
            }
        }
        if let Some(s) = symbols.get_mut(id.0 as usize) {
            s.refs.push(r);
        }
    }
    for s in symbols.iter_mut() {
        s.refs
            .sort_by_key(|r| (r.loc.file, r.loc.start, r.loc.end, r.kind));
        s.refs
            .dedup_by_key(|r| (r.loc.file, r.loc.start, r.loc.end, r.kind));
    }
}
