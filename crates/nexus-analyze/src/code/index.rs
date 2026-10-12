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
use super::imports::{self, ImportEdge, SegTarget};
use super::lang::Lang;
use super::model::SymbolKind;
use super::noncode::{self, OTHER_BASE, OtherFile};

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
    /// Manifests found while walking (`tsconfig.json`, `go.mod`, `Cargo.toml`, …), relative paths.
    pub config_files: Vec<String>,
    /// Config and doc files that were scanned for references (`Location.file >= OTHER_BASE`).
    pub others: Vec<OtherFile>,
    /// Resolved imports per file, aligned with `files`.
    pub imports: Vec<Vec<ImportEdge>>,
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
    /// Per-project ignore list lines (names with `*`, or `path:<prefix>`); matches are `RESERVED`.
    pub ignore: Vec<String>,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            max_files: 20_000,
            max_file_bytes: 1_500_000,
            skip_dirs: Vec::new(),
            ignore: Vec::new(),
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

    /// Relative path of a location's file (code, config, or doc file).
    pub fn path_of(&self, file: u32) -> Option<&str> {
        if file >= OTHER_BASE {
            self.others
                .get((file - OTHER_BASE) as usize)
                .map(|o| o.path.as_str())
        } else {
            self.files.get(file as usize).map(|f| f.path.as_str())
        }
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

/// (code files, warnings, manifest files, config/doc files to scan)
type Walked = (Vec<Found>, Vec<String>, Vec<String>, Vec<OtherFile>);

fn walk(root: &Path, opts: &IndexOptions, cancel: &CancellationToken) -> Result<Walked, IndexError> {
    let skip: HashSet<String> = SKIP_DIRS
        .iter()
        .map(|s| (*s).to_owned())
        .chain(opts.skip_dirs.iter().cloned())
        .collect();
    let mut found = Vec::new();
    let mut warnings = Vec::new();
    let mut configs = Vec::new();
    let mut others: Vec<OtherFile> = Vec::new();
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
        let Ok(rel) = entry.path().strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if imports::is_config_name(&entry.file_name().to_string_lossy()) {
            configs.push(rel.clone());
        }
        let Some(lang) = Lang::from_path(entry.path()) else {
            if let Some(kind) = noncode::other_kind(&entry.file_name().to_string_lossy())
                && noncode::wants(entry.metadata().map(|m| m.len()).unwrap_or(0), others.len())
            {
                others.push(OtherFile {
                    path: rel,
                    kind: kind.to_owned(),
                });
            }
            continue;
        };
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
    configs.sort();
    others.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((found, warnings, configs, others))
}

/// Build (or incrementally refresh) the index of `root`. Files whose hash matches `prev` are not parsed again.
pub fn build_index(
    root: &Path,
    opts: &IndexOptions,
    prev: Option<&CodeIndex>,
    cancel: &CancellationToken,
) -> Result<CodeIndex, IndexError> {
    let (found, mut warnings, config_files, others) = walk(root, opts, cancel)?;
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
    let cfg = imports::read_config(root, &config_files);
    let edges = imports::resolve_all(&files, &facts, &folders, &cfg);
    let mut symbols = link(&files, &facts, &folders, &edges);
    noncode::scan(
        &noncode::Project {
            root,
            files: &files,
            facts: &facts,
            folders: &folders,
            others: &others,
        },
        &mut symbols,
    );
    let mut index = CodeIndex {
        version: INDEX_VERSION,
        root: root.to_string_lossy().replace('\\', "/"),
        files,
        folders,
        symbols,
        warnings,
        reparsed: reparsed.load(Ordering::Relaxed),
        config_files,
        others,
        imports: edges,
        facts,
    };
    let rules: Vec<_> = opts
        .ignore
        .iter()
        .filter_map(|l| super::reserved::IgnoreRule::parse(l))
        .collect();
    super::reserved::apply(&mut index, &rules);
    Ok(index)
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

/// Whether two languages share names (JavaScript, TypeScript, and TSX do).
pub fn same_family(a: Lang, b: Lang) -> bool {
    family(a) == family(b)
}

fn family(lang: Lang) -> u8 {
    match lang {
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx => 0,
        Lang::Python => 1,
        Lang::Rust => 2,
        Lang::CSharp => 3,
        Lang::Go => 4,
        Lang::Java => 5,
        Lang::Kotlin => 6,
        Lang::Swift => 7,
        Lang::Php => 8,
        Lang::Ruby => 9,
        Lang::C | Lang::Cpp => 10,
        Lang::Lua => 11,
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
    name: String,
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

fn link(
    files: &[IndexedFile],
    facts: &[FileFacts],
    folders: &[String],
    edges: &[Vec<ImportEdge>],
) -> Vec<Symbol> {
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
                name: d.name.clone(),
                file: fi,
                kind: d.kind,
                scope: d.scope.clone(),
                visible: d.visible,
                lang: file.lang,
            });
        }
    }

    // 2. Files and folders are symbols too (their references are the imports that name them).
    let mut file_syms: Vec<SymbolId> = Vec::new();
    for (fi, f) in files.iter().enumerate() {
        let name = f.path.rsplit('/').next().unwrap_or(&f.path);
        let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
        let scope = f
            .path
            .rsplit_once('/')
            .map(|(d, _)| d.to_owned())
            .unwrap_or_default();
        let id = SymbolId(symbols.len() as u32);
        file_syms.push(id);
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
    let mut folder_syms: HashMap<&str, SymbolId> = HashMap::new();
    for dir in folders {
        let name = dir.rsplit('/').next().unwrap_or(dir);
        let scope = dir
            .rsplit_once('/')
            .map(|(d, _)| d.to_owned())
            .unwrap_or_default();
        let id = SymbolId(symbols.len() as u32);
        folder_syms.insert(dir.as_str(), id);
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
    let target_sym = |t: &SegTarget| -> Option<SymbolId> {
        match t {
            SegTarget::File(f) => file_syms.get(*f as usize).copied(),
            SegTarget::Folder(d) => folder_syms.get(d.as_str()).copied(),
        }
    };

    // 3. Import statements name files and folders.
    for (fi, es) in edges.iter().enumerate() {
        for e in es {
            for seg in &e.segs {
                if let Some(id) = target_sym(&seg.target) {
                    symbols[id.0 as usize].refs.push(Ref {
                        loc: Location {
                            file: fi as u32,
                            start: seg.span.start,
                            end: seg.span.end,
                            line: seg.span.line,
                        },
                        kind: RefKind::Import,
                        ambiguous: false,
                        is_def: false,
                    });
                }
            }
        }
    }

    // 4. Identifier occurrences.
    let lc = LinkCtx {
        files,
        facts,
        metas: &metas,
        by_name: &by_name,
        visible: edges
            .iter()
            .map(|es| es.iter().flat_map(|e| e.files.iter().copied()).collect())
            .collect(),
        aliases: edges
            .iter()
            .map(|es| {
                let mut m: HashMap<&str, Vec<&SegTarget>> = HashMap::new();
                for e in es {
                    for (n, t) in &e.aliases {
                        m.entry(n.as_str()).or_default().push(t);
                    }
                }
                m
            })
            .collect(),
    };
    for (fi, fa) in facts.iter().enumerate() {
        let fi = fi as u32;
        for r in &fa.refs {
            let res = lc.resolve(r, fi);
            let ambiguous = res.symbols.len() > 1;
            for id in res.symbols {
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
            for t in res.modules {
                if let Some(id) = target_sym(t) {
                    symbols[id.0 as usize].refs.push(Ref {
                        loc: loc(fi, &r.span),
                        kind: RefKind::Code,
                        ambiguous: false,
                        is_def: false,
                    });
                }
            }
        }
    }

    // 5. Stable, de-duplicated reference lists.
    for s in &mut symbols {
        s.refs.sort_by_key(|r| (r.loc.file, r.loc.start, r.loc.end));
        s.refs.dedup_by_key(|r| (r.loc.file, r.loc.start, r.loc.end));
    }
    symbols
}

/// What one identifier occurrence may mean.
struct Resolution<'a> {
    symbols: Vec<SymbolId>,
    /// Files/folders it names (a module alias used in code).
    modules: Vec<&'a SegTarget>,
}

struct LinkCtx<'a> {
    files: &'a [IndexedFile],
    facts: &'a [FileFacts],
    metas: &'a [Meta],
    by_name: &'a HashMap<String, Vec<SymbolId>>,
    /// Files whose top-level items each file can see through its imports.
    visible: Vec<HashSet<u32>>,
    aliases: Vec<HashMap<&'a str, Vec<&'a SegTarget>>>,
}

impl<'a> LinkCtx<'a> {
    fn m(&self, id: SymbolId) -> &Meta {
        &self.metas[id.0 as usize]
    }

    fn stem(&self, file: u32) -> &str {
        let name = self.files[file as usize].path.rsplit('/').next().unwrap_or("");
        name.rsplit_once('.').map_or(name, |(s, _)| s)
    }

    fn dir(&self, file: u32) -> &str {
        self.files[file as usize]
            .path
            .rsplit_once('/')
            .map(|(d, _)| d)
            .unwrap_or("")
    }

    fn pool(&self, name: &str, lang: Lang) -> Vec<SymbolId> {
        self.by_name
            .get(name)
            .map(|v| {
                v.iter()
                    .copied()
                    .filter(|id| family(self.m(*id).lang) == family(lang))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn resolve(&self, r: &RefFact, fi: u32) -> Resolution<'a> {
        let lang = self.files[fi as usize].lang;
        let none = Resolution {
            symbols: Vec::new(),
            modules: Vec::new(),
        };
        let pool = self.pool(&r.name, lang);
        let top_level = |x: &Meta| x.scope.is_empty() && x.visible.is_none();
        let is_loc = |x: &Meta| is_local_kind(x.kind) && x.visible.is_some();
        let sym = |v: Vec<SymbolId>| Resolution {
            symbols: v,
            modules: Vec::new(),
        };

        if r.member {
            let class = r.enclosing.iter().rev().find(|e| e.class);
            if let (Some(obj), Some(class)) = (&r.object, class)
                && is_self_name(obj)
            {
                let v: Vec<SymbolId> = pool
                    .iter()
                    .copied()
                    .filter(|id| {
                        let x = self.m(*id);
                        x.file == fi && is_member_kind(x.kind) && x.scope.last() == Some(&class.name)
                    })
                    .collect();
                if !v.is_empty() {
                    return sym(v);
                }
            }
            // `Type.member`, `module.item`, `package.Item`: look at what the receiver names.
            if let Some(obj) = r
                .object
                .as_deref()
                .filter(|o| o.chars().all(|c| c.is_alphanumeric() || c == '_'))
            {
                let recv = RefFact {
                    name: obj.to_owned(),
                    span: r.span.clone(),
                    member: false,
                    object: None,
                    import: false,
                    enclosing: r.enclosing.clone(),
                };
                let got = self.resolve(&recv, fi);
                let types: Vec<&Meta> = got
                    .symbols
                    .iter()
                    .map(|id| self.m(*id))
                    .filter(|x| {
                        matches!(
                            x.kind,
                            SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum | SymbolKind::Module
                        )
                    })
                    .collect();
                if !types.is_empty() {
                    let names: Vec<&str> = types.iter().map(|x| x.name.as_str()).collect();
                    let v: Vec<SymbolId> = pool
                        .iter()
                        .copied()
                        .filter(|id| {
                            let x = self.m(*id);
                            is_member_kind(x.kind)
                                && x.scope.last().is_some_and(|l| names.contains(&l.as_str()))
                        })
                        .collect();
                    if !v.is_empty() {
                        return sym(v);
                    }
                }
                if !got.modules.is_empty() {
                    let in_files: HashSet<u32> = got
                        .modules
                        .iter()
                        .flat_map(|t| match t {
                            SegTarget::File(f) => vec![*f],
                            SegTarget::Folder(d) => (0..self.files.len() as u32)
                                .filter(|f| self.dir(*f) == d)
                                .collect(),
                        })
                        .collect();
                    return sym(pool
                        .iter()
                        .copied()
                        .filter(|id| {
                            let x = self.m(*id);
                            in_files.contains(&x.file) && top_level(x) && !is_loc(x)
                        })
                        .collect());
                }
            }
            // Anything else: any member with that name, or a top-level item.
            return sym(pool
                .iter()
                .copied()
                .filter(|id| {
                    let x = self.m(*id);
                    (is_member_kind(x.kind) && !is_loc(x)) || (top_level(x) && !is_loc(x))
                })
                .collect());
        }

        // 1. Locals and parameters of the enclosing functions, innermost first.
        for e in r.enclosing.iter().rev().filter(|e| !e.class) {
            let v: Vec<SymbolId> = pool
                .iter()
                .copied()
                .filter(|id| {
                    let x = self.m(*id);
                    x.file == fi && is_local_kind(x.kind) && x.visible == Some((e.start, e.end))
                })
                .collect();
            if !v.is_empty() {
                return sym(v);
            }
        }
        // 2. Bare member access inside a class (Java, C#).
        if matches!(lang, Lang::Java | Lang::CSharp) {
            for e in r.enclosing.iter().rev().filter(|e| e.class) {
                let v: Vec<SymbolId> = pool
                    .iter()
                    .copied()
                    .filter(|id| {
                        let x = self.m(*id);
                        x.file == fi && is_member_kind(x.kind) && x.scope.last() == Some(&e.name)
                    })
                    .collect();
                if !v.is_empty() {
                    return sym(v);
                }
            }
        }
        // 3. Top level of the same file (and items nested in an enclosing function).
        let nested: Vec<&str> = r
            .enclosing
            .iter()
            .filter(|e| !e.class)
            .map(|e| e.name.as_str())
            .collect();
        let v: Vec<SymbolId> = pool
            .iter()
            .copied()
            .filter(|id| {
                let x = self.m(*id);
                x.file == fi
                    && !is_loc(x)
                    && (top_level(x) || x.scope.last().is_some_and(|l| nested.contains(&l.as_str())))
            })
            .collect();
        if !v.is_empty() {
            return sym(v);
        }
        // 4. Items of other files that this file can see: imports, same package/namespace.
        let package = self.facts[fi as usize].package.as_deref();
        let my_dir = self.dir(fi);
        let visible = &self.visible[fi as usize];
        let v: Vec<SymbolId> = pool
            .iter()
            .copied()
            .filter(|id| {
                let x = self.m(*id);
                if x.file == fi || !top_level(x) || is_loc(x) {
                    return false;
                }
                let seen = visible.contains(&x.file);
                match lang {
                    Lang::Go => seen || self.dir(x.file) == my_dir,
                    Lang::Java | Lang::Kotlin | Lang::Php => {
                        seen || (package.is_some()
                            && package == self.facts[x.file as usize].package.as_deref())
                    }
                    // Swift modules and C# namespaces are visible as a whole (one module per project here).
                    Lang::CSharp | Lang::Swift => true,
                    // A C/C++ header's functions are defined in the source file next to it.
                    Lang::C | Lang::Cpp => {
                        let pair =
                            |a: u32, b: u32| self.dir(a) == self.dir(b) && self.stem(a) == self.stem(b);
                        seen || pair(fi, x.file) || visible.iter().any(|&v| pair(v, x.file))
                    }
                    _ => seen,
                }
            })
            .collect();
        if !v.is_empty() {
            return sym(v);
        }
        // 5. A module alias used in code (`pricing.calc_total`, `pricing::x`, `fmt.Println`).
        match self.aliases[fi as usize].get(r.name.as_str()) {
            Some(t) => Resolution {
                symbols: Vec::new(),
                modules: t.clone(),
            },
            None => none,
        }
    }
}
