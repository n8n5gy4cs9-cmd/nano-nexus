//! Import and module resolution per language (ANALYZER §5.3). For every import statement we work out
//! which project file or folder it names, which path segments of the specifier name a file or folder
//! (so renaming that file or folder can rewrite them), and which files become visible to the importer.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::file_facts::{FileFacts, ImportFact, Span, TextFact};
use super::index::IndexedFile;
use super::lang::Lang;

/// What a path segment of an import specifier names.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SegTarget {
    File(u32),
    Folder(String),
}

/// One segment of an import specifier that names a file or folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegRef {
    /// Absolute span in the importing file (extension excluded for files).
    pub span: Span,
    pub target: SegTarget,
}

/// A resolved import statement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportEdge {
    pub spec: String,
    pub span: Span,
    pub segs: Vec<SegRef>,
    /// Files whose top-level items the importer can now see.
    pub files: Vec<u32>,
    /// Local names that stand for a file or folder (`from util import pricing`, `mod x;`, Go package names).
    pub aliases: Vec<(String, SegTarget)>,
}

/// `tsconfig.json` / `jsconfig.json` path settings.
#[derive(Clone, Debug, Default)]
pub struct TsConfig {
    pub dir: String,
    pub base_url: Option<String>,
    pub paths: Vec<(String, Vec<String>)>,
}

/// Settings read from manifests.
#[derive(Clone, Debug, Default)]
pub struct ProjectConfig {
    pub ts: Vec<TsConfig>,
    /// `(directory of go.mod, module path)`.
    pub go: Vec<(String, String)>,
    /// Directories that contain a `Cargo.toml`.
    pub cargo: Vec<String>,
}

/// Names of the manifest files the index remembers.
pub fn is_config_name(name: &str) -> bool {
    matches!(name, "tsconfig.json" | "jsconfig.json" | "go.mod" | "Cargo.toml")
}

/// Remove `//` and `/* */` comments and trailing commas (tsconfig is JSON with comments).
pub fn strip_jsonc(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let (mut i, mut in_str) = (0, false);
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 1;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        } else if c == '/' && b.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        } else if c == ',' {
            // Trailing comma: next non-space char closes the container.
            let mut j = i + 1;
            while j < b.len() && b[j].is_whitespace() {
                j += 1;
            }
            if !matches!(b.get(j), Some('}' | ']')) {
                out.push(c);
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// Join and normalise (`..`, `.`). `None` if the result escapes the project root.
pub fn join(base: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// Read the manifests listed in `config_files` (paths relative to `root`).
pub fn read_config(root: &Path, config_files: &[String]) -> ProjectConfig {
    let mut cfg = ProjectConfig::default();
    for rel in config_files {
        let dir = dir_of(rel).to_owned();
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        match name {
            "Cargo.toml" => cfg.cargo.push(dir),
            "go.mod" => {
                if let Some(m) = text
                    .lines()
                    .find_map(|l| l.trim().strip_prefix("module "))
                    .map(|m| m.trim().trim_matches('"').to_owned())
                {
                    cfg.go.push((dir, m));
                }
            }
            _ => {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&strip_jsonc(&text)) else {
                    continue;
                };
                let opts = &v["compilerOptions"];
                let paths = opts["paths"]
                    .as_object()
                    .map(|m| {
                        m.iter()
                            .map(|(k, v)| {
                                let targets = v
                                    .as_array()
                                    .map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_owned)).collect())
                                    .unwrap_or_default();
                                (k.clone(), targets)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                cfg.ts.push(TsConfig {
                    dir,
                    base_url: opts["baseUrl"].as_str().map(str::to_owned),
                    paths,
                });
            }
        }
    }
    cfg
}

struct Ctx<'a> {
    files: &'a [IndexedFile],
    facts: &'a [FileFacts],
    by_path: HashMap<&'a str, u32>,
    folders: HashSet<&'a str>,
    cfg: &'a ProjectConfig,
}

fn sp(base: &Span, from: usize, to: usize) -> Span {
    Span {
        start: base.start + from as u32,
        end: base.start + to as u32,
        line: base.line,
    }
}

/// Split `text` on `sep` into `(start, end)` byte ranges (non-empty parts only).
fn parts(text: &str, sep: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut pos = 0;
    for piece in text.split(sep) {
        if !piece.is_empty() {
            out.push((pos, pos + piece.len()));
        }
        pos += piece.len() + sep.len();
    }
    out
}

fn edge(f: &TextFact) -> ImportEdge {
    ImportEdge {
        spec: f.text.clone(),
        span: f.span.clone(),
        segs: Vec::new(),
        files: Vec::new(),
        aliases: Vec::new(),
    }
}

/// Resolve every import of every file.
pub fn resolve_all(
    files: &[IndexedFile],
    facts: &[FileFacts],
    folders: &[String],
    cfg: &ProjectConfig,
) -> Vec<Vec<ImportEdge>> {
    let ctx = Ctx {
        files,
        facts,
        by_path: files
            .iter()
            .enumerate()
            .map(|(i, f)| (f.path.as_str(), i as u32))
            .collect(),
        folders: folders.iter().map(String::as_str).collect(),
        cfg,
    };
    files
        .iter()
        .zip(facts.iter())
        .map(|(f, fa)| {
            fa.imports
                .iter()
                .map(|imp| match f.lang {
                    Lang::Python => python(&ctx, f, imp),
                    Lang::JavaScript | Lang::TypeScript | Lang::Tsx => js(&ctx, f, imp),
                    Lang::Rust => rust(&ctx, f, imp),
                    Lang::Go => go(&ctx, f, imp),
                    Lang::Java => java(&ctx, imp, &["java"]),
                    Lang::Kotlin => java(&ctx, imp, &["kt", "kts", "java"]),
                    Lang::CSharp | Lang::Swift => edge(&imp.module),
                    Lang::Php => path_import(&ctx, f, imp, PHP_EXTS, &["", "src", "lib"]),
                    Lang::Ruby => path_import(&ctx, f, imp, &["rb"], &["", "lib"]),
                    Lang::C | Lang::Cpp => path_import(&ctx, f, imp, C_EXTS, &["", "include", "src", "inc"]),
                    Lang::Lua => lua(&ctx, imp),
                })
                .collect()
        })
        .collect()
}

fn files_in_folder(ctx: &Ctx<'_>, folder: &str) -> Vec<u32> {
    ctx.files
        .iter()
        .enumerate()
        .filter(|(_, f)| dir_of(&f.path) == folder)
        .map(|(i, _)| i as u32)
        .collect()
}

// ---------------------------------------------------------------- Python

fn python(ctx: &Ctx<'_>, from: &IndexedFile, imp: &ImportFact) -> ImportEdge {
    let mut e = edge(&imp.module);
    let text = imp.module.text.as_str();
    let dots = text.chars().take_while(|c| *c == '.').count();
    let rest = &text[dots..];
    let segs = parts(rest, ".");
    let mut bases: Vec<String> = Vec::new();
    if dots > 0 {
        let mut b = dir_of(&from.path).to_owned();
        for _ in 1..dots {
            b = dir_of(&b).to_owned();
        }
        bases.push(b);
    } else {
        for b in ["".to_owned(), "src".to_owned(), dir_of(&from.path).to_owned()] {
            if !bases.contains(&b) && (b.is_empty() || ctx.folders.contains(b.as_str())) {
                bases.push(b);
            }
        }
    }
    for base in bases {
        let mut cur = base.clone();
        let mut out: Vec<SegRef> = Vec::new();
        let mut file = None;
        for (i, (a, b)) in segs.iter().enumerate() {
            let Some(path) = join(&cur, &rest[*a..*b]) else {
                break;
            };
            let last = i + 1 == segs.len();
            let py = format!("{path}.py");
            let init = format!("{path}/__init__.py");
            if let Some(&fi) = ctx.by_path.get(py.as_str()) {
                out.push(SegRef {
                    span: sp(&imp.module.span, dots + a, dots + b),
                    target: SegTarget::File(fi),
                });
                file = Some(fi);
                cur = path;
                break;
            } else if ctx.folders.contains(path.as_str()) || ctx.by_path.contains_key(init.as_str()) {
                out.push(SegRef {
                    span: sp(&imp.module.span, dots + a, dots + b),
                    target: SegTarget::Folder(path.clone()),
                });
                if last {
                    file = ctx.by_path.get(init.as_str()).copied();
                }
                cur = path;
            } else {
                break;
            }
        }
        if out.is_empty() && !(segs.is_empty() && dots > 0) {
            continue;
        }
        e.segs = out;
        if let Some(fi) = file {
            e.files.push(fi);
        } else if segs.is_empty() || !e.segs.is_empty() {
            let init = format!("{cur}/__init__.py");
            if let Some(&fi) = ctx.by_path.get(init.as_str()) {
                e.files.push(fi);
            }
        }
        // `from pkg import submodule`
        let pkg_target = e
            .segs
            .last()
            .is_none_or(|s| matches!(s.target, SegTarget::Folder(_)));
        if !imp.names.is_empty() {
            if !pkg_target {
                return e;
            }
            for n in &imp.names {
                let Some(path) = join(&cur, &n.text) else { continue };
                let py = format!("{path}.py");
                if let Some(&fi) = ctx.by_path.get(py.as_str()) {
                    e.segs.push(SegRef {
                        span: n.span.clone(),
                        target: SegTarget::File(fi),
                    });
                    e.aliases.push((n.text.clone(), SegTarget::File(fi)));
                    e.files.push(fi);
                } else if ctx.folders.contains(path.as_str()) {
                    e.segs.push(SegRef {
                        span: n.span.clone(),
                        target: SegTarget::Folder(path.clone()),
                    });
                    e.aliases.push((n.text.clone(), SegTarget::Folder(path)));
                }
            }
        } else if segs.len() == 1
            && let Some(SegRef { target, .. }) = e.segs.first()
        {
            // `import pricing` binds the module to its own name.
            e.aliases.push((rest.to_owned(), target.clone()));
        }
        return e;
    }
    e
}

// ---------------------------------------------------------------- JavaScript / TypeScript

const JS_EXTS: [&str; 8] = ["ts", "tsx", "js", "jsx", "mts", "cts", "mjs", "cjs"];

enum JsHit {
    File(u32),
    /// A folder; the index file inside it, if any.
    Folder(Option<u32>),
}

fn resolve_js(ctx: &Ctx<'_>, path: &str) -> Option<JsHit> {
    if let Some(&fi) = ctx.by_path.get(path) {
        return Some(JsHit::File(fi));
    }
    let stripped = ["js", "jsx", "mjs", "cjs"]
        .iter()
        .find_map(|e| path.strip_suffix(&format!(".{e}")))
        .unwrap_or(path);
    for ext in JS_EXTS {
        if let Some(&fi) = ctx.by_path.get(format!("{stripped}.{ext}").as_str()) {
            return Some(JsHit::File(fi));
        }
    }
    if ctx.folders.contains(path) {
        let index = JS_EXTS
            .iter()
            .find_map(|e| ctx.by_path.get(format!("{path}/index.{e}").as_str()).copied());
        return Some(JsHit::Folder(index));
    }
    None
}

/// Segments of `rel` (a specifier tail) mapped to files/folders below `start_dir`.
fn js_segs(
    ctx: &Ctx<'_>,
    span: &Span,
    rel_off: usize,
    rel: &str,
    start_dir: &str,
    hit: &JsHit,
    exts: &[&str],
) -> Vec<SegRef> {
    let mut out = Vec::new();
    let mut cur = start_dir.to_owned();
    let segs = parts(rel, "/");
    for (i, (a, b)) in segs.iter().enumerate() {
        let seg = &rel[*a..*b];
        match seg {
            "." => continue,
            ".." => {
                cur = dir_of(&cur).to_owned();
                continue;
            }
            _ => {}
        }
        let Some(path) = join(&cur, seg) else { break };
        let last = i + 1 == segs.len();
        if last && let JsHit::File(fi) = hit {
            // Keep a written extension out of the span so a rename keeps it.
            let stem_end = match seg.rsplit_once('.') {
                Some((stem, ext)) if exts.contains(&ext) => a + stem.len(),
                _ => *b,
            };
            out.push(SegRef {
                span: sp(span, rel_off + a, rel_off + stem_end),
                target: SegTarget::File(*fi),
            });
        } else if ctx.folders.contains(path.as_str()) {
            out.push(SegRef {
                span: sp(span, rel_off + a, rel_off + b),
                target: SegTarget::Folder(path.clone()),
            });
        }
        cur = path;
    }
    out
}

fn js(ctx: &Ctx<'_>, from: &IndexedFile, imp: &ImportFact) -> ImportEdge {
    let mut e = edge(&imp.module);
    let raw = imp.module.text.as_str();
    let quoted = raw.starts_with(['"', '\'', '`']);
    let spec = raw.trim_matches(['"', '\'', '`']);
    let off = usize::from(quoted);
    let finish = |e: &mut ImportEdge, hit: &JsHit| match hit {
        JsHit::File(fi) => e.files.push(*fi),
        JsHit::Folder(Some(fi)) => e.files.push(*fi),
        JsHit::Folder(None) => {}
    };
    if spec.starts_with('.') {
        let base = dir_of(&from.path);
        if let Some(path) = join(base, spec)
            && let Some(hit) = resolve_js(ctx, &path)
        {
            e.segs = js_segs(ctx, &imp.module.span, off, spec, base, &hit, &JS_EXTS);
            finish(&mut e, &hit);
        }
        return e;
    }
    // tsconfig `paths` / `baseUrl`
    let cfg = ctx
        .cfg
        .ts
        .iter()
        .filter(|c| c.dir.is_empty() || from.path.starts_with(&format!("{}/", c.dir)))
        .max_by_key(|c| c.dir.len());
    let Some(cfg) = cfg else { return e };
    let base = join(&cfg.dir, cfg.base_url.as_deref().unwrap_or(".")).unwrap_or_default();
    for (pattern, targets) in &cfg.paths {
        let (matched, star) = match pattern.split_once('*') {
            Some((pre, post))
                if spec.starts_with(pre) && spec.ends_with(post) && spec.len() >= pre.len() + post.len() =>
            {
                (true, Some((pre.len(), spec.len() - post.len())))
            }
            None if pattern == spec => (true, None),
            _ => (false, None),
        };
        if !matched {
            continue;
        }
        for t in targets {
            let tail = star.map(|(a, b)| &spec[a..b]).unwrap_or("");
            let target = t.replace('*', tail);
            let Some(path) = join(&base, &target) else {
                continue;
            };
            let Some(hit) = resolve_js(ctx, &path) else {
                continue;
            };
            if let Some((a, b)) = star {
                let prefix_dir = t.split('*').next().unwrap_or("").trim_end_matches('/');
                let start = join(&base, prefix_dir).unwrap_or_default();
                e.segs = js_segs(
                    ctx,
                    &imp.module.span,
                    off + a,
                    &spec[a..b],
                    &start,
                    &hit,
                    &JS_EXTS,
                );
            }
            finish(&mut e, &hit);
            return e;
        }
    }
    if cfg.base_url.is_some()
        && let Some(path) = join(&base, spec)
        && let Some(hit) = resolve_js(ctx, &path)
    {
        e.segs = js_segs(ctx, &imp.module.span, off, spec, &base, &hit, &JS_EXTS);
        finish(&mut e, &hit);
    }
    e
}

// ---------------------------------------------------------------- Rust

fn module_dir(from: &str) -> String {
    let stem = from.rsplit('/').next().unwrap_or(from).trim_end_matches(".rs");
    if matches!(stem, "mod" | "main" | "lib") {
        dir_of(from).to_owned()
    } else {
        join(dir_of(from), stem).unwrap_or_default()
    }
}

fn crate_root(ctx: &Ctx<'_>, from: &str) -> String {
    let mut cur = dir_of(from).to_owned();
    loop {
        if ctx.cfg.cargo.contains(&cur) {
            return if cur.is_empty() {
                "src".into()
            } else {
                format!("{cur}/src")
            };
        }
        if cur.is_empty() {
            return dir_of(from).to_owned();
        }
        cur = dir_of(&cur).to_owned();
    }
}

/// `mod name` below `dir`: the segment target, the file that defines it, and the module's own directory.
fn mod_target(ctx: &Ctx<'_>, dir: &str, name: &str) -> Option<(SegTarget, u32, String)> {
    let file = join(dir, &format!("{name}.rs"))?;
    if let Some(&fi) = ctx.by_path.get(file.as_str()) {
        return Some((SegTarget::File(fi), fi, join(dir, name)?));
    }
    let m = join(dir, &format!("{name}/mod.rs"))?;
    if let Some(&fi) = ctx.by_path.get(m.as_str()) {
        let d = join(dir, name)?;
        return Some((SegTarget::Folder(d.clone()), fi, d));
    }
    None
}

fn module_file_of_dir(ctx: &Ctx<'_>, dir: &str) -> Option<u32> {
    for n in ["mod.rs", "main.rs", "lib.rs"] {
        if let Some(&fi) = ctx.by_path.get(join(dir, n)?.as_str()) {
            return Some(fi);
        }
    }
    let (parent, stem) = dir.rsplit_once('/').unwrap_or(("", dir));
    ctx.by_path
        .get(join(parent, &format!("{stem}.rs"))?.as_str())
        .copied()
}

fn rust(ctx: &Ctx<'_>, from: &IndexedFile, imp: &ImportFact) -> ImportEdge {
    let mut e = edge(&imp.module);
    let text = imp.module.text.as_str();
    let here = module_dir(&from.path);
    // `mod x;`
    if !text.contains("::") && !text.contains('{') && text.chars().all(|c| c.is_alphanumeric() || c == '_') {
        // Either `mod x;` or `use x;`; both name a child module of this one.
        if let Some((seg, fi, _)) = mod_target(ctx, &here, text) {
            e.segs.push(SegRef {
                span: imp.module.span.clone(),
                target: seg.clone(),
            });
            e.files.push(fi);
            e.aliases.push((text.to_owned(), seg));
        }
        return e;
    }
    let without_alias = text.split(" as ").next().unwrap_or(text);
    let prefix_end = without_alias.find(['{', '*']).unwrap_or(without_alias.len());
    let prefix = &without_alias[..prefix_end];
    let segs = parts(prefix, "::");
    let mut dir = here.clone();
    let mut resolved_any = false;
    let mut idx = 0;
    if let Some((a, b)) = segs.first() {
        match &prefix[*a..*b] {
            "crate" => {
                dir = crate_root(ctx, &from.path);
                idx = 1;
            }
            "self" => idx = 1,
            "super" => {
                dir = dir_of(&here).to_owned();
                idx = 1;
                while segs.get(idx).is_some_and(|(a, b)| &prefix[*a..*b] == "super") {
                    dir = dir_of(&dir).to_owned();
                    idx += 1;
                }
            }
            _ => {}
        }
    }
    let mut last_file = None;
    for (a, b) in segs.iter().skip(idx) {
        let Some((seg, fi, next)) = mod_target(ctx, &dir, &prefix[*a..*b]) else {
            break;
        };
        e.segs.push(SegRef {
            span: sp(&imp.module.span, *a, *b),
            target: seg,
        });
        last_file = Some(fi);
        dir = next;
        resolved_any = true;
    }
    if last_file.is_none() && idx > 0 {
        last_file = module_file_of_dir(ctx, &dir);
    }
    if let Some(fi) = last_file {
        e.files.push(fi);
    }
    // Names inside `{ … }` that are modules themselves.
    if resolved_any || idx > 0 {
        let tail_start = prefix_end;
        let tail = &without_alias[tail_start..];
        let mut pos = 0;
        while pos < tail.len() {
            let rest = &tail[pos..];
            let Some(start) = rest.find(|c: char| c.is_alphabetic() || c == '_') else {
                break;
            };
            let ident_len = rest[start..]
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(rest.len() - start);
            let ident = &rest[start..start + ident_len];
            let abs = tail_start + pos + start;
            pos += start + ident_len;
            if matches!(ident, "self" | "as" | "super" | "crate") {
                continue;
            }
            if let Some((seg, fi, _)) = mod_target(ctx, &dir, ident) {
                e.segs.push(SegRef {
                    span: sp(&imp.module.span, abs, abs + ident_len),
                    target: seg.clone(),
                });
                e.aliases.push((ident.to_owned(), seg));
                e.files.push(fi);
            }
        }
    }
    e
}

// ---------------------------------------------------------------- Go

fn go(ctx: &Ctx<'_>, from: &IndexedFile, imp: &ImportFact) -> ImportEdge {
    let mut e = edge(&imp.module);
    let spec = imp.module.text.trim_matches('"');
    let modules = ctx
        .cfg
        .go
        .iter()
        .filter(|(d, _)| d.is_empty() || from.path.starts_with(&format!("{d}/")))
        .max_by_key(|(d, _)| d.len());
    let Some((mod_dir, module)) = modules else {
        return e;
    };
    let rest = if spec == module {
        ""
    } else if let Some(r) = spec.strip_prefix(&format!("{module}/")) {
        r
    } else {
        return e;
    };
    let base_off = 1 + spec.len() - rest.len();
    let mut cur = mod_dir.clone();
    for (a, b) in parts(rest, "/") {
        let Some(path) = join(&cur, &rest[a..b]) else {
            return e;
        };
        if !ctx.folders.contains(path.as_str()) {
            break;
        }
        e.segs.push(SegRef {
            span: sp(&imp.module.span, base_off + a, base_off + b),
            target: SegTarget::Folder(path.clone()),
        });
        cur = path;
    }
    if rest.is_empty() || !e.segs.is_empty() {
        e.files = files_in_folder(ctx, &cur);
        let pkg = e
            .files
            .first()
            .and_then(|f| ctx.facts[*f as usize].package.clone())
            .unwrap_or_else(|| cur.rsplit('/').next().unwrap_or("").to_owned());
        if !pkg.is_empty() {
            e.aliases.push((pkg, SegTarget::Folder(cur)));
        }
    }
    e
}

// ---------------------------------------------------------------- Java

fn java(ctx: &Ctx<'_>, imp: &ImportFact, exts: &[&str]) -> ImportEdge {
    let mut e = edge(&imp.module);
    let spec = imp.module.text.as_str();
    let segs = parts(spec, ".");
    // Longest prefix that names a file (`a/b/C.java`) or a folder (`a/b`), under any source root.
    for take in (1..=segs.len()).rev() {
        let path: String = segs[..take]
            .iter()
            .map(|(a, b)| &spec[*a..*b])
            .collect::<Vec<_>>()
            .join("/");
        let mut file_suffix = format!("{path}.{}", exts[0]);
        let mut file = None;
        for ext in exts {
            let suffix = format!("{path}.{ext}");
            file = ctx
                .files
                .iter()
                .position(|f| f.path == suffix || f.path.ends_with(&format!("/{suffix}")));
            if file.is_some() {
                file_suffix = suffix;
                break;
            }
        }
        let folder = ctx
            .folders
            .iter()
            .find(|d| **d == path || d.ends_with(&format!("/{path}")))
            .map(|d| (*d).to_owned());
        if file.is_none() && folder.is_none() {
            continue;
        }
        // Folder chain for the leading segments, rooted where the match was found.
        let root = match (&file, &folder) {
            (Some(f), _) => ctx.files[*f]
                .path
                .trim_end_matches(&file_suffix)
                .trim_end_matches('/')
                .to_owned(),
            (None, Some(d)) => d.trim_end_matches(&path).trim_end_matches('/').to_owned(),
            _ => String::new(),
        };
        let mut cur = root;
        for (i, (a, b)) in segs[..take].iter().enumerate() {
            let Some(p) = join(&cur, &spec[*a..*b]) else { break };
            let is_last = i + 1 == take;
            if is_last && let Some(fi) = file {
                // A class import names the file only by convention; the class symbol itself carries the rename.
                e.files.push(fi as u32);
            } else if ctx.folders.contains(p.as_str()) {
                e.segs.push(SegRef {
                    span: sp(&imp.module.span, *a, *b),
                    target: SegTarget::Folder(p.clone()),
                });
                if is_last {
                    e.files = files_in_folder(ctx, &p);
                }
            }
            cur = p;
        }
        return e;
    }
    e
}

// ---------------------------------------------------------------- C / C++, PHP, Ruby, Lua

const C_EXTS: &[&str] = &["h", "hh", "hpp", "hxx", "c", "cc", "cpp", "cxx"];
const PHP_EXTS: &[&str] = &["php", "phtml", "inc"];

/// An include/require of a quoted path: relative to the importing file first, then to each base
/// folder (project root, `include`, `lib`, …). A written extension stays out of the segment spans.
fn path_import(
    ctx: &Ctx<'_>,
    from: &IndexedFile,
    imp: &ImportFact,
    exts: &[&str],
    bases: &[&str],
) -> ImportEdge {
    let mut e = edge(&imp.module);
    let raw = imp.module.text.as_str();
    let quoted = raw.starts_with(['"', '\'']);
    let spec = raw.trim_matches(['"', '\'']);
    if spec.is_empty() {
        return e;
    }
    let off = usize::from(quoted);
    let own_dir = dir_of(&from.path).to_owned();
    let starts = std::iter::once(own_dir.as_str()).chain(bases.iter().copied());
    for start in starts {
        let Some(path) = join(start, spec) else { continue };
        let found = if let Some(&fi) = ctx.by_path.get(path.as_str()) {
            Some(fi)
        } else {
            exts.iter()
                .find_map(|x| ctx.by_path.get(format!("{path}.{x}").as_str()).copied())
        };
        if let Some(fi) = found {
            e.segs = js_segs(ctx, &imp.module.span, off, spec, start, &JsHit::File(fi), exts);
            e.files.push(fi);
            return e;
        }
    }
    e
}

/// `require("a.b")`: dots are folder separators; the last segment is a `.lua` file or an `init.lua` folder.
fn lua(ctx: &Ctx<'_>, imp: &ImportFact) -> ImportEdge {
    let mut e = edge(&imp.module);
    let spec = imp.module.text.as_str();
    let segs = parts(spec, ".");
    for base in ["", "src", "lib"] {
        let mut cur = base.to_owned();
        let mut out: Vec<SegRef> = Vec::new();
        let mut file = None;
        for (i, (a, b)) in segs.iter().enumerate() {
            let Some(path) = join(&cur, &spec[*a..*b]) else {
                break;
            };
            let last = i + 1 == segs.len();
            let lua_file = format!("{path}.lua");
            let init = format!("{path}/init.lua");
            if let Some(&fi) = ctx.by_path.get(lua_file.as_str()) {
                out.push(SegRef {
                    span: sp(&imp.module.span, *a, *b),
                    target: SegTarget::File(fi),
                });
                file = Some(fi);
                break;
            } else if ctx.folders.contains(path.as_str()) {
                out.push(SegRef {
                    span: sp(&imp.module.span, *a, *b),
                    target: SegTarget::Folder(path.clone()),
                });
                if last {
                    file = ctx.by_path.get(init.as_str()).copied();
                }
                cur = path;
            } else {
                break;
            }
        }
        if out.is_empty() {
            continue;
        }
        e.segs = out;
        e.files.extend(file);
        return e;
    }
    e
}
