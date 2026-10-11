//! Find (M09, T09.11, ANALYZER §5.9): symbol search, usages, and a project overview over the code index.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use super::index::{CodeIndex, Flags, RefKind, SymbolId};
use super::model::SymbolKind;
use super::propose::Sources;

/// Occurrences of a symbol by kind (the definition is not counted).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct UsageCounts {
    pub code: u32,
    pub import: u32,
    pub config: u32,
    pub string: u32,
    pub doc: u32,
    pub comment: u32,
}

impl UsageCounts {
    pub fn total(&self) -> u32 {
        self.code + self.import + self.config + self.string + self.doc + self.comment
    }
}

/// How well a name matched the query (best first).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    Exact,
    Prefix,
    Substring,
    Fuzzy,
}

/// One search result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Found {
    pub symbol: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    /// Project-relative path (for files and folders: the path itself).
    pub path: String,
    pub line: u32,
    pub signature: String,
    pub doc: Option<String>,
    pub matched: MatchKind,
    pub usages: UsageCounts,
}

/// One occurrence with a one-line preview.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Usage {
    pub path: String,
    pub line: u32,
    pub kind: RefKind,
    pub ambiguous: bool,
    pub is_def: bool,
    pub preview: String,
}

fn subsequence(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|h| h == c))
}

fn counts(index: &CodeIndex, id: SymbolId) -> UsageCounts {
    let mut c = UsageCounts::default();
    if let Some(s) = index.symbol(id) {
        for r in s.refs.iter().filter(|r| !r.is_def) {
            match r.kind {
                RefKind::Code => c.code += 1,
                RefKind::Import => c.import += 1,
                RefKind::ConfigFile => c.config += 1,
                RefKind::StringLiteral => c.string += 1,
                RefKind::Doc => c.doc += 1,
                RefKind::Comment => c.comment += 1,
            }
        }
    }
    c
}

/// Search symbols, files and folders by name: exact, prefix, substring, then fuzzy (subsequence);
/// case-insensitive. Within a match class, more-used symbols come first.
pub fn find_symbols(index: &CodeIndex, query: &str, limit: usize) -> Vec<Found> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(MatchKind, u32, SymbolId)> = Vec::new();
    for s in &index.symbols {
        if s.kind == SymbolKind::Parameter {
            continue;
        }
        let n = s.name.to_lowercase();
        let m = if n == q {
            MatchKind::Exact
        } else if n.starts_with(&q) {
            MatchKind::Prefix
        } else if n.contains(&q) {
            MatchKind::Substring
        } else if q.len() >= 3 && subsequence(&q, &n) {
            MatchKind::Fuzzy
        } else {
            continue;
        };
        hits.push((m, counts(index, s.id).total(), s.id));
    }
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    hits.truncate(limit);
    hits.into_iter()
        .filter_map(|(m, _, id)| {
            let s = index.symbol(id)?;
            let path = match s.kind {
                SymbolKind::File | SymbolKind::Folder => s.signature.clone(),
                _ => index.path_of(s.def.file).unwrap_or_default().to_owned(),
            };
            Some(Found {
                symbol: id,
                name: s.name.clone(),
                kind: s.kind,
                path,
                line: s.def.line,
                signature: s.signature.clone(),
                doc: s.doc.clone(),
                matched: m,
                usages: counts(index, id),
            })
        })
        .collect()
}

/// Every occurrence of a symbol, grouped by file (sorted by path, then line).
pub fn usages(index: &CodeIndex, id: SymbolId) -> Vec<Usage> {
    let Some(s) = index.symbol(id) else {
        return Vec::new();
    };
    let src = Sources::new(index);
    let mut out: Vec<Usage> = s
        .refs
        .iter()
        .filter_map(|r| {
            Some(Usage {
                path: index.path_of(r.loc.file)?.to_owned(),
                line: r.loc.line,
                kind: r.kind,
                ambiguous: r.ambiguous,
                is_def: r.is_def,
                preview: src.line(r.loc.file, r.loc.line),
            })
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
    out
}

/// A file with a number attached (size in bytes, or how often it is imported).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct FileStat {
    pub path: String,
    pub value: u64,
}

/// The project at a glance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Overview {
    /// `(language, files)`, most files first.
    pub languages: Vec<(String, u32)>,
    pub files: u32,
    pub folders: u32,
    pub symbols: u32,
    /// Files that define an entry point (`main`, …).
    pub entry_points: Vec<String>,
    pub largest_files: Vec<FileStat>,
    /// Files imported by the most other files.
    pub most_imported: Vec<FileStat>,
}

pub fn overview(index: &CodeIndex, top: usize) -> Overview {
    let mut langs: BTreeMap<&str, u32> = BTreeMap::new();
    for f in &index.files {
        *langs.entry(f.lang.name()).or_default() += 1;
    }
    let mut languages: Vec<(String, u32)> = langs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    languages.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let mut entry_points: Vec<String> = index
        .symbols
        .iter()
        .filter(|s| {
            s.flags.has(Flags::ENTRY_POINT) && !matches!(s.kind, SymbolKind::File | SymbolKind::Folder)
        })
        .filter_map(|s| index.path_of(s.def.file).map(str::to_owned))
        .collect();
    entry_points.sort();
    entry_points.dedup();

    let mut largest: Vec<FileStat> = index
        .files
        .iter()
        .map(|f| FileStat {
            path: f.path.clone(),
            value: f.size,
        })
        .collect();
    largest.sort_by(|a, b| b.value.cmp(&a.value).then(a.path.cmp(&b.path)));
    largest.truncate(top);

    let mut imported: HashMap<u32, std::collections::HashSet<u32>> = HashMap::new();
    for (importer, edges) in index.imports.iter().enumerate() {
        for e in edges {
            for f in &e.files {
                if *f as usize != importer {
                    imported.entry(*f).or_default().insert(importer as u32);
                }
            }
        }
    }
    let mut most: Vec<FileStat> = imported
        .into_iter()
        .filter_map(|(f, by)| {
            Some(FileStat {
                path: index.file(f)?.path.clone(),
                value: by.len() as u64,
            })
        })
        .collect();
    most.sort_by(|a, b| b.value.cmp(&a.value).then(a.path.cmp(&b.path)));
    most.truncate(top);

    Overview {
        languages,
        files: index.files.len() as u32,
        folders: index.folders.len() as u32,
        symbols: index
            .symbols
            .iter()
            .filter(|s| {
                !matches!(
                    s.kind,
                    SymbolKind::File | SymbolKind::Folder | SymbolKind::Parameter
                )
            })
            .count() as u32,
        entry_points,
        largest_files: largest,
        most_imported: most,
    }
}

/// Text for the `find_symbol` tool: matches with usage counts, then the usages of the best match,
/// cut to about `max_chars`.
pub fn render_find(index: &CodeIndex, query: &str, max_chars: usize) -> String {
    let found = find_symbols(index, query, 8);
    if found.is_empty() {
        return format!("No symbol, file or folder matches `{query}`.\n");
    }
    let mut out = String::new();
    for f in &found {
        let u = f.usages;
        out.push_str(&format!(
            "{} {} ({}:{}) — {} uses: {} code, {} import, {} config, {} string\n",
            f.kind.label(),
            f.name,
            f.path,
            f.line,
            u.total(),
            u.code,
            u.import,
            u.config,
            u.string
        ));
        if !f.signature.is_empty() && !matches!(f.kind, SymbolKind::File | SymbolKind::Folder) {
            out.push_str(&format!("  {}\n", f.signature));
        }
    }
    let best = &found[0];
    out.push_str(&format!("\nUsages of {}:\n", best.name));
    let mut last = String::new();
    for u in usages(index, best.symbol) {
        if u.path != last {
            out.push_str(&format!("{}\n", u.path));
            last = u.path.clone();
        }
        let mark = if u.is_def {
            " (definition)"
        } else if u.ambiguous {
            " (maybe)"
        } else {
            ""
        };
        out.push_str(&format!("  {}: {}{}\n", u.line, u.preview, mark));
        if out.len() > max_chars {
            let mut cut = max_chars;
            while !out.is_char_boundary(cut) {
                cut -= 1;
            }
            out.truncate(cut);
            out.push_str("\n… (more usages not shown)\n");
            break;
        }
    }
    out
}
