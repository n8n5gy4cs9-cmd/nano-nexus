//! Text extraction (ANALYZER §2): plain/markdown/code, PDF, DOCX, and HTML into numbered pages.
//! Binary or oversized input is refused with a clear error; parsers that can panic run under `catch_unwind`.

use std::io::Read;
use std::path::Path;
use std::process::Command;

use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};

/// Files above this are not read whole.
pub const MAX_TEXT_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("this file type cannot be read as text: {0}")]
    Unsupported(String),
    #[error("the file is larger than {0} MB")]
    TooLarge(u64),
    #[error("this looks like a binary file, not text")]
    Binary,
    #[error("the file could not be read: {0}")]
    Io(#[from] std::io::Error),
    #[error("the document could not be parsed: {0}")]
    Parse(String),
}

/// One page of extracted text (1-based). Formats without pages (plain text, DOCX, HTML) are one page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Page {
    pub number: u32,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Extracted {
    /// `plain | pdf | docx | html`
    pub kind: String,
    pub title: Option<String>,
    pub pages: Vec<Page>,
    /// PDF pages that have no text layer (scanned images).
    pub scanned_pages: Vec<u32>,
}

impl Extracted {
    pub fn full_text(&self) -> String {
        self.pages
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
    pub fn word_count(&self) -> usize {
        self.pages.iter().map(|p| p.text.split_whitespace().count()).sum()
    }
}

fn one_page(kind: &str, title: Option<String>, text: String) -> Extracted {
    Extracted {
        kind: kind.into(),
        title,
        pages: vec![Page { number: 1, text }],
        scanned_pages: Vec::new(),
    }
}

/// Is `bytes` text? No NUL bytes in the first 8 KiB.
pub fn looks_like_text(bytes: &[u8]) -> bool {
    !bytes.iter().take(8192).any(|b| *b == 0)
}

/// Read plain text (also markdown and source code). Splits on form feeds into pages.
pub fn extract_plain(path: &Path) -> Result<Extracted, ExtractError> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_TEXT_BYTES {
        return Err(ExtractError::TooLarge(MAX_TEXT_BYTES / 1024 / 1024));
    }
    let bytes = std::fs::read(path)?;
    if !looks_like_text(&bytes) {
        return Err(ExtractError::Binary);
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if text.contains('\u{c}') {
        let pages = text
            .split('\u{c}')
            .enumerate()
            .map(|(i, t)| Page {
                number: i as u32 + 1,
                text: t.trim().to_owned(),
            })
            .collect();
        return Ok(Extracted {
            kind: "plain".into(),
            title: None,
            pages,
            scanned_pages: Vec::new(),
        });
    }
    Ok(one_page("plain", None, text))
}

// ---- PDF --------------------------------------------------------------------------------------

fn pdftotext_pages(path: &Path) -> Option<Vec<String>> {
    let out = Command::new("pdftotext")
        .arg("-layout")
        .arg("-enc")
        .arg("UTF-8")
        .arg(path)
        .arg("-")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut pages: Vec<String> = text.split('\u{c}').map(str::to_owned).collect();
    if pages.last().is_some_and(|p| p.trim().is_empty()) {
        pages.pop();
    }
    Some(pages)
}

/// Extract the text of each page. `pdf-extract` can panic on malformed files, so it runs under
/// `catch_unwind`; `pdftotext` is the fallback when it is installed.
pub fn extract_pdf(path: &Path) -> Result<Extracted, ExtractError> {
    let p = path.to_owned();
    let primary = std::panic::catch_unwind(move || pdf_extract::extract_text_by_pages(&p));
    let pages: Vec<String> = match primary {
        Ok(Ok(pages)) if pages.iter().any(|t| !t.trim().is_empty()) => pages,
        other => {
            let reason = match &other {
                Ok(Err(e)) => e.to_string(),
                Err(_) => "the PDF reader crashed on this file".into(),
                _ => "no text found".into(),
            };
            match pdftotext_pages(path) {
                Some(p) if p.iter().any(|t| !t.trim().is_empty()) => p,
                _ => match other {
                    Ok(Ok(pages)) => pages, // all pages empty: report them as scanned
                    _ => return Err(ExtractError::Parse(reason)),
                },
            }
        }
    };
    let mut scanned = Vec::new();
    let pages: Vec<Page> = pages
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            let n = i as u32 + 1;
            if t.trim().is_empty() {
                scanned.push(n);
            }
            Page {
                number: n,
                text: tidy(&t),
            }
        })
        .collect();
    Ok(Extracted {
        kind: "pdf".into(),
        title: None,
        pages,
        scanned_pages: scanned,
    })
}

/// Collapse runs of blank lines and trailing spaces.
fn tidy(s: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in s.lines() {
        let l = line.trim_end();
        if l.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(l);
        out.push('\n');
    }
    out.trim().to_owned()
}

// ---- DOCX -------------------------------------------------------------------------------------

/// Text of `word/document.xml`. Headings (`Heading1`..) get `#` prefixes so the chunker can split on them.
pub fn docx_text(xml: &str) -> Result<String, ExtractError> {
    let mut r = Reader::from_str(xml);
    let mut out = String::new();
    let mut para = String::new();
    let mut heading = 0usize;
    let mut in_text = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"t" => in_text = true,
                b"tab" => para.push('\t'),
                b"br" => para.push('\n'),
                b"pStyle" => {
                    heading = style_heading(&e).unwrap_or(heading);
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"tab" => para.push('\t'),
                b"br" => para.push('\n'),
                b"pStyle" => {
                    heading = style_heading(&e).unwrap_or(heading);
                }
                _ => {}
            },
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"t" => in_text = false,
                b"p" => {
                    let t = para.trim();
                    if !t.is_empty() {
                        if heading > 0 {
                            out.push_str(&"#".repeat(heading.min(6)));
                            out.push(' ');
                        }
                        out.push_str(t);
                        out.push_str("\n\n");
                    }
                    para.clear();
                    heading = 0;
                }
                _ => {}
            },
            Ok(Event::Text(t)) if in_text => {
                para.push_str(&t.unescape().map_err(|e| ExtractError::Parse(e.to_string()))?)
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(ExtractError::Parse(e.to_string())),
            _ => {}
        }
    }
    Ok(out.trim().to_owned())
}

fn style_heading(e: &quick_xml::events::BytesStart<'_>) -> Option<usize> {
    let v = e
        .attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == b"val")?;
    let v = String::from_utf8_lossy(&v.value).to_lowercase();
    if v == "title" {
        return Some(1);
    }
    v.strip_prefix("heading")
        .and_then(|n| n.trim().parse::<usize>().ok())
        .filter(|n| (1..=9).contains(n))
}

pub fn extract_docx(path: &Path) -> Result<Extracted, ExtractError> {
    if std::fs::metadata(path)?.len() > MAX_TEXT_BYTES * 5 {
        return Err(ExtractError::TooLarge(MAX_TEXT_BYTES * 5 / 1024 / 1024));
    }
    let mut zip =
        zip::ZipArchive::new(std::fs::File::open(path)?).map_err(|e| ExtractError::Parse(e.to_string()))?;
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .map_err(|e| ExtractError::Parse(format!("not a Word document: {e}")))?
        .take(MAX_TEXT_BYTES * 3)
        .read_to_string(&mut xml)?;
    let text = docx_text(&xml)?;
    let title = text
        .lines()
        .find(|l| l.starts_with("# "))
        .map(|l| l.trim_start_matches("# ").to_owned());
    Ok(one_page("docx", title, text))
}

// ---- HTML -------------------------------------------------------------------------------------

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        match tail.find(';').filter(|e| *e <= 10) {
            Some(end) => {
                let ent = &tail[1..end];
                let rep = match ent {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" | "#39" => Some('\''),
                    "nbsp" => Some(' '),
                    e if e.starts_with("#x") => {
                        u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32)
                    }
                    e if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
                    _ => None,
                };
                match rep {
                    Some(c) => {
                        out.push(c);
                        rest = &tail[end + 1..];
                    }
                    None => {
                        out.push('&');
                        rest = &tail[1..];
                    }
                }
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Strip tags, scripts, and styles; keep paragraph breaks and headings.
pub fn html_text(html: &str) -> (Option<String>, String) {
    let mut title = None;
    let mut out = String::new();
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let Some(end) = html[i..].find('>').map(|e| i + e) else {
                break;
            };
            let tag = lower[i + 1..end]
                .trim_start_matches('/')
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or("");
            let closing = lower[i + 1..].starts_with('/');
            if !closing && matches!(tag, "script" | "style" | "noscript") {
                let close = format!("</{tag}");
                i = lower[end..]
                    .find(&close)
                    .and_then(|c| lower[end + c..].find('>').map(|g| end + c + g + 1))
                    .unwrap_or(bytes.len());
                continue;
            }
            if tag == "title"
                && !closing
                && let Some(c) = lower[end..].find("</title")
            {
                title = Some(decode_entities(html[end + 1..end + c].trim()));
            }
            match tag {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" if !closing => {
                    out.push_str("\n\n");
                    out.push_str(&"#".repeat(tag[1..].parse::<usize>().unwrap_or(1)));
                    out.push(' ');
                }
                "p" | "div" | "br" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol"
                | "table" | "section" | "article" | "blockquote" | "pre" => out.push('\n'),
                _ => {}
            }
            i = end + 1;
        } else {
            let next = html[i..].find('<').map_or(bytes.len(), |n| i + n);
            out.push_str(&html[i..next]);
            i = next;
        }
    }
    let decoded = decode_entities(&out);
    // The title tag's text was copied as body text; drop it when it leads the document.
    let body = tidy(&decoded.split('\n').map(str::trim).collect::<Vec<_>>().join("\n"));
    let body = match &title {
        Some(t) if body.starts_with(t.as_str()) => body[t.len()..].trim_start().to_owned(),
        _ => body,
    };
    (title, body)
}

pub fn extract_html(path: &Path) -> Result<Extracted, ExtractError> {
    if std::fs::metadata(path)?.len() > MAX_TEXT_BYTES {
        return Err(ExtractError::TooLarge(MAX_TEXT_BYTES / 1024 / 1024));
    }
    let raw = std::fs::read(path)?;
    let (title, text) = html_text(&String::from_utf8_lossy(&raw));
    Ok(one_page("html", title, text))
}

/// Extract by file type.
pub fn extract(path: &Path) -> Result<Extracted, ExtractError> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => extract_pdf(path),
        "docx" => extract_docx(path),
        "html" | "htm" | "xhtml" => extract_html(path),
        "doc" | "xls" | "xlsx" | "ppt" | "pptx" | "zip" | "png" | "jpg" | "jpeg" | "gif" | "mp3" | "mp4"
        | "mov" | "exe" | "dmg" => Err(ExtractError::Unsupported(format!(".{ext}"))),
        _ => extract_plain(path),
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::io::Write;
    use std::path::Path;

    /// A minimal valid PDF with one text page per entry.
    pub fn pdf(pages: &[&str]) -> Vec<u8> {
        let mut objs: Vec<String> = Vec::new();
        let n = pages.len();
        objs.push("<< /Type /Catalog /Pages 2 0 R >>".into());
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + i * 2)).collect();
        objs.push(format!(
            "<< /Type /Pages /Kids [{}] /Count {n} >>",
            kids.join(" ")
        ));
        objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
        for (i, text) in pages.iter().enumerate() {
            let stream = if text.is_empty() {
                String::new()
            } else {
                format!(
                    "BT /F1 12 Tf 72 720 Td ({}) Tj ET",
                    text.replace('(', "\\(").replace(')', "\\)")
                )
            };
            objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> >> >>", 5 + i * 2));
            objs.push(format!(
                "<< /Length {} >>\nstream\n{stream}\nendstream",
                stream.len()
            ));
        }
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for o in &offsets {
            out.extend(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objs.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    /// A minimal DOCX with the given `(style, text)` paragraphs.
    pub fn docx(paras: &[(&str, &str)]) -> Vec<u8> {
        let body: String = paras
            .iter()
            .map(|(style, text)| {
                let ppr = if style.is_empty() {
                    String::new()
                } else {
                    format!("<w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr>")
                };
                format!(
                    "<w:p>{ppr}<w:r><w:t>{}</w:t></w:r></w:p>",
                    text.replace('&', "&amp;").replace('<', "&lt;")
                )
            })
            .collect();
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}</w:body></w:document>"
        );
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            z.start_file("[Content_Types].xml", opts).unwrap();
            z.write_all(b"<Types/>").unwrap();
            z.start_file("word/document.xml", opts).unwrap();
            z.write_all(xml.as_bytes()).unwrap();
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    pub fn write(dir: &Path, name: &str, data: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, data).unwrap();
        p
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn plain_text_and_pages_by_form_feed() {
        let d = tempfile::tempdir().unwrap();
        let p = write(d.path(), "a.md", "# Title\nhello".as_bytes());
        let e = extract(&p).unwrap();
        assert_eq!((e.kind.as_str(), e.pages.len(), e.word_count()), ("plain", 1, 3));
        let p = write(d.path(), "b.txt", b"page one\x0cpage two\x0cpage three");
        assert_eq!(
            extract(&p)
                .unwrap()
                .pages
                .iter()
                .map(|p| p.number)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
    }

    #[test]
    fn binary_and_unsupported_files_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let p = write(d.path(), "blob.dat", &[0, 1, 2, 3, 0]);
        assert!(matches!(extract(&p), Err(ExtractError::Binary)));
        let p = write(d.path(), "x.png", b"png");
        assert!(matches!(extract(&p), Err(ExtractError::Unsupported(_))));
        assert!(matches!(
            extract(&d.path().join("nope.txt")),
            Err(ExtractError::Io(_))
        ));
    }

    #[test]
    fn pdf_pages_are_numbered_and_scanned_pages_flagged() {
        let d = tempfile::tempdir().unwrap();
        let p = write(
            d.path(),
            "doc.pdf",
            &pdf(&["First page about apples", "", "Third page about oranges"]),
        );
        let e = extract(&p).unwrap();
        assert_eq!(e.kind, "pdf");
        assert_eq!(e.pages.len(), 3);
        assert!(e.pages[0].text.contains("apples"), "{:?}", e.pages);
        assert!(e.pages[2].text.contains("oranges"));
        assert_eq!(e.scanned_pages, [2]);
    }

    #[test]
    fn a_broken_pdf_is_an_error_not_a_crash() {
        let d = tempfile::tempdir().unwrap();
        let p = write(d.path(), "bad.pdf", b"%PDF-1.4\nthis is not really a pdf");
        assert!(extract(&p).is_err());
        let p = write(d.path(), "empty.pdf", b"");
        assert!(extract(&p).is_err());
    }

    #[test]
    fn docx_headings_and_text() {
        let d = tempfile::tempdir().unwrap();
        let p = write(
            d.path(),
            "r.docx",
            &docx(&[
                ("Heading1", "Quarterly report"),
                ("", "Revenue grew 12% & margins held."),
                ("Heading2", "Risks"),
                ("", "Supply chain."),
            ]),
        );
        let e = extract(&p).unwrap();
        assert_eq!(e.kind, "docx");
        assert_eq!(e.title.as_deref(), Some("Quarterly report"));
        let t = e.full_text();
        assert!(
            t.contains("# Quarterly report\n\nRevenue grew 12% & margins held.\n\n## Risks"),
            "{t}"
        );
        let p = write(d.path(), "notword.docx", b"not a zip");
        assert!(matches!(extract(&p), Err(ExtractError::Parse(_))));
    }

    #[test]
    fn html_is_stripped() {
        let html = "<html><head><title>My &amp; Page</title><style>p{color:red}</style><script>var x = '<p>no</p>';</script></head><body><h1>Hello</h1><p>One &lt;two&gt; three.</p><ul><li>a</li><li>b</li></ul></body></html>";
        let (title, text) = html_text(html);
        assert_eq!(title.as_deref(), Some("My & Page"));
        assert!(
            text.contains("# Hello")
                && text.contains("One <two> three.")
                && !text.contains("color")
                && !text.contains("var x"),
            "{text}"
        );
        assert!(
            text.lines().any(|l| l == "a") && text.lines().any(|l| l == "b"),
            "{text}"
        );
    }

    #[test]
    fn entities() {
        assert_eq!(
            decode_entities("a &amp; b &#65; &#x42; &nbsp;c &bogus; &"),
            "a & b A B  c &bogus; &"
        );
    }
}
