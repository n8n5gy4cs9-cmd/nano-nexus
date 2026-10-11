//! Rename proposals, deterministic half (ANALYZER §5.6): `<sym>` blocks for the prompt, and the
//! validator that decides whether a proposed name may be used. The model call itself lives in
//! `nexus-agent::pipelines::rename`.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::index::{CodeIndex, SymbolId, same_family};
use super::lang::Lang;
use super::model::SymbolKind;
use super::reserved::{is_builtin, is_keyword};
use super::rules::{Candidate, Case, ProjectStyle, detect_style, file_convention};

/// Result state of a proposal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    /// A valid new name.
    Proposed,
    /// The model says the name is already clear.
    Unchanged,
    /// No valid name was found; the user types one.
    NeedsManual,
}

/// Where a proposal came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProposalSource {
    /// Pure case conversion, no model call.
    Rule,
    Model,
    /// The model failed; a deterministic suggestion was used.
    Fallback,
}

/// One proposed rename.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Proposal {
    pub symbol: SymbolId,
    pub old_name: String,
    pub new_name: Option<String>,
    pub confidence: f32,
    pub reason: String,
    pub status: ProposalStatus,
    pub source: ProposalSource,
    /// Rule ids that made this a candidate.
    pub issues: Vec<String>,
}

/// An example name in a style, for the prompt.
pub fn example_for(case: Case) -> &'static str {
    match case {
        Case::Snake => "calc_total_price",
        Case::Camel => "calcTotalPrice",
        Case::Pascal => "CartItem",
        Case::Screaming => "MAX_ITEM_COUNT",
        Case::Kebab => "cart-item",
        Case::UnderscoreCamel => "_itemCount",
        Case::Flat => "pricing",
    }
}

/// Reads source files for previews, once each.
pub struct Sources<'a> {
    index: &'a CodeIndex,
    cache: std::cell::RefCell<HashMap<String, Option<String>>>,
}

impl<'a> Sources<'a> {
    pub fn new(index: &'a CodeIndex) -> Self {
        Self {
            index,
            cache: std::cell::RefCell::new(HashMap::new()),
        }
    }

    /// The text of a file (code, config or doc), or `None` if unreadable.
    pub fn text(&self, file: u32) -> Option<String> {
        let rel = self.index.path_of(file)?.to_owned();
        let mut c = self.cache.borrow_mut();
        c.entry(rel.clone())
            .or_insert_with(|| {
                std::fs::read_to_string(std::path::Path::new(&self.index.root).join(&rel)).ok()
            })
            .clone()
    }

    /// One line (1-based), trimmed and shortened.
    pub fn line(&self, file: u32, line: u32) -> String {
        self.text(file)
            .and_then(|t| {
                t.lines()
                    .nth(line.saturating_sub(1) as usize)
                    .map(|l| l.trim().chars().take(80).collect())
            })
            .unwrap_or_default()
    }
}

fn kind_attr(c: &Candidate, index: &CodeIndex) -> &'static str {
    index.symbol(c.symbol).map(|s| s.kind.label()).unwrap_or("symbol")
}

/// The `<sym>` block for a candidate, at most about 120 tokens.
pub fn render_sym(index: &CodeIndex, src: &Sources<'_>, c: &Candidate, local_id: &str) -> String {
    let Some(s) = index.symbol(c.symbol) else {
        return String::new();
    };
    let mut out = String::new();
    let lang = s.lang.map(Lang::name).unwrap_or("any");
    let file = match s.kind {
        SymbolKind::Folder => s.signature.clone(),
        _ => index.path_of(s.def.file).unwrap_or("").to_owned(),
    };
    let issues: Vec<&str> = c.issues.iter().map(|i| i.rule.as_str()).collect();
    out.push_str(&format!(
        "<sym id=\"{local_id}\" kind=\"{}\" lang=\"{lang}\" convention=\"{}\" file=\"{file}\" issues=\"{}\">\n",
        kind_attr(c, index),
        c.convention.primary().label(),
        issues.join(",")
    ));
    match s.kind {
        SymbolKind::File => {
            out.push_str(&format!("{}\n", s.signature));
            if let Some(t) = src.text(s.def.file) {
                for l in t.lines().filter(|l| !l.trim().is_empty()).take(4) {
                    out.push_str(&format!("  {}\n", l.trim().chars().take(80).collect::<String>()));
                }
            }
            let types: Vec<&str> = index
                .symbols
                .iter()
                .filter(|x| {
                    x.def.file == s.def.file
                        && x.scope.is_empty()
                        && matches!(
                            x.kind,
                            SymbolKind::Class
                                | SymbolKind::Function
                                | SymbolKind::Interface
                                | SymbolKind::Enum
                        )
                })
                .map(|x| x.name.as_str())
                .take(6)
                .collect();
            if !types.is_empty() {
                out.push_str(&format!("  defines: {}\n", types.join(", ")));
            }
            let users: Vec<&str> = s
                .refs
                .iter()
                .filter_map(|r| index.path_of(r.loc.file))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .take(4)
                .collect();
            if !users.is_empty() {
                out.push_str(&format!("  imported by: {}\n", users.join(", ")));
            }
        }
        SymbolKind::Folder => {
            let inside: Vec<&str> = index
                .files
                .iter()
                .filter(|f| f.path.starts_with(&format!("{}/", s.signature)))
                .map(|f| f.path.rsplit('/').next().unwrap_or(&f.path))
                .take(8)
                .collect();
            out.push_str(&format!("  contains: {}\n", inside.join(", ")));
        }
        _ => {
            out.push_str(&format!("{}\n", s.signature));
            if let Some(d) = &s.doc {
                out.push_str(&format!("  # {}\n", d.chars().take(100).collect::<String>()));
            }
            // A few lines of the body.
            if let (Some(t), Some(def)) = (
                src.text(s.def.file),
                index
                    .facts
                    .get(s.def.file as usize)
                    .and_then(|f| f.defs.iter().find(|d| d.name_span.start == s.def.start)),
            ) {
                let body = t.get(def.node.0 as usize..def.node.1 as usize).unwrap_or("");
                for l in body.lines().skip(1).filter(|l| !l.trim().is_empty()).take(3) {
                    out.push_str(&format!("  {}\n", l.trim().chars().take(80).collect::<String>()));
                }
            }
            let uses: Vec<String> = s
                .refs
                .iter()
                .filter(|r| {
                    !r.is_def
                        && matches!(
                            r.kind,
                            super::index::RefKind::Code | super::index::RefKind::Import
                        )
                })
                .take(3)
                .map(|r| {
                    format!(
                        "{}:{} `{}`",
                        index.path_of(r.loc.file).unwrap_or(""),
                        r.loc.line,
                        src.line(r.loc.file, r.loc.line)
                    )
                })
                .collect();
            if !uses.is_empty() {
                out.push_str(&format!("  usages: {}\n", uses.join(" · ")));
            }
        }
    }
    out.push_str("</sym>");
    // Keep each block under about 120 tokens (3.2 characters each).
    if out.chars().count() > 420 {
        let cut: String = out.chars().take(400).collect();
        return format!("{cut}…\n</sym>");
    }
    out
}

const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "lpt1", "lpt2", "lpt3",
];

/// Checks a proposed name against keywords, builtins, the convention, and collisions.
pub struct Validator<'a> {
    index: &'a CodeIndex,
    style: ProjectStyle,
    /// Names declared at the top of each file (a new local must not shadow them).
    top_names: Vec<HashSet<&'a str>>,
}

impl<'a> Validator<'a> {
    pub fn new(index: &'a CodeIndex) -> Self {
        let mut top_names: Vec<HashSet<&str>> = vec![HashSet::new(); index.files.len()];
        for s in &index.symbols {
            if s.scope.is_empty()
                && !matches!(
                    s.kind,
                    SymbolKind::File | SymbolKind::Folder | SymbolKind::Parameter
                )
                && (s.kind != SymbolKind::Variable || s.flags.has(super::index::Flags::EXPORTED))
                && let Some(set) = top_names.get_mut(s.def.file as usize)
            {
                set.insert(s.name.as_str());
            }
        }
        Self {
            index,
            style: detect_style(index),
            top_names,
        }
    }

    fn effective<'b>(&'b self, id: SymbolId, taken: &'b HashMap<SymbolId, String>) -> &'b str {
        taken
            .get(&id)
            .map(String::as_str)
            .or_else(|| self.index.symbol(id).map(|s| s.name.as_str()))
            .unwrap_or("")
    }

    /// `Ok` if `new_name` can replace the symbol's name, given the other renames already accepted.
    pub fn check(
        &self,
        id: SymbolId,
        new_name: &str,
        taken: &HashMap<SymbolId, String>,
    ) -> Result<(), String> {
        let Some(s) = self.index.symbol(id) else {
            return Err("unknown symbol".into());
        };
        if new_name.is_empty() || new_name.len() > 48 {
            return Err("the name must be 1 to 48 characters".into());
        }
        if new_name == s.name {
            return Err("the new name is the same as the old name".into());
        }
        if matches!(s.kind, SymbolKind::File | SymbolKind::Folder) {
            return self.check_path(id, new_name, taken);
        }
        let Some(lang) = s.lang else {
            return Err("unknown language".into());
        };
        if is_keyword(lang, new_name) {
            return Err(format!("`{new_name}` is a keyword in {}", lang.name()));
        }
        if is_builtin(lang, new_name) {
            return Err(format!(
                "`{new_name}` is a builtin or standard name in {}",
                lang.name()
            ));
        }
        // Convention.
        let jsx = self
            .index
            .files
            .get(s.def.file as usize)
            .is_some_and(|f| f.path.ends_with(".tsx") || f.path.ends_with(".jsx"));
        let conv = super::rules::convention(&super::rules::NameCtx {
            lang,
            kind: s.kind,
            name: &s.name,
            exported: s.flags.has(super::index::Flags::EXPORTED),
            visibility: s.visibility.as_deref(),
            signature: &s.signature,
            jsx_file: jsx,
        });
        let go_ok = lang == Lang::Go
            && !new_name.contains('_')
            && new_name.chars().all(|c| c.is_ascii_alphanumeric());
        let iface_ok = lang != Lang::CSharp
            || s.kind != SymbolKind::Interface
            || (new_name.starts_with('I') && new_name.chars().nth(1).is_some_and(|c| c.is_ascii_uppercase()));
        if !(conv.accepts(new_name) || go_ok) || !iface_ok {
            return Err(format!("`{new_name}` must be {}", conv.primary().label()));
        }
        // Collisions in the same scope.
        let local = matches!(
            s.kind,
            SymbolKind::Variable | SymbolKind::Parameter | SymbolKind::Constant
        ) && self.is_local(s);
        let my_vis = self.visible_of(s);
        for p in &self.index.symbols {
            if p.id == id || matches!(p.kind, SymbolKind::File | SymbolKind::Folder) {
                continue;
            }
            if !p.lang.is_some_and(|l| same_family(l, lang)) || self.effective(p.id, taken) != new_name {
                continue;
            }
            let same_file = p.def.file == s.def.file;
            let peer = if local {
                // Same function, overlapping visibility.
                same_file && p.scope == s.scope && self.visible_of(p) == my_vis
                    || (same_file
                        && self.is_local(p)
                        && my_vis.is_some()
                        && self
                            .visible_of(p)
                            .is_some_and(|v| ranges_overlap(v, my_vis.unwrap_or((0, 0)))))
            } else if s.scope.is_empty() {
                // Top level: same file, or the same package/namespace.
                let pf = &self.index.files[p.def.file as usize];
                let sf = &self.index.files[s.def.file as usize];
                p.scope.is_empty()
                    && !self.is_local(p)
                    && (same_file
                        || match lang {
                            Lang::Go => dir_of(&pf.path) == dir_of(&sf.path),
                            Lang::Java => {
                                self.index.facts[p.def.file as usize].package
                                    == self.index.facts[s.def.file as usize].package
                            }
                            Lang::CSharp => true,
                            _ => false,
                        })
            } else {
                // Member: same class.
                same_file && p.scope == s.scope && !self.is_local(p)
            };
            if peer {
                return Err(format!("`{new_name}` already exists in this scope"));
            }
        }
        if local {
            // A local must not hide something the function can see.
            if self
                .top_names
                .get(s.def.file as usize)
                .is_some_and(|set| set.contains(new_name))
            {
                return Err(format!("`{new_name}` would hide a name declared in this file"));
            }
            if self.index.facts[s.def.file as usize]
                .import_names
                .iter()
                .any(|n| n.text == new_name)
            {
                return Err(format!("`{new_name}` would hide an imported name"));
            }
        }
        Ok(())
    }

    fn is_local(&self, s: &super::index::Symbol) -> bool {
        self.visible_of(s).is_some()
    }

    fn visible_of(&self, s: &super::index::Symbol) -> Option<(u32, u32)> {
        self.index
            .facts
            .get(s.def.file as usize)
            .and_then(|f| {
                f.defs
                    .iter()
                    .find(|d| d.name_span.start == s.def.start && d.name == s.name)
            })
            .and_then(|d| d.visible)
    }

    fn check_path(
        &self,
        id: SymbolId,
        new_name: &str,
        taken: &HashMap<SymbolId, String>,
    ) -> Result<(), String> {
        let Some(s) = self.index.symbol(id) else {
            return Err("unknown symbol".into());
        };
        let folder = s.kind == SymbolKind::Folder;
        if new_name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|', '.']) && folder
            || new_name.contains(['/', '\\'])
        {
            return Err("a file or folder name cannot contain path characters".into());
        }
        if WINDOWS_RESERVED.contains(&new_name.to_ascii_lowercase().as_str()) {
            return Err(format!("`{new_name}` is a reserved name on Windows"));
        }
        let lang = s.lang.or_else(|| {
            self.index
                .files
                .iter()
                .find(|f| f.path.starts_with(&format!("{}/", s.signature)))
                .map(|f| f.lang)
        });
        if let Some(lang) = lang {
            let jsx = s.signature.ends_with(".tsx") || s.signature.ends_with(".jsx");
            let conv = file_convention(lang, folder, &self.style, jsx);
            if !conv.accepts(new_name) {
                return Err(format!("`{new_name}` must be {}", conv.primary().label()));
            }
        }
        // Siblings in the same directory, compared case-insensitively. A case-only change of itself is fine.
        let parent = s.scope.as_str();
        let ext = if folder {
            ""
        } else {
            s.signature.rsplit_once('.').map(|(_, e)| e).unwrap_or("")
        };
        let full = |name: &str| {
            if ext.is_empty() {
                name.to_ascii_lowercase()
            } else {
                format!("{name}.{ext}").to_ascii_lowercase()
            }
        };
        let mine = full(new_name);
        for p in &self.index.symbols {
            if p.id == id || !matches!(p.kind, SymbolKind::File | SymbolKind::Folder) || p.scope != parent {
                continue;
            }
            let p_ext = if p.kind == SymbolKind::Folder {
                ""
            } else {
                p.signature.rsplit_once('.').map(|(_, e)| e).unwrap_or("")
            };
            let name = self.effective(p.id, taken);
            let theirs = if p_ext.is_empty() {
                name.to_ascii_lowercase()
            } else {
                format!("{name}.{p_ext}").to_ascii_lowercase()
            };
            if theirs == mine {
                return Err(format!("`{new_name}` already exists in this folder"));
            }
        }
        Ok(())
    }
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

fn ranges_overlap(a: (u32, u32), b: (u32, u32)) -> bool {
    a.0 < b.1 && b.0 < a.1
}
