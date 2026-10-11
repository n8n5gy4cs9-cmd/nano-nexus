//! The symbol index (ANALYZER §5.2): definitions, references, scopes, file and folder symbols,
//! ambiguity, serialization, and incremental re-indexing.
//!
//! Resolution is name + scope based, not a type checker: locals and parameters resolve inside their
//! function, `self.x` inside the class, other members and top-level names through the project (and, from
//! T09.05, through the import graph). When one name can mean several symbols the reference is
//! attached to each of them and marked `ambiguous`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use ignore::WalkBuilder;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::file_facts::{self, FileFacts, RefFact, is_self_name};
use super::lang::Lang;
use super::model::SymbolKind;

/// Bump when the on-disk layout changes; an old cache is simply rebuilt.
pub const INDEX_VERSION: u32 = 1;

/// A symbol id (index into [`CodeIndex::symbols`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
pub struct SymbolId(pub u32);

/// A byte range in an indexed file (`file` indexes [`CodeIndex::files`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
pub struct Location {
    pub file: u32,
    pub start: u32,
    pub end: u32,
    pub line: u32,
}

/// What kind of occurrence a reference is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    Code,
    Import,
    StringLiteral,
    ConfigFile,
    Doc,
    Comment,
}

/// One occurrence of a symbol (the definition itself included, `is_def`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Ref {
    pub loc: Location,
    pub kind: RefKind,
    /// The name could also mean another symbol: the user must confirm it.
    pub ambiguous: bool,
    pub is_def: bool,
}

/// Symbol flags (bit set).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Flags(pub u16);

impl Flags {
    pub const RESERVED: u16 = 1;
    pub const EXPORTED: u16 = 2;
    pub const PUBLIC_API: u16 = 4;
    pub const DYNAMIC_REFS: u16 = 8;
    pub const OVERRIDE: u16 = 16;
    pub const TEST: u16 = 32;
    pub const ENTRY_POINT: u16 = 64;
    pub const AMBIGUOUS: u16 = 128;

    pub fn has(self, bit: u16) -> bool {
        self.0 & bit != 0
    }

    pub fn set(&mut self, bit: u16) {
        self.0 |= bit;
    }
}

/// A named thing in the project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub lang: Option<Lang>,
    pub def: Location,
    /// Enclosing containers joined by `::` (`Cart::total`), empty at the top level.
    pub scope: String,
    pub signature: String,
    pub doc: Option<String>,
    pub refs: Vec<Ref>,
    pub flags: Flags,
    /// Visibility keyword from the syntax (`pub`, `public`, `export`, …).
    pub visibility: Option<String>,
}

/// A code file in the index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct IndexedFile {
    /// Path relative to the project root, with `/` separators.
    pub path: String,
    pub lang: Lang,
    /// blake3 of the bytes (hex).
    pub hash: String,
    pub size: u64,
}

/// The whole index of one project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndex {
    pub version: u32,
    pub root: String,
    pub files: Vec<IndexedFile>,
    /// Folders that contain indexed code (relative, `/`), without the root.
    pub folders: Vec<String>,
    pub symbols: Vec<Symbol>,
    pub warnings: Vec<String>,
    /// How many files were parsed in the last build (the rest came from the cache).
    pub reparsed: u32,
    /// Per-file parse results, aligned with `files`.
    pub facts: Vec<FileFacts>,
}

/// Why indexing stopped.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("indexing was cancelled")]
    Cancelled,
    #[error("could not read the project: {0}")]
    Io(String),
    #[error("could not read or write the index cache: {0}")]
    Cache(String),
}

/// Limits and extra ignores.
#[derive(Clone, Debug)]
pub struct IndexOptions {
    pub max_files: usize,
    pub max_file_bytes: u64,
    /// Directory names to skip in addition to the built-in list.
    pub skip_dirs: Vec<String>,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            max_files: 20_000,
            max_file_bytes: 1_500_000,
            skip_dirs: Vec::new(),
        }
    }
}

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "vendor",
    "dist",
    "build",
    "out",
    "bin",
    "obj",
    "__pycache__",
    ".venv",
    "venv",
    ".idea",
    ".vscode",
    ".next",
    ".gradle",
    "Pods",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
];

impl CodeIndex {
    pub fn file(&self, id: u32) -> Option<&IndexedFile> {
        self.files.get(id as usize)
    }

    pub fn symbol(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(id.0 as usize)
    }

    /// All symbols with exactly this name.
    pub fn symbols_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Symbol> + 'a {
        self.symbols.iter().filter(move |s| s.name == name)
    }

    /// Write to `path` (postcard).
    pub fn save(&self, path: &Path) -> Result<(), IndexError> {
        let bytes = postcard::to_stdvec(self).map_err(|e| IndexError::Cache(e.to_string()))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| IndexError::Cache(e.to_string()))?;
        }
        std::fs::write(path, bytes).map_err(|e| IndexError::Cache(e.to_string()))
    }

    /// Read a saved index. A wrong version or damaged file is an error (the caller rebuilds).
    pub fn load(path: &Path) -> Result<CodeIndex, IndexError> {
        let bytes = std::fs::read(path).map_err(|e| IndexError::Cache(e.to_string()))?;
        let idx: CodeIndex = postcard::from_bytes(&bytes).map_err(|e| IndexError::Cache(e.to_string()))?;
        if idx.version != INDEX_VERSION {
            return Err(IndexError::Cache(format!(
                "index version {} is not {INDEX_VERSION}",
                idx.version
            )));
        }
        Ok(idx)
    }
}

struct Found {
    rel: String,
    abs: PathBuf,
    lang: Lang,
    size: u64,
}

fn walk(
    root: &Path,
    opts: &IndexOptions,
    cancel: &CancellationToken,
) -> Result<(Vec<Found>, Vec<String>), IndexError> {
    let skip: HashSet<String> = SKIP_DIRS
        .iter()
        .map(|s| (*s).to_owned())
        .chain(opts.skip_dirs.iter().cloned())
        .collect();
    let mut found = Vec::new();
    let mut warnings = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .require_git(false)
        .filter_entry(move |e| {
            let is_dir = e.file_type().is_some_and(|t| t.is_dir());
            !(is_dir && e.depth() > 0 && skip.contains(e.file_name().to_string_lossy().as_ref()))
        })
        .build();
    for entry in walker {
        if cancel.is_cancelled() {
            return Err(IndexError::Cancelled);
        }
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warnings.push(format!("skipped: {e}"));
                continue;
            }
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Some(lang) = Lang::from_path(entry.path()) else {
            continue;
        };
        let Ok(rel) = entry.path().strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if size > opts.max_file_bytes {
            warnings.push(format!(
                "{rel}: skipped (larger than {} bytes)",
                opts.max_file_bytes
            ));
            continue;
        }
        if found.len() >= opts.max_files {
            warnings.push(format!("stopped at {} files", opts.max_files));
            break;
        }
        found.push(Found {
            rel,
            abs: entry.path().to_path_buf(),
            lang,
            size,
        });
    }
    found.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok((found, warnings))
}

/// Build (or incrementally refresh) the index of `root`. Files whose hash matches `prev` are not parsed again.
pub fn build_index(
    root: &Path,
    opts: &IndexOptions,
    prev: Option<&CodeIndex>,
    cancel: &CancellationToken,
) -> Result<CodeIndex, IndexError> {
    let (found, mut warnings) = walk(root, opts, cancel)?;
    let cached: HashMap<&str, (&IndexedFile, &FileFacts)> = prev
        .filter(|p| p.version == INDEX_VERSION)
        .map(|p| {
            p.files
                .iter()
                .zip(p.facts.iter())
                .map(|(f, facts)| (f.path.as_str(), (f, facts)))
                .collect()
        })
        .unwrap_or_default();
    let reparsed = std::sync::atomic::AtomicU32::new(0);

    let results: Vec<Result<Option<(IndexedFile, FileFacts)>, String>> = found
        .par_iter()
        .map(|f| {
            if cancel.is_cancelled() {
                return Ok(None);
            }
            let bytes = std::fs::read(&f.abs).map_err(|e| format!("{}: {e}", f.rel))?;
            if std::str::from_utf8(&bytes).is_err() {
                return Err(format!("{}: skipped (not UTF-8)", f.rel));
            }
            let hash = blake3::hash(&bytes).to_hex().to_string();
            if let Some((old, facts)) = cached.get(f.rel.as_str())
                && old.hash == hash
            {
                return Ok(Some((
                    IndexedFile {
                        path: f.rel.clone(),
                        lang: f.lang,
                        hash,
                        size: f.size,
                    },
                    (*facts).clone(),
                )));
            }
            reparsed.fetch_add(1, Ordering::Relaxed);
            let facts = file_facts::extract(f.lang, &bytes)
                .ok_or_else(|| format!("{}: could not be parsed", f.rel))?;
            Ok(Some((
                IndexedFile {
                    path: f.rel.clone(),
                    lang: f.lang,
                    hash,
                    size: bytes.len() as u64,
                },
                facts,
            )))
        })
        .collect();
    if cancel.is_cancelled() {
        return Err(IndexError::Cancelled);
    }
    let mut files = Vec::new();
    let mut facts = Vec::new();
    for r in results {
        match r {
            Ok(Some((f, fa))) => {
                files.push(f);
                facts.push(fa);
            }
            Ok(None) => {}
            Err(w) => warnings.push(w),
        }
    }
    let folders = folders_of(&files);
    let symbols = link(&files, &facts, &folders);
    Ok(CodeIndex {
        version: INDEX_VERSION,
        root: root.to_string_lossy().replace('\\', "/"),
        files,
        folders,
        symbols,
        warnings,
        reparsed: reparsed.load(Ordering::Relaxed),
        facts,
    })
}

fn folders_of(files: &[IndexedFile]) -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    for f in files {
        let mut cur = f.path.as_str();
        while let Some((dir, _)) = cur.rsplit_once('/') {
            set.insert(dir.to_owned());
            cur = dir;
        }
    }
    set.into_iter().collect()
}

fn family(lang: Lang) -> u8 {
    match lang {
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx => 0,
        Lang::Python => 1,
        Lang::Rust => 2,
        Lang::CSharp => 3,
        Lang::Go => 4,
        Lang::Java => 5,
    }
}

fn is_local_kind(k: SymbolKind) -> bool {
    matches!(
        k,
        SymbolKind::Variable | SymbolKind::Parameter | SymbolKind::Constant
    )
}

fn is_member_kind(k: SymbolKind) -> bool {
    matches!(
        k,
        SymbolKind::Method | SymbolKind::Field | SymbolKind::EnumMember | SymbolKind::Constant
    )
}

/// Internal per-symbol data used while linking.
struct Meta {
    file: u32,
    kind: SymbolKind,
    scope: Vec<String>,
    visible: Option<(u32, u32)>,
    lang: Lang,
}

/// Key under which repeated definitions of the same thing (reassignments, overloads, `self.x = …` in
/// several methods) are merged into one symbol.
fn merge_key(file: u32, d: &super::file_facts::DefFact) -> (u32, String, String, String) {
    let cat = if is_local_kind(d.kind) && d.visible.is_some() {
        format!("local@{}", d.visible.map(|v| v.0).unwrap_or(0))
    } else {
        d.kind.label().to_owned()
    };
    (file, d.scope.join("::"), d.name.clone(), cat)
}

fn loc(file: u32, s: &super::file_facts::Span) -> Location {
    Location {
        file,
        start: s.start,
        end: s.end,
        line: s.line,
    }
}

fn is_go_exported(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

/// Last path/dotted segment of an import specifier: `shop.pricing.Calc` -> `Calc`, `"./lib/x"` -> `x`.
fn last_segment(spec: &str) -> String {
    let t = spec.trim_matches(['"', '\'', '`']);
    t.rsplit(['.', '/', ':', '\\'])
        .find(|s| !s.is_empty() && *s != "*")
        .unwrap_or("")
        .to_owned()
}

fn link(files: &[IndexedFile], facts: &[FileFacts], folders: &[String]) -> Vec<Symbol> {
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut metas: Vec<Meta> = Vec::new();
    let mut keys: HashMap<(u32, String, String, String), SymbolId> = HashMap::new();
    let mut by_name: HashMap<String, Vec<SymbolId>> = HashMap::new();

    // 1. Definitions.
    for (fi, (file, fa)) in files.iter().zip(facts.iter()).enumerate() {
        let fi = fi as u32;
        for d in &fa.defs {
            let key = merge_key(fi, d);
            let def_ref = Ref {
                loc: loc(fi, &d.name_span),
                kind: RefKind::Code,
                ambiguous: false,
                is_def: true,
            };
            if let Some(&id) = keys.get(&key) {
                symbols[id.0 as usize].refs.push(def_ref);
                continue;
            }
            let id = SymbolId(symbols.len() as u32);
            keys.insert(key, id);
            by_name.entry(d.name.clone()).or_default().push(id);
            let mut flags = Flags::default();
            if d.overrides {
                flags.set(Flags::OVERRIDE);
            }
            let exported = match (&d.visibility, file.lang) {
                (Some(v), _) if matches!(v.as_str(), "pub" | "public" | "export") => true,
                (_, Lang::Go) => d.scope.is_empty() && is_go_exported(&d.name),
                (_, Lang::Python | Lang::JavaScript) => {
                    d.scope.is_empty() && d.visible.is_none() && !d.name.starts_with('_')
                }
                _ => false,
            };
            if exported {
                flags.set(Flags::EXPORTED);
                if !is_local_kind(d.kind) || d.visible.is_none() {
                    flags.set(Flags::PUBLIC_API);
                }
            }
            symbols.push(Symbol {
                id,
                name: d.name.clone(),
                kind: d.kind,
                lang: Some(file.lang),
                def: loc(fi, &d.name_span),
                scope: d.scope.join("::"),
                signature: d.signature.clone(),
                doc: d.doc.clone(),
                refs: vec![def_ref],
                flags,
                visibility: d.visibility.clone(),
            });
            metas.push(Meta {
                file: fi,
                kind: d.kind,
                scope: d.scope.clone(),
                visible: d.visible,
                lang: file.lang,
            });
        }
    }

    // 2. References.
    for (fi, (file, fa)) in files.iter().zip(facts.iter()).enumerate() {
        let fi = fi as u32;
        let mut imported: HashSet<String> = fa.import_names.iter().map(|t| t.text.clone()).collect();
        imported.extend(fa.import_modules.iter().map(|t| last_segment(&t.text)));
        let dir = file.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        for r in &fa.refs {
            let Some(cands) = by_name.get(&r.name) else {
                continue;
            };
            let hits = resolve(r, fi, file.lang, fa, dir, &imported, cands, &metas, files, facts);
            let ambiguous = hits.len() > 1;
            for id in hits {
                let sym = &mut symbols[id.0 as usize];
                sym.refs.push(Ref {
                    loc: loc(fi, &r.span),
                    kind: if r.import { RefKind::Import } else { RefKind::Code },
                    ambiguous,
                    is_def: false,
                });
                if ambiguous {
                    sym.flags.set(Flags::AMBIGUOUS);
                }
            }
        }
    }

    // 3. Files and folders are symbols too.
    for (fi, f) in files.iter().enumerate() {
        let name = f.path.rsplit('/').next().unwrap_or(&f.path);
        let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
        let scope = f
            .path
            .rsplit_once('/')
            .map(|(d, _)| d.to_owned())
            .unwrap_or_default();
        let id = SymbolId(symbols.len() as u32);
        symbols.push(Symbol {
            id,
            name: stem.to_owned(),
            kind: SymbolKind::File,
            lang: Some(f.lang),
            def: Location {
                file: fi as u32,
                start: 0,
                end: 0,
                line: 1,
            },
            scope,
            signature: f.path.clone(),
            doc: None,
            refs: Vec::new(),
            flags: Flags::default(),
            visibility: None,
        });
    }
    for dir in folders {
        let name = dir.rsplit('/').next().unwrap_or(dir);
        let scope = dir
            .rsplit_once('/')
            .map(|(d, _)| d.to_owned())
            .unwrap_or_default();
        let id = SymbolId(symbols.len() as u32);
        symbols.push(Symbol {
            id,
            name: name.to_owned(),
            kind: SymbolKind::Folder,
            lang: None,
            def: Location {
                file: 0,
                start: 0,
                end: 0,
                line: 1,
            },
            scope,
            signature: dir.clone(),
            doc: None,
            refs: Vec::new(),
            flags: Flags::default(),
            visibility: None,
        });
    }

    // 4. Stable, de-duplicated reference lists.
    for s in &mut symbols {
        s.refs.sort_by_key(|r| (r.loc.file, r.loc.start, r.loc.end));
        s.refs.dedup_by_key(|r| (r.loc.file, r.loc.start, r.loc.end));
    }
    symbols
}

/// Resolve one identifier occurrence to the symbols it may mean (0 = external, 1 = sure, >1 = ambiguous).
#[allow(clippy::too_many_arguments)]
fn resolve(
    r: &RefFact,
    fi: u32,
    lang: Lang,
    fa: &FileFacts,
    dir: &str,
    imported: &HashSet<String>,
    cands: &[SymbolId],
    metas: &[Meta],
    files: &[IndexedFile],
    facts: &[FileFacts],
) -> Vec<SymbolId> {
    let fam = family(lang);
    let pool: Vec<SymbolId> = cands
        .iter()
        .copied()
        .filter(|id| family(metas[id.0 as usize].lang) == fam)
        .collect();
    let m = |id: &SymbolId| &metas[id.0 as usize];
    let top_level = |x: &Meta| x.scope.is_empty() && x.visible.is_none();
    let is_loc = |x: &Meta| is_local_kind(x.kind) && x.visible.is_some();
    let first = |v: Vec<SymbolId>| if v.is_empty() { None } else { Some(v) };

    if r.member {
        let class = r.enclosing.iter().rev().find(|e| e.class);
        // `self.x` / `this.x`: a member of the enclosing class.
        if let (Some(obj), Some(class)) = (&r.object, class)
            && is_self_name(obj)
            && let Some(v) = first(
                pool.iter()
                    .copied()
                    .filter(|id| {
                        let x = m(id);
                        x.file == fi && is_member_kind(x.kind) && x.scope.last() == Some(&class.name)
                    })
                    .collect(),
            )
        {
            return v;
        }
        // Anything else: any member with that name, or a top-level item reached through a module/type name.
        return pool
            .iter()
            .copied()
            .filter(|id| {
                let x = m(id);
                (is_member_kind(x.kind) && !is_loc(x)) || (top_level(x) && !is_loc(x))
            })
            .collect();
    }

    // 1. Locals and parameters of the enclosing functions, innermost first.
    for e in r.enclosing.iter().rev().filter(|e| !e.class) {
        if let Some(v) = first(
            pool.iter()
                .copied()
                .filter(|id| {
                    let x = m(id);
                    x.file == fi && is_local_kind(x.kind) && x.visible == Some((e.start, e.end))
                })
                .collect(),
        ) {
            return v;
        }
    }
    // 2. Bare member access inside a class (Java, C#, TypeScript without `this`).
    if matches!(lang, Lang::Java | Lang::CSharp) {
        for e in r.enclosing.iter().rev().filter(|e| e.class) {
            if let Some(v) = first(
                pool.iter()
                    .copied()
                    .filter(|id| {
                        let x = m(id);
                        x.file == fi && is_member_kind(x.kind) && x.scope.last() == Some(&e.name)
                    })
                    .collect(),
            ) {
                return v;
            }
        }
    }
    // 3. Top level of the same file (and nested items of an enclosing function).
    let nested: Vec<&str> = r
        .enclosing
        .iter()
        .filter(|e| !e.class)
        .map(|e| e.name.as_str())
        .collect();
    if let Some(v) = first(
        pool.iter()
            .copied()
            .filter(|id| {
                let x = m(id);
                x.file == fi
                    && !is_loc(x)
                    && (top_level(x) || x.scope.last().is_some_and(|l| nested.contains(&l.as_str())))
            })
            .collect(),
    ) {
        return v;
    }
    // 4. Items visible from other files: imported by name, or in the same package/namespace.
    let package = fa.package.as_deref();
    pool.iter()
        .copied()
        .filter(|id| {
            let x = m(id);
            if x.file == fi || !top_level(x) || is_loc(x) {
                return false;
            }
            let other = &facts[x.file as usize];
            let same_dir = files[x.file as usize]
                .path
                .rsplit_once('/')
                .map(|(d, _)| d)
                .unwrap_or("")
                == dir;
            match lang {
                Lang::Go => same_dir || imported.contains(&r.name),
                Lang::Java => {
                    imported.contains(&r.name) || (package.is_some() && package == other.package.as_deref())
                }
                Lang::CSharp => true,
                _ => r.import || imported.contains(&r.name),
            }
        })
        .collect()
}
