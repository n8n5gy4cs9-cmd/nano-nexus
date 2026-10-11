//! Per-file reports (ANALYZER §2): deterministic facts for code, text, PDF, DOCX, CSV/TSV, JSON, logs,
//! images, and ZIP archives. The LLM step (if any) is named in [`LlmPlan`] and runs in a pipeline.

use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;
use std::time::UNIX_EPOCH;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::extract::{self, ExtractError};
use crate::fmt::{bytes as fmt_bytes, count, date_time};
use crate::{logs, profile};

#[derive(Debug, thiserror::Error)]
pub enum FileReportError {
    #[error("could not read the file: {0}")]
    Io(#[from] std::io::Error),
    #[error("this is a folder, not a file")]
    IsFolder,
    #[error("{0}")]
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Fact {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Table {
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Flag {
    /// `info | warning`
    pub level: String,
    pub text: String,
}

/// Which model step follows, and the text it works from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct LlmPlan {
    /// Internal prompt or pipeline: `file_report_code | doc_summarize | data_profile | log_triage`.
    pub prompt: String,
    /// The `<facts>` block.
    pub facts_text: String,
    /// Extra text for the prompt (the top of a source file).
    pub excerpt: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct FileReport {
    pub path: String,
    pub name: String,
    /// `code | text | pdf | docx | csv | json | log | image | zip | other`
    pub kind: String,
    pub bytes: u64,
    pub modified: Option<i64>,
    pub facts: Vec<Fact>,
    pub tables: Vec<Table>,
    pub flags: Vec<Flag>,
    pub llm: Option<LlmPlan>,
}

fn fact(label: &str, value: impl Into<String>) -> Fact {
    Fact {
        label: label.into(),
        value: value.into(),
    }
}

// ---- code -------------------------------------------------------------------------------------

struct Lang {
    name: &'static str,
    exts: &'static [&'static str],
    line_comments: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    func: &'static str,
    ty: &'static str,
    import: &'static str,
    indent_based: bool,
}

const LANGS: &[Lang] = &[
    Lang {
        name: "Rust",
        exts: &["rs"],
        line_comments: &["//"],
        block: Some(("/*", "*/")),
        func: r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?(?:const\s+)?fn\s+(\w+)",
        ty: r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|trait|union|type)\s+(\w+)",
        import: r"^\s*(?:pub\s+)?use\s+([\w:{}, *]+);",
        indent_based: false,
    },
    Lang {
        name: "Python",
        exts: &["py", "pyw"],
        line_comments: &["#"],
        block: None,
        func: r"^\s*(?:async\s+)?def\s+(\w+)",
        ty: r"^\s*class\s+(\w+)",
        import: r"^\s*(?:from\s+([\w.]+)\s+import|import\s+([\w., ]+))",
        indent_based: true,
    },
    Lang {
        name: "JavaScript",
        exts: &["js", "mjs", "cjs", "jsx"],
        line_comments: &["//"],
        block: Some(("/*", "*/")),
        func: r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?(?:function\*?\s+(\w+)|(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s*)?(?:\([^)]*\)|\w+)\s*=>)",
        ty: r"^\s*(?:export\s+)?(?:default\s+)?class\s+(\w+)",
        import: r#"^\s*(?:import\s.*?from\s+['"]([^'"]+)['"]|(?:const|let|var)\s+.*?require\(['"]([^'"]+)['"]\))"#,
        indent_based: false,
    },
    Lang {
        name: "TypeScript",
        exts: &["ts", "tsx"],
        line_comments: &["//"],
        block: Some(("/*", "*/")),
        func: r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?(?:function\*?\s+(\w+)|(?:const|let|var)\s+(\w+)\s*(?::[^=]+)?=\s*(?:async\s*)?(?:\([^)]*\)|\w+)\s*(?::[^=]+)?=>)",
        ty: r"^\s*(?:export\s+)?(?:default\s+)?(?:abstract\s+)?(?:class|interface|enum|type)\s+(\w+)",
        import: r#"^\s*import\s.*?from\s+['"]([^'"]+)['"]"#,
        indent_based: false,
    },
    Lang {
        name: "Go",
        exts: &["go"],
        line_comments: &["//"],
        block: Some(("/*", "*/")),
        func: r"^\s*func\s+(?:\([^)]*\)\s*)?(\w+)",
        ty: r"^\s*type\s+(\w+)\s+(?:struct|interface)",
        import: r#"^\s*(?:import\s+)?"([\w./-]+)"\s*$"#,
        indent_based: false,
    },
    Lang {
        name: "Java",
        exts: &["java"],
        line_comments: &["//"],
        block: Some(("/*", "*/")),
        func: r"^\s*(?:public|private|protected)?\s*(?:static\s+)?(?:final\s+)?(?:synchronized\s+)?[\w<>\[\], ?]+\s+(\w+)\s*\([^;]*\)\s*(?:throws [\w., ]+)?\s*\{?\s*$",
        ty: r"^\s*(?:public\s+)?(?:abstract\s+)?(?:final\s+)?(?:class|interface|enum|record)\s+(\w+)",
        import: r"^\s*import\s+(?:static\s+)?([\w.*]+);",
        indent_based: false,
    },
    Lang {
        name: "C#",
        exts: &["cs"],
        line_comments: &["//"],
        block: Some(("/*", "*/")),
        func: r"^\s*(?:public|private|protected|internal)\s+(?:static\s+)?(?:async\s+)?(?:override\s+)?(?:virtual\s+)?[\w<>\[\], ?]+\s+(\w+)\s*\([^;]*\)\s*\{?\s*$",
        ty: r"^\s*(?:public\s+|internal\s+)?(?:static\s+|abstract\s+|sealed\s+|partial\s+)*(?:class|struct|interface|enum|record)\s+(\w+)",
        import: r"^\s*using\s+([\w.]+);",
        indent_based: false,
    },
    Lang {
        name: "C/C++",
        exts: &["c", "h", "cc", "cpp", "cxx", "hpp", "hh"],
        line_comments: &["//"],
        block: Some(("/*", "*/")),
        func: r"^[\w:<>*&~ ]+\s+[*&]*(\w+)\s*\([^;]*\)\s*(?:const)?\s*\{?\s*$",
        ty: r"^\s*(?:typedef\s+)?(?:struct|class|enum|union)\s+(\w+)",
        import: r#"^\s*#\s*include\s+[<"]([^>"]+)[>"]"#,
        indent_based: false,
    },
    Lang {
        name: "Ruby",
        exts: &["rb"],
        line_comments: &["#"],
        block: None,
        func: r"^\s*def\s+(?:self\.)?(\w+[?!]?)",
        ty: r"^\s*(?:class|module)\s+([\w:]+)",
        import: r#"^\s*require(?:_relative)?\s+['"]([^'"]+)['"]"#,
        indent_based: true,
    },
    Lang {
        name: "PHP",
        exts: &["php"],
        line_comments: &["//", "#"],
        block: Some(("/*", "*/")),
        func: r"^\s*(?:public|private|protected)?\s*(?:static\s+)?function\s+(\w+)",
        ty: r"^\s*(?:abstract\s+|final\s+)?(?:class|interface|trait)\s+(\w+)",
        import: r#"^\s*(?:use|require(?:_once)?|include(?:_once)?)\s+([\w\\/.'"]+)"#,
        indent_based: false,
    },
    Lang {
        name: "Lua",
        exts: &["lua"],
        line_comments: &["--"],
        block: Some(("--[[", "]]")),
        func: r"^\s*(?:local\s+)?function\s+([\w.:]+)",
        ty: r"^$never$",
        import: r#"require\s*\(?\s*['"]([^'"]+)['"]"#,
        indent_based: true,
    },
    Lang {
        name: "Shell",
        exts: &["sh", "bash", "zsh"],
        line_comments: &["#"],
        block: None,
        func: r"^\s*(?:function\s+)?(\w+)\s*\(\)\s*\{?",
        ty: r"^$never$",
        import: r"^\s*(?:source|\.)\s+(\S+)",
        indent_based: false,
    },
    Lang {
        name: "Swift",
        exts: &["swift"],
        line_comments: &["//"],
        block: Some(("/*", "*/")),
        func: r"^\s*(?:public\s+|private\s+|internal\s+|open\s+)?(?:static\s+)?func\s+(\w+)",
        ty: r"^\s*(?:public\s+|private\s+|final\s+)*(?:class|struct|enum|protocol|actor)\s+(\w+)",
        import: r"^\s*import\s+(\w+)",
        indent_based: false,
    },
    Lang {
        name: "Kotlin",
        exts: &["kt", "kts"],
        line_comments: &["//"],
        block: Some(("/*", "*/")),
        func: r"^\s*(?:public\s+|private\s+|internal\s+|override\s+|suspend\s+)*fun\s+(?:<[^>]+>\s*)?(?:[\w.]+\.)?(\w+)",
        ty: r"^\s*(?:data\s+|sealed\s+|abstract\s+|open\s+)*(?:class|interface|object|enum class)\s+(\w+)",
        import: r"^\s*import\s+([\w.*]+)",
        indent_based: false,
    },
];

fn lang_for(ext: &str) -> Option<&'static Lang> {
    LANGS.iter().find(|l| l.exts.contains(&ext))
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|_| Regex::new("$^").unwrap_or_else(|_| unreachable!()))
}

fn todo_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| re(r"\b(TODO|FIXME|HACK|XXX)\b[:\s]*(.*)"))
}

fn branch_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| re(r"\b(if|for|while|match|switch|case|elif|except|catch|when|unless)\b|&&|\|\|"))
}

#[derive(Clone, Debug)]
struct Func {
    name: String,
    line: usize,
    len: usize,
    branches: usize,
}

fn indent(l: &str) -> usize {
    l.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

/// Last line of the function starting at `start`, or `None` for a declaration without a body.
fn func_end(lines: &[&str], start: usize, lang: &Lang) -> Option<usize> {
    if lang.indent_based {
        let base = indent(lines[start]);
        let mut end = start;
        for (i, l) in lines.iter().enumerate().skip(start + 1) {
            if l.trim().is_empty() {
                continue;
            }
            if indent(l) <= base {
                // Ruby/Lua close with `end` at the same indent.
                if (lang.name == "Ruby" || lang.name == "Lua") && l.trim_start().starts_with("end") {
                    end = i;
                }
                break;
            }
            end = i;
        }
        return Some(end);
    }
    let mut depth = 0i32;
    let mut opened = false;
    for (i, l) in lines.iter().enumerate().skip(start) {
        for c in l.chars() {
            match c {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth <= 0 {
            return Some(i);
        }
        if !opened && (l.trim_end().ends_with(';') || i > start + 3) {
            return None; // a declaration without a body (trait method, prototype)
        }
    }
    Some(lines.len().saturating_sub(1))
}

/// Facts about source code from line patterns. (A tree-sitter version replaces the function finder in M09.)
fn code_report(text: &str, lang: &Lang, r: &mut FileReport) {
    let lines: Vec<&str> = text.lines().collect();
    let (mut blank, mut comment, mut in_block) = (0usize, 0usize, false);
    let mut todos: Vec<Vec<String>> = Vec::new();
    let mut imports: Vec<String> = Vec::new();
    let func_re = re(lang.func);
    let ty_re = re(lang.ty);
    let imp_re = re(lang.import);
    let mut funcs: Vec<Func> = Vec::new();
    let mut types = 0usize;
    for (i, raw) in lines.iter().enumerate() {
        let t = raw.trim();
        if t.is_empty() {
            blank += 1;
            continue;
        }
        let mut is_comment = false;
        if let Some((open, close)) = lang.block {
            if in_block {
                is_comment = true;
                if t.contains(close) {
                    in_block = false;
                }
            } else if let Some(rest) = t.strip_prefix(open) {
                is_comment = true;
                in_block = !rest.contains(close);
            }
        }
        if !is_comment && lang.line_comments.iter().any(|p| t.starts_with(p)) {
            is_comment = true;
        }
        if is_comment {
            comment += 1;
        }
        if let Some(c) = todo_re().captures(raw)
            && todos.len() < 15
            && (is_comment || lang.line_comments.iter().any(|p| raw.contains(p)))
        {
            todos.push(vec![
                (i + 1).to_string(),
                c[1].to_owned(),
                c[2].trim().chars().take(80).collect(),
            ]);
        }
        if is_comment {
            continue;
        }
        if let Some(c) = imp_re.captures(raw) {
            let name = c
                .iter()
                .skip(1)
                .flatten()
                .next()
                .map_or("", |m| m.as_str())
                .trim()
                .to_owned();
            if !name.is_empty() && imports.len() < 200 && !imports.contains(&name) {
                imports.push(name);
            }
        }
        if ty_re.is_match(raw) {
            types += 1;
        }
        if let Some(c) = func_re.captures(raw) {
            let name = c
                .iter()
                .skip(1)
                .flatten()
                .next()
                .map_or("?", |m| m.as_str())
                .to_owned();
            if matches!(
                name.as_str(),
                "if" | "for" | "while" | "switch" | "catch" | "return" | "else"
            ) {
                continue;
            }
            if let Some(end) = func_end(&lines, i, lang) {
                let branches = lines[i..=end.min(lines.len() - 1)]
                    .iter()
                    .map(|l| branch_re().find_iter(l).count())
                    .sum::<usize>();
                funcs.push(Func {
                    name,
                    line: i + 1,
                    len: end - i + 1,
                    branches,
                });
            }
        }
    }
    let total = lines.len();
    let code = total.saturating_sub(blank + comment);
    r.facts.push(fact("Language", lang.name));
    r.facts.push(fact(
        "Lines",
        format!(
            "{} total · {} code · {} comments · {} blank",
            count(total as u64),
            count(code as u64),
            count(comment as u64),
            count(blank as u64)
        ),
    ));
    r.facts.push(fact("Functions", count(funcs.len() as u64)));
    r.facts.push(fact("Types", count(types as u64)));
    r.facts.push(fact("Imports", count(imports.len() as u64)));
    let mut longest = funcs.clone();
    longest.sort_by(|a, b| b.len.cmp(&a.len).then(a.line.cmp(&b.line)));
    if !longest.is_empty() {
        r.tables.push(Table {
            title: "Longest functions".into(),
            columns: [
                "Function".into(),
                "Line".into(),
                "Lines".into(),
                "Branches".into(),
            ]
            .into(),
            rows: longest
                .iter()
                .take(5)
                .map(|f| {
                    vec![
                        f.name.clone(),
                        f.line.to_string(),
                        f.len.to_string(),
                        f.branches.to_string(),
                    ]
                })
                .collect(),
        });
    }
    if !todos.is_empty() {
        r.tables.push(Table {
            title: "TODO / FIXME".into(),
            columns: ["Line".into(), "Tag".into(), "Note".into()].into(),
            rows: todos.clone(),
        });
    }
    if !imports.is_empty() {
        r.tables.push(Table {
            title: "Imports".into(),
            columns: vec!["Module".into()],
            rows: imports.iter().take(30).map(|i| vec![i.clone()]).collect(),
        });
    }
    let mut facts = format!(
        "<facts>\nfile: {}   language: {}   lines: {total} ({code} code, {comment} comments, {blank} blank)\nfunctions: {}   types: {types}   imports: {}\n",
        r.name,
        lang.name,
        funcs.len(),
        imports.len()
    );
    for f in longest.iter().take(5) {
        facts.push_str(&format!(
            "long function: {} at line {} ({} lines, {} branches)\n",
            f.name, f.line, f.len, f.branches
        ));
    }
    for t in todos.iter().take(5) {
        facts.push_str(&format!("{} at line {}: {}\n", t[1], t[0], t[2]));
    }
    if !imports.is_empty() {
        facts.push_str(&format!(
            "imports: {}\n",
            imports.iter().take(15).cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    facts.push_str("</facts>");
    let excerpt = lines.iter().take(120).copied().collect::<Vec<_>>().join("\n");
    r.llm = Some(LlmPlan {
        prompt: "file_report_code".into(),
        facts_text: facts,
        excerpt: Some(excerpt),
    });
}

// ---- text / documents -------------------------------------------------------------------------

fn outline(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .filter(|l| l.starts_with('#'))
        .take(40)
        .map(|l| {
            let level = l.chars().take_while(|c| *c == '#').count();
            vec![level.to_string(), l.trim_start_matches('#').trim().to_owned()]
        })
        .collect()
}

fn text_facts(r: &mut FileReport, e: &extract::Extracted, kind_label: &str) {
    let full = e.full_text();
    let words = e.word_count();
    r.facts.push(fact("Words", count(words as u64)));
    r.facts
        .push(fact("Reading time", format!("{} min", (words / 200).max(1))));
    if e.pages.len() > 1 {
        r.facts.push(fact("Pages", count(e.pages.len() as u64)));
    }
    if let Some(l) = whatlang::detect(&full.chars().take(2000).collect::<String>())
        .filter(|i| i.is_reliable() || i.confidence() > 0.5)
    {
        r.facts.push(fact("Language", l.lang().eng_name()));
    }
    if let Some(t) = &e.title {
        r.facts.push(fact("Title", t.clone()));
    }
    let o = outline(&full);
    if !o.is_empty() {
        r.tables.push(Table {
            title: "Outline".into(),
            columns: vec!["Level".into(), "Heading".into()],
            rows: o,
        });
    }
    if !e.scanned_pages.is_empty() {
        r.flags.push(Flag {
            level: "warning".into(),
            text: format!(
                "{} page(s) have no text layer (scanned images): {}",
                e.scanned_pages.len(),
                e.scanned_pages
                    .iter()
                    .take(10)
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    let facts = format!(
        "<facts>\nfile: {}   type: {kind_label}   pages: {}   words: {words}\n</facts>",
        r.name,
        e.pages.len()
    );
    r.llm = Some(LlmPlan {
        prompt: "doc_summarize".into(),
        facts_text: facts,
        excerpt: None,
    });
}

// ---- images -----------------------------------------------------------------------------------

/// `(format, width, height)` from the file header.
pub fn image_dimensions(b: &[u8]) -> Option<(&'static str, u32, u32)> {
    if b.len() > 24 && b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((
            "PNG",
            u32::from_be_bytes(b[16..20].try_into().ok()?),
            u32::from_be_bytes(b[20..24].try_into().ok()?),
        ));
    }
    if b.len() > 10 && (b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a")) {
        return Some((
            "GIF",
            u32::from(u16::from_le_bytes([b[6], b[7]])),
            u32::from(u16::from_le_bytes([b[8], b[9]])),
        ));
    }
    if b.len() > 26 && b.starts_with(b"BM") {
        return Some((
            "BMP",
            u32::from_le_bytes(b[18..22].try_into().ok()?),
            i32::from_le_bytes(b[22..26].try_into().ok()?).unsigned_abs(),
        ));
    }
    if b.len() > 4 && b.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = b[i + 1];
            if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                return Some((
                    "JPEG",
                    u32::from(u16::from_be_bytes([b[i + 7], b[i + 8]])),
                    u32::from(u16::from_be_bytes([b[i + 5], b[i + 6]])),
                ));
            }
            if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
                i += 2;
            } else {
                i += 2 + usize::from(u16::from_be_bytes([b[i + 2], b[i + 3]]));
            }
        }
        return Some(("JPEG", 0, 0));
    }
    None
}

fn image_report(path: &Path, r: &mut FileReport) {
    let mut head = Vec::new();
    if let Ok(f) = std::fs::File::open(path) {
        let _ = f.take(256 * 1024).read_to_end(&mut head);
    }
    match image_dimensions(&head) {
        Some((fmt, w, h)) if w > 0 => {
            r.facts.push(fact("Format", fmt));
            r.facts.push(fact("Dimensions", format!("{w} × {h} px")));
            r.facts.push(fact(
                "Megapixels",
                format!("{:.1}", f64::from(w) * f64::from(h) / 1e6),
            ));
        }
        Some((fmt, ..)) => r.facts.push(fact("Format", fmt)),
        None => r.facts.push(fact(
            "Format",
            infer::get(&head).map_or("unknown".into(), |t| t.extension().to_uppercase()),
        )),
    }
    if let Ok(file) = std::fs::File::open(path)
        && let Ok(exif) = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(file))
    {
        let get = |tag: exif::Tag| {
            exif.get_field(tag, exif::In::PRIMARY)
                .map(|f| f.display_value().to_string().trim_matches('"').to_owned())
        };
        if let Some(d) = get(exif::Tag::DateTimeOriginal).or_else(|| get(exif::Tag::DateTime)) {
            r.facts.push(fact("Taken", d));
        }
        let camera = [get(exif::Tag::Make), get(exif::Tag::Model)]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        if !camera.is_empty() {
            r.facts.push(fact("Camera", camera));
        }
        if exif.fields().any(|f| {
            matches!(
                f.tag,
                exif::Tag::GPSLatitude | exif::Tag::GPSLongitude | exif::Tag::GPSLatitudeRef
            )
        }) {
            r.flags.push(Flag {
                level: "warning".into(),
                text: "This photo contains GPS location data. Share it with care.".into(),
            });
        }
    }
}

// ---- zip --------------------------------------------------------------------------------------

fn zip_report(path: &Path, r: &mut FileReport) -> Result<(), FileReportError> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(path)?)
        .map_err(|e| FileReportError::Other(e.to_string()))?;
    let (mut files, mut dirs, mut total, mut packed) = (0u64, 0u64, 0u64, 0u64);
    let mut biggest: Vec<(u64, String)> = Vec::new();
    for i in 0..z.len().min(100_000) {
        let Ok(f) = z.by_index_raw(i) else { continue };
        if f.is_dir() {
            dirs += 1;
        } else {
            files += 1;
            total += f.size();
            packed += f.compressed_size();
            biggest.push((f.size(), f.name().to_owned()));
        }
    }
    biggest.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    r.facts.push(fact("Files", count(files)));
    r.facts.push(fact("Folders", count(dirs)));
    r.facts.push(fact("Uncompressed", fmt_bytes(total)));
    r.facts.push(fact("Compressed", fmt_bytes(packed)));
    r.tables.push(Table {
        title: "Largest entries".into(),
        columns: vec!["Entry".into(), "Size".into()],
        rows: biggest
            .iter()
            .take(10)
            .map(|(s, n)| vec![n.clone(), fmt_bytes(*s)])
            .collect(),
    });
    if packed > 0 && total / packed.max(1) > 100 && total > 1 << 30 {
        r.flags.push(Flag {
            level: "warning".into(),
            text: "Extremely high compression ratio: be careful when extracting.".into(),
        });
    }
    if biggest.iter().any(|(_, n)| n.contains("..")) {
        r.flags.push(Flag {
            level: "warning".into(),
            text: "Some entries have `..` in their path.".into(),
        });
    }
    Ok(())
}

// ---- dispatch ---------------------------------------------------------------------------------

fn looks_like_log(text: &str) -> bool {
    let sample: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(50).collect();
    !sample.is_empty()
        && sample
            .iter()
            .filter(|l| logs::parse_ts(l).is_some() || logs::detect_level(l) != "OTHER")
            .count()
            * 2
            > sample.len()
}

/// Build the report for one file.
pub fn file_report(path: &Path) -> Result<FileReport, FileReportError> {
    let meta = std::fs::metadata(path)?;
    if meta.is_dir() {
        return Err(FileReportError::IsFolder);
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let mut r = FileReport {
        path: path.display().to_string(),
        name,
        kind: "other".into(),
        bytes: meta.len(),
        modified: meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64),
        facts: vec![],
        tables: vec![],
        flags: vec![],
        llm: None,
    };
    r.facts.push(fact("Size", fmt_bytes(r.bytes)));
    if let Some(m) = r.modified {
        r.facts.push(fact("Modified", date_time(m)));
    }
    let io_err = |e: ExtractError| FileReportError::Other(e.to_string());
    match ext.as_str() {
        "pdf" => {
            r.kind = "pdf".into();
            text_facts(&mut r, &extract::extract_pdf(path).map_err(io_err)?, "PDF");
        }
        "docx" => {
            r.kind = "docx".into();
            text_facts(
                &mut r,
                &extract::extract_docx(path).map_err(io_err)?,
                "Word document",
            );
        }
        "csv" | "tsv" => {
            r.kind = "csv".into();
            let p = profile::profile_csv(path).map_err(|e| FileReportError::Other(e.to_string()))?;
            r.facts.push(fact("Rows", count(p.rows)));
            r.facts.push(fact("Columns", count(p.columns.len() as u64)));
            r.tables.push(Table {
                title: "Columns".into(),
                columns: ["Name", "Type", "Empty", "Distinct", "Range", "Top value"]
                    .map(String::from)
                    .into(),
                rows: p
                    .columns
                    .iter()
                    .map(|c| {
                        vec![
                            c.name.clone(),
                            c.kind.clone(),
                            format!("{:.0}%", c.null_pct),
                            format!("{}{}", count(c.unique), if c.unique_capped { "+" } else { "" }),
                            c.min
                                .clone()
                                .zip(c.max.clone())
                                .map_or(String::new(), |(a, b)| format!("{a} → {b}")),
                            c.top_values
                                .first()
                                .map_or(String::new(), |t| format!("{} ×{}", t.value, count(t.count))),
                        ]
                    })
                    .collect(),
            });
            r.tables.push(Table {
                title: "Sample rows".into(),
                columns: p.columns.iter().map(|c| c.name.clone()).collect(),
                rows: p.sample_rows.clone(),
            });
            r.llm = Some(LlmPlan {
                prompt: "data_profile".into(),
                facts_text: profile::csv_facts(&p, &r.name),
                excerpt: None,
            });
        }
        "json" | "jsonl" | "ndjson" => {
            r.kind = "json".into();
            let p = profile::profile_json(path).map_err(|e| FileReportError::Other(e.to_string()))?;
            r.facts
                .push(fact("Shape", format!("{} · {} item(s)", p.shape, count(p.items))));
            r.facts.push(fact("Depth", p.depth.to_string()));
            r.facts.push(fact("Values", count(p.total_values)));
            r.tables.push(Table {
                title: "Keys".into(),
                columns: ["Key", "Type", "In items"].map(String::from).into(),
                rows: p
                    .top_keys
                    .iter()
                    .map(|k| vec![k.key.clone(), k.kind.clone(), count(k.present)])
                    .collect(),
            });
            r.llm = Some(LlmPlan {
                prompt: "data_profile".into(),
                facts_text: profile::json_facts(&p, &r.name),
                excerpt: None,
            });
        }
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "tif" | "tiff" | "heic" => {
            r.kind = "image".into();
            image_report(path, &mut r);
        }
        "zip" | "jar" | "whl" | "apk" => {
            r.kind = "zip".into();
            zip_report(path, &mut r)?;
        }
        _ => {
            if let Some(lang) = lang_for(&ext) {
                let ex = extract::extract_plain(path).map_err(io_err)?;
                r.kind = "code".into();
                code_report(&ex.full_text(), lang, &mut r);
            } else {
                let ex = extract::extract_plain(path).map_err(io_err)?;
                let full = ex.full_text();
                if ext == "log" || looks_like_log(&full) {
                    r.kind = "log".into();
                    let lr = logs::analyze(std::io::Cursor::new(full));
                    r.facts.push(fact("Lines", count(lr.lines)));
                    if let (Some(a), Some(b)) = (lr.first_ts, lr.last_ts) {
                        r.facts.push(fact(
                            "Time range",
                            format!("{} to {}", date_time(a), date_time(b)),
                        ));
                    }
                    r.tables.push(Table {
                        title: "Levels".into(),
                        columns: vec!["Level".into(), "Lines".into()],
                        rows: lr
                            .levels
                            .iter()
                            .map(|(k, v)| vec![k.clone(), count(*v)])
                            .collect(),
                    });
                    r.tables.push(Table {
                        title: "Top messages".into(),
                        columns: ["Count", "Level", "Message"].map(String::from).into(),
                        rows: lr
                            .templates
                            .iter()
                            .take(20)
                            .map(|t| vec![count(t.count), t.level.clone(), t.template.clone()])
                            .collect(),
                    });
                    r.llm = Some(LlmPlan {
                        prompt: "log_triage".into(),
                        facts_text: logs::facts(&lr),
                        excerpt: None,
                    });
                } else {
                    r.kind = "text".into();
                    text_facts(&mut r, &ex, "text");
                }
            }
        }
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::fixtures::{docx, pdf, write};

    fn table<'a>(r: &'a FileReport, title: &str) -> &'a Table {
        r.tables
            .iter()
            .find(|t| t.title == title)
            .unwrap_or_else(|| panic!("no table {title}"))
    }

    #[test]
    fn rust_code() {
        let d = tempfile::tempdir().unwrap();
        let src = "use std::io;\nuse crate::foo::Bar;\n\n// TODO: handle errors\n/// Doc\npub struct Thing {\n    a: u32,\n}\n\nimpl Thing {\n    pub fn new() -> Self {\n        Thing { a: 1 }\n    }\n\n    fn long(&self, x: u32) -> u32 {\n        if x > 1 && x < 5 {\n            for i in 0..x {\n                println!(\"{i}\");\n            }\n        }\n        x\n    }\n}\n\n/* block\n comment */\nfn main() {}\n";
        let p = write(d.path(), "lib.rs", src.as_bytes());
        let r = file_report(&p).unwrap();
        assert_eq!(r.kind, "code");
        let f = |l: &str| r.facts.iter().find(|f| f.label == l).unwrap().value.clone();
        assert_eq!(f("Language"), "Rust");
        assert_eq!(f("Functions"), "3");
        assert_eq!(f("Types"), "1");
        assert_eq!(f("Imports"), "2");
        assert!(f("Lines").contains("comments"));
        let long = table(&r, "Longest functions");
        assert_eq!(long.rows[0][0], "long");
        assert_eq!(
            long.rows[0][3], "3",
            "if, &&, for -> branches: {:?}",
            long.rows[0]
        );
        assert_eq!(table(&r, "TODO / FIXME").rows[0], ["4", "TODO", "handle errors"]);
        let llm = r.llm.unwrap();
        assert_eq!(llm.prompt, "file_report_code");
        assert!(
            llm.facts_text.contains("long function: long at line 15")
                && llm.excerpt.unwrap().starts_with("use std::io;")
        );
    }

    #[test]
    fn python_code_uses_indentation() {
        let d = tempfile::tempdir().unwrap();
        let src = "import os\nfrom a.b import c\n\nclass Foo:\n    def one(self):\n        return 1\n\n    def two(self, x):\n        if x:\n            return 2\n        return 3\n\n# FIXME later\ndef top():\n    pass\n";
        let r = file_report(&write(d.path(), "m.py", src.as_bytes())).unwrap();
        let f = |l: &str| r.facts.iter().find(|f| f.label == l).unwrap().value.clone();
        assert_eq!(
            (
                f("Language").as_str(),
                f("Functions").as_str(),
                f("Types").as_str(),
                f("Imports").as_str()
            ),
            ("Python", "3", "1", "2")
        );
        assert_eq!(table(&r, "Longest functions").rows[0][0], "two");
    }

    #[test]
    fn text_markdown_with_outline() {
        let d = tempfile::tempdir().unwrap();
        let body =
            "The quick brown fox jumps over the lazy dog and keeps running through the forest. ".repeat(60);
        let r = file_report(&write(
            d.path(),
            "n.md",
            format!("# Title\n\n{body}\n\n## Part two\n\nMore words here.").as_bytes(),
        ))
        .unwrap();
        assert_eq!(r.kind, "text");
        assert_eq!(table(&r, "Outline").rows, [["1", "Title"], ["2", "Part two"]]);
        assert!(
            r.facts
                .iter()
                .any(|f| f.label == "Language" && f.value == "English")
        );
        assert!(r.facts.iter().any(|f| f.label == "Reading time"));
        assert_eq!(r.llm.unwrap().prompt, "doc_summarize");
    }

    #[test]
    fn pdf_and_docx() {
        let d = tempfile::tempdir().unwrap();
        let r = file_report(&write(
            d.path(),
            "a.pdf",
            &pdf(&["Hello world page one", "", "Page three"]),
        ))
        .unwrap();
        assert_eq!(r.kind, "pdf");
        assert!(r.facts.iter().any(|f| f.label == "Pages" && f.value == "3"));
        assert!(r.flags[0].text.contains("scanned") && r.flags[0].text.contains('2'));
        let r = file_report(&write(
            d.path(),
            "a.docx",
            &docx(&[("Heading1", "Report"), ("", "Body text goes here.")]),
        ))
        .unwrap();
        assert_eq!(r.kind, "docx");
        assert_eq!(table(&r, "Outline").rows[0], ["1", "Report"]);
    }

    #[test]
    fn csv_json_and_logs() {
        let d = tempfile::tempdir().unwrap();
        let r = file_report(&write(d.path(), "t.csv", b"id,city\n1,Paris\n2,Rome\n")).unwrap();
        assert_eq!(r.kind, "csv");
        assert!(r.facts.iter().any(|f| f.label == "Rows" && f.value == "2"));
        assert_eq!(table(&r, "Columns").rows.len(), 2);
        assert_eq!(r.llm.as_ref().unwrap().prompt, "data_profile");
        let r = file_report(&write(d.path(), "d.json", br#"[{"a":1},{"a":2,"b":"x"}]"#)).unwrap();
        assert_eq!(r.kind, "json");
        assert_eq!(table(&r, "Keys").rows[0][0], "a");
        let log = "2026-10-11 14:00:00 INFO started\n2026-10-11 14:00:01 ERROR failed to open 12\n2026-10-11 14:00:02 ERROR failed to open 13\n";
        let r = file_report(&write(d.path(), "app.log", log.as_bytes())).unwrap();
        assert_eq!(r.kind, "log");
        assert_eq!(table(&r, "Top messages").rows[0][0], "2");
        let r = file_report(&write(d.path(), "output.txt", log.as_bytes())).unwrap();
        assert_eq!(r.kind, "log", "a .txt that looks like a log is treated as one");
        assert_eq!(r.llm.unwrap().prompt, "log_triage");
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 2, 0, 0, 0, 0, 0, 0, 0]);
        b
    }

    /// A tiny JPEG with an EXIF block that has Make and a GPS IFD.
    fn jpeg_with_gps() -> Vec<u8> {
        let mut t: Vec<u8> = b"II*\0\x08\0\0\0".to_vec();
        // IFD0: 2 entries: Make (0x010F ASCII "Acme\0" in 4 bytes? 5 bytes -> offset) and GPSInfo pointer (0x8825).
        let ifd0_len = 2 + 2 * 12 + 4;
        let make_off = 8 + ifd0_len;
        let gps_off = make_off + 5 + 1;
        t.extend(2u16.to_le_bytes());
        t.extend([0x0F, 0x01, 2, 0, 5, 0, 0, 0]);
        t.extend((make_off as u32).to_le_bytes());
        t.extend([0x25, 0x88, 4, 0, 1, 0, 0, 0]);
        t.extend((gps_off as u32).to_le_bytes());
        t.extend(0u32.to_le_bytes());
        t.extend(b"Acme\0\0");
        // GPS IFD: one entry GPSLatitudeRef (0x0001, ASCII, 2 bytes "N\0").
        t.extend(1u16.to_le_bytes());
        t.extend([0x01, 0x00, 2, 0, 2, 0, 0, 0, b'N', 0, 0, 0]);
        t.extend(0u32.to_le_bytes());
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend(t);
        let mut j = vec![0xFF, 0xD8, 0xFF, 0xE1];
        j.extend(((app1.len() + 2) as u16).to_be_bytes());
        j.extend(app1);
        // SOF0 with 4x3
        j.extend([0xFF, 0xC0, 0, 11, 8, 0, 3, 0, 4, 1, 1, 0x11, 0]);
        j.extend([0xFF, 0xD9]);
        j
    }

    #[test]
    fn images_report_dimensions_and_flag_gps() {
        let d = tempfile::tempdir().unwrap();
        let r = file_report(&write(d.path(), "a.png", &png(1920, 1080))).unwrap();
        assert_eq!(r.kind, "image");
        assert!(
            r.facts
                .iter()
                .any(|f| f.label == "Dimensions" && f.value == "1920 × 1080 px")
        );
        assert!(
            r.facts
                .iter()
                .any(|f| f.label == "Megapixels" && f.value == "2.1")
        );
        assert!(r.flags.is_empty());
        let j = jpeg_with_gps();
        assert_eq!(image_dimensions(&j), Some(("JPEG", 4, 3)));
        let r = file_report(&write(d.path(), "photo.jpg", &j)).unwrap();
        assert!(
            r.facts.iter().any(|f| f.label == "Camera" && f.value == "Acme"),
            "{:?}",
            r.facts
        );
        assert!(r.flags.iter().any(|f| f.text.contains("GPS")), "{:?}", r.flags);
    }

    #[test]
    fn zip_listing() {
        use std::io::Write;
        let d = tempfile::tempdir().unwrap();
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("a.txt", o).unwrap();
            z.write_all(&[b'x'; 5000]).unwrap();
            z.start_file("dir/b.txt", o).unwrap();
            z.write_all(b"hi").unwrap();
            z.add_directory("dir2/", o).unwrap();
            z.finish().unwrap();
        }
        let r = file_report(&write(d.path(), "x.zip", &buf.into_inner())).unwrap();
        assert_eq!(r.kind, "zip");
        assert!(r.facts.iter().any(|f| f.label == "Files" && f.value == "2"));
        assert!(r.facts.iter().any(|f| f.label == "Folders" && f.value == "1"));
        assert_eq!(table(&r, "Largest entries").rows[0][0], "a.txt");
    }

    #[test]
    fn errors() {
        let d = tempfile::tempdir().unwrap();
        assert!(matches!(file_report(d.path()), Err(FileReportError::IsFolder)));
        assert!(matches!(
            file_report(&d.path().join("nope.txt")),
            Err(FileReportError::Io(_))
        ));
        let p = write(d.path(), "blob.bin", &[0, 1, 2, 0]);
        assert!(file_report(&p).is_err());
    }
}
