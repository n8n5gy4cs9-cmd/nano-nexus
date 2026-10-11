//! Extractors for library documents. Output is pages of markdown-ish text where `#` lines are
//! headings, so the chunker can build breadcrumbs. M08's extractors are reused for PDF, DOCX, HTML,
//! and plain text; EPUB, JSON, and CSV are added here. Images and scanned PDFs are skipped with a note.

use std::io::Read;
use std::path::{Path, PathBuf};

use nexus_analyze::extract as m08;
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::types::KbError;

/// One page of text. `number` is `None` for formats without real pages (EPUB, HTML, JSON, ...).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KbPage {
    pub number: Option<u32>,
    pub text: String,
}

/// An extracted document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KbDoc {
    pub kind: String,
    pub title: Option<String>,
    pub pages: Vec<KbPage>,
    /// Visible notes such as "12 scanned pages skipped (no OCR)".
    pub notes: Vec<String>,
}

const SOURCE_EXT: &[&str] = &[
    "rs", "py", "js", "jsx", "ts", "tsx", "go", "java", "kt", "c", "h", "cpp", "hpp", "cs", "rb", "php",
    "swift", "sh", "lua", "sql", "toml", "yaml", "yml", "ini", "cfg", "css", "scss",
];
const TEXT_EXT: &[&str] = &["md", "markdown", "txt", "text", "rst", "log"];
const MAX_EPUB_BYTES: u64 = 200 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 20 * 1024 * 1024;

fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Can this file type be indexed?
pub fn is_supported(path: &Path) -> bool {
    let e = ext_of(path);
    matches!(
        e.as_str(),
        "pdf" | "epub" | "docx" | "html" | "htm" | "xhtml" | "json" | "csv" | "tsv"
    ) || TEXT_EXT.contains(&e.as_str())
        || SOURCE_EXT.contains(&e.as_str())
}

fn plain_page(text: String) -> Vec<KbPage> {
    vec![KbPage { number: None, text }]
}

/// Extract a file by type.
pub fn extract_file(path: &Path) -> Result<KbDoc, KbError> {
    let e = ext_of(path);
    let err = |m: m08::ExtractError| KbError::Extract(m.to_string());
    match e.as_str() {
        "pdf" => {
            let x = m08::extract_pdf(path).map_err(err)?;
            let mut notes = Vec::new();
            if !x.scanned_pages.is_empty() {
                notes.push(format!(
                    "{} page(s) have no text layer and were skipped (no OCR in v1)",
                    x.scanned_pages.len()
                ));
            }
            let pages: Vec<KbPage> = x
                .pages
                .into_iter()
                .filter(|p| !p.text.trim().is_empty())
                .map(|p| KbPage {
                    number: Some(p.number),
                    text: p.text,
                })
                .collect();
            if pages.is_empty() {
                return Err(KbError::Extract(
                    "scanned PDF without a text layer (no OCR in v1)".into(),
                ));
            }
            Ok(KbDoc {
                kind: "pdf".into(),
                title: x.title,
                pages,
                notes,
            })
        }
        "epub" => extract_epub(path),
        "docx" => {
            let x = m08::extract_docx(path).map_err(err)?;
            Ok(KbDoc {
                kind: "docx".into(),
                title: x.title,
                pages: plain_page(x.full_text()),
                notes: Vec::new(),
            })
        }
        "html" | "htm" | "xhtml" => {
            let x = m08::extract_html(path).map_err(err)?;
            Ok(KbDoc {
                kind: "html".into(),
                title: x.title,
                pages: plain_page(x.full_text()),
                notes: Vec::new(),
            })
        }
        "json" => extract_json(path),
        "csv" | "tsv" => extract_csv(path, if e == "tsv" { b'\t' } else { b',' }),
        _ if TEXT_EXT.contains(&e.as_str()) => {
            let x = m08::extract_plain(path).map_err(err)?;
            let title = x
                .pages
                .iter()
                .flat_map(|p| p.text.lines())
                .find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_owned()));
            // Form-feed separated text keeps real page numbers; single-page text has none.
            let multi = x.pages.len() > 1;
            let pages = x
                .pages
                .into_iter()
                .map(|p| KbPage {
                    number: multi.then_some(p.number),
                    text: p.text,
                })
                .collect();
            Ok(KbDoc {
                kind: if matches!(e.as_str(), "md" | "markdown") {
                    "markdown"
                } else {
                    "text"
                }
                .into(),
                title,
                pages,
                notes: Vec::new(),
            })
        }
        _ if SOURCE_EXT.contains(&e.as_str()) => {
            let x = m08::extract_plain(path).map_err(err)?;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            // Source files get one heading (the file name) and a fenced body so `#` comments are not headings.
            let body = format!("# {name}\n\n```{e}\n{}\n```", x.full_text());
            Ok(KbDoc {
                kind: "source".into(),
                title: Some(name),
                pages: plain_page(body),
                notes: Vec::new(),
            })
        }
        _ => Err(KbError::Extract(format!("unsupported file type .{e}"))),
    }
}

// ---- JSON -------------------------------------------------------------------------------------

fn flatten_json(v: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
    use serde_json::Value;
    match v {
        Value::Object(m) => {
            for (k, child) in m {
                let p = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_json(child, &p, out);
            }
        }
        Value::Array(a) => {
            for (i, child) in a.iter().enumerate() {
                flatten_json(child, &format!("{prefix}[{i}]"), out);
            }
        }
        Value::String(s) => out.push(format!("{prefix}: {s}")),
        Value::Null => {}
        other => out.push(format!("{prefix}: {other}")),
    }
}

fn extract_json(path: &Path) -> Result<KbDoc, KbError> {
    if std::fs::metadata(path)?.len() > m08::MAX_TEXT_BYTES {
        return Err(KbError::Extract("JSON file is too large".into()));
    }
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)
        .map_err(|e| KbError::Extract(format!("invalid JSON: {e}")))?;
    let mut lines = Vec::new();
    flatten_json(&v, "", &mut lines);
    // Group by top-level key so each group has a heading.
    let mut text = String::new();
    let mut last_top = String::new();
    for l in lines {
        let top = l.split(['.', '[', ':']).next().unwrap_or("").to_owned();
        if top != last_top && !top.is_empty() {
            text.push_str(&format!("\n\n## {top}\n\n"));
            last_top = top;
        }
        text.push_str(&l);
        text.push('\n');
    }
    Ok(KbDoc {
        kind: "json".into(),
        title: None,
        pages: plain_page(text.trim().to_owned()),
        notes: Vec::new(),
    })
}

// ---- CSV --------------------------------------------------------------------------------------

fn extract_csv(path: &Path, delim: u8) -> Result<KbDoc, KbError> {
    if std::fs::metadata(path)?.len() > m08::MAX_TEXT_BYTES {
        return Err(KbError::Extract("CSV file is too large".into()));
    }
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delim)
        .flexible(true)
        .from_path(path)
        .map_err(|e| KbError::Extract(e.to_string()))?;
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| KbError::Extract(e.to_string()))?
        .iter()
        .map(str::to_owned)
        .collect();
    let mut text = String::new();
    for (i, rec) in rdr.records().enumerate() {
        let Ok(rec) = rec else { continue };
        if i % 50 == 0 {
            text.push_str(&format!("\n\n## Rows {}-{}\n\n", i + 1, i + 50));
        }
        let row: Vec<String> = rec
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.trim().is_empty())
            .map(|(j, v)| format!("{}: {}", headers.get(j).map_or("col", String::as_str), v.trim()))
            .collect();
        text.push_str(&row.join("; "));
        text.push('\n');
    }
    Ok(KbDoc {
        kind: "csv".into(),
        title: None,
        pages: plain_page(text.trim().to_owned()),
        notes: Vec::new(),
    })
}

// ---- EPUB -------------------------------------------------------------------------------------

fn read_entry<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Result<String, KbError> {
    let mut f = zip
        .by_name(name)
        .map_err(|e| KbError::Extract(format!("EPUB entry `{name}`: {e}")))?;
    let mut buf = Vec::new();
    f.by_ref().take(MAX_ENTRY_BYTES).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Resolve `href` relative to the directory of `base` (both zip-style `/` paths); drops `#fragment`.
pub(crate) fn resolve_href(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or("");
    let mut parts: Vec<&str> = match base.rfind('/') {
        Some(i) => base[..i].split('/').collect(),
        None => Vec::new(),
    };
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

struct Opf {
    title: Option<String>,
    /// (id, href, media-type, properties)
    manifest: Vec<(String, String, String, String)>,
    spine: Vec<String>,
}

fn attr(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> String {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
        .unwrap_or_default()
}

fn parse_opf(xml: &str) -> Opf {
    let mut r = Reader::from_str(xml);
    let mut opf = Opf {
        title: None,
        manifest: Vec::new(),
        spine: Vec::new(),
    };
    let mut in_title = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"item" => opf.manifest.push((
                    attr(&e, b"id"),
                    attr(&e, b"href"),
                    attr(&e, b"media-type"),
                    attr(&e, b"properties"),
                )),
                b"itemref" => opf.spine.push(attr(&e, b"idref")),
                b"title" if opf.title.is_none() => in_title = true,
                _ => {}
            },
            Ok(Event::Text(t)) if in_title => {
                opf.title = t
                    .unescape()
                    .ok()
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty());
                in_title = false;
            }
            Ok(Event::End(_)) => in_title = false,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    opf
}

fn container_opf_path(xml: &str) -> Option<String> {
    let mut r = Reader::from_str(xml);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.local_name().as_ref() == b"rootfile" => {
                let p = attr(&e, b"full-path");
                if !p.is_empty() {
                    return Some(p);
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

/// EPUB: read `META-INF/container.xml` -> `content.opf` -> spine order; each XHTML item becomes a chapter
/// headed by its own `<h1>`/`<title>`; `nav` documents are skipped.
fn extract_epub(path: &Path) -> Result<KbDoc, KbError> {
    if std::fs::metadata(path)?.len() > MAX_EPUB_BYTES {
        return Err(KbError::Extract("EPUB is larger than 200 MB".into()));
    }
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)
        .map_err(|e| KbError::Extract(format!("not an EPUB: {e}")))?;
    let container = read_entry(&mut zip, "META-INF/container.xml")?;
    let opf_path =
        container_opf_path(&container).ok_or_else(|| KbError::Extract("EPUB has no content.opf".into()))?;
    let opf = parse_opf(&read_entry(&mut zip, &opf_path)?);
    let mut pages = Vec::new();
    let mut notes = Vec::new();
    let mut skipped = 0usize;
    for (n, idref) in opf.spine.iter().enumerate() {
        let Some((_, href, media, props)) = opf.manifest.iter().find(|m| &m.0 == idref) else {
            continue;
        };
        if props.split_whitespace().any(|p| p == "nav") || !media.contains("html") {
            continue;
        }
        let entry = resolve_href(&opf_path, href);
        let Ok(raw) = read_entry(&mut zip, &entry) else {
            skipped += 1;
            continue;
        };
        let (title, mut body) = m08::html_text(&raw);
        if body.trim().is_empty() {
            continue;
        }
        if !body.lines().any(|l| l.starts_with('#')) {
            let heading = title.unwrap_or_else(|| format!("Chapter {}", n + 1));
            body = format!("# {heading}\n\n{body}");
        }
        pages.push(KbPage {
            number: None,
            text: body,
        });
    }
    if skipped > 0 {
        notes.push(format!("{skipped} chapter file(s) could not be read"));
    }
    if pages.is_empty() {
        return Err(KbError::Extract("EPUB has no readable chapters".into()));
    }
    Ok(KbDoc {
        kind: "epub".into(),
        title: opf.title,
        pages,
        notes,
    })
}

/// All supported files below `root`, sorted, hidden files and folders skipped.
pub fn list_files(root: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
        .flatten()
    {
        if entry.file_type().is_file() && is_supported(entry.path()) {
            let rel = entry
                .path()
                .strip_prefix(root)
                .unwrap_or(entry.path())
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push((entry.path().to_path_buf(), rel));
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_epub(dir: &Path) -> PathBuf {
        let p = dir.join("book.epub");
        let f = std::fs::File::create(&p).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("META-INF/container.xml", o).unwrap();
        z.write_all(
            br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#,
        )
        .unwrap();
        z.start_file("OEBPS/content.opf", o).unwrap();
        z.write_all(
            br#"<package><metadata><dc:title>Mixing Guide</dc:title></metadata>
<manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
<item id="c2" href="text/two.xhtml" media-type="application/xhtml+xml"/>
<item id="c1" href="text/one.xhtml" media-type="application/xhtml+xml"/></manifest>
<spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#,
        )
        .unwrap();
        z.start_file("OEBPS/nav.xhtml", o).unwrap();
        z.write_all(b"<html><body><h1>Contents</h1></body></html>")
            .unwrap();
        z.start_file("OEBPS/text/one.xhtml", o).unwrap();
        z.write_all(b"<html><head><title>One</title></head><body><h1>Gain staging</h1><p>Keep peaks below minus six.</p></body></html>").unwrap();
        z.start_file("OEBPS/text/two.xhtml", o).unwrap();
        z.write_all(b"<html><head><title>Two</title></head><body><p>Compression basics.</p></body></html>")
            .unwrap();
        z.finish().unwrap();
        p
    }

    #[test]
    fn epub_follows_spine_and_skips_nav() {
        let dir = tempfile::tempdir().unwrap();
        let d = extract_file(&make_epub(dir.path())).unwrap();
        assert_eq!(d.kind, "epub");
        assert_eq!(d.title.as_deref(), Some("Mixing Guide"));
        assert_eq!(d.pages.len(), 2);
        assert!(d.pages[0].text.contains("Gain staging"));
        assert!(d.pages[1].text.starts_with("# Two"));
        assert!(!d.pages.iter().any(|p| p.text.contains("Contents")));
    }

    #[test]
    fn href_resolution() {
        assert_eq!(
            resolve_href("OEBPS/content.opf", "text/a.xhtml#x"),
            "OEBPS/text/a.xhtml"
        );
        assert_eq!(
            resolve_href("OEBPS/text/a.xhtml", "../img/b.png"),
            "OEBPS/img/b.png"
        );
        assert_eq!(resolve_href("content.opf", "a.xhtml"), "a.xhtml");
    }

    #[test]
    fn json_is_flattened_with_headings() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.json");
        std::fs::write(&p, r#"{"routing":{"sends":["pre","post"]},"count":3}"#).unwrap();
        let d = extract_file(&p).unwrap();
        let t = &d.pages[0].text;
        assert!(t.contains("## routing"));
        assert!(t.contains("routing.sends[1]: post"));
        assert!(t.contains("count: 3"));
    }

    #[test]
    fn csv_rows_become_key_value_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.csv");
        std::fs::write(&p, "key,action\nCtrl+S,Save\nCtrl+Z,Undo\n").unwrap();
        let d = extract_file(&p).unwrap();
        assert!(d.pages[0].text.contains("key: Ctrl+S; action: Save"));
    }

    #[test]
    fn markdown_title_and_source_fence() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("a.md");
        std::fs::write(&md, "# Reaper manual\n\n## Routing\n\nSends.").unwrap();
        assert_eq!(extract_file(&md).unwrap().title.as_deref(), Some("Reaper manual"));
        let py = dir.path().join("s.py");
        std::fs::write(&py, "# comment\nprint(1)\n").unwrap();
        let d = extract_file(&py).unwrap();
        assert!(d.pages[0].text.starts_with("# s.py"));
        assert!(d.pages[0].text.contains("```py"));
    }

    #[test]
    fn unsupported_and_listing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "x").unwrap();
        std::fs::write(dir.path().join("pic.png"), [0u8, 1]).unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git/b.md"), "x").unwrap();
        let files = list_files(dir.path());
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].1, "a.md");
        assert!(extract_file(&dir.path().join("pic.png")).is_err());
    }
}
