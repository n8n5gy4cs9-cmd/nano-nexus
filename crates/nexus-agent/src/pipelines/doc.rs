//! Document pipelines: `doc_summarize` (map-reduce, ANALYZER §2 / R13) and `doc_qa` (BM25 + citations).

use std::path::PathBuf;

use nexus_analyze::bm25::Bm25;
use nexus_analyze::chunk::{Chunk, OVERLAP_TOKENS, TARGET_TOKENS, chunk_doc};
use nexus_analyze::extract::{self, Extracted};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    Artifact, JsonCheck, PipelineCx, PipelineError, PipelineOutput, authorize_read, llm_schema,
    render_prompt, stream_plan,
};

/// Most chunks the map step reads; longer documents are sampled evenly.
pub const MAX_MAP_CHUNKS: usize = 60;
/// Chunks given to the answer step.
pub const QA_CHUNKS: usize = 4;

/// An extracted, chunked, searchable document.
#[derive(Debug)]
pub struct DocIndex {
    pub title: String,
    pub pages: u32,
    pub chunks: Vec<Chunk>,
    pub bm25: Bm25,
}

impl DocIndex {
    pub fn from_extracted(title: &str, e: &Extracted) -> Result<Self, PipelineError> {
        if e.word_count() == 0 {
            return Err(PipelineError::Input(if e.scanned_pages.is_empty() {
                "This file has no text to read.".into()
            } else {
                "This PDF has no text layer (it is probably scanned images), so it cannot be read yet.".into()
            }));
        }
        let chunks = chunk_doc(&e.pages, TARGET_TOKENS, OVERLAP_TOKENS);
        let bm25 = Bm25::from_chunks(&chunks);
        Ok(Self {
            title: e.title.clone().unwrap_or_else(|| title.to_owned()),
            pages: e.pages.len() as u32,
            chunks,
            bm25,
        })
    }

    /// The best chunks for `query`, in document order.
    pub fn top(&self, query: &str, k: usize) -> Vec<&Chunk> {
        let mut hits: Vec<usize> = self.bm25.search(query, k).into_iter().map(|h| h.index).collect();
        hits.sort_unstable();
        hits.into_iter().map(|i| &self.chunks[i]).collect()
    }
}

pub async fn load_doc(cx: &PipelineCx<'_>, path: PathBuf) -> Result<DocIndex, PipelineError> {
    let path = authorize_read(cx, &path, "to read the document").await?;
    cx.say("Reading the document…");
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let e = tokio::task::spawn_blocking(move || extract::extract(&path))
        .await
        .map_err(|e| PipelineError::Failed(e.to_string()))?
        .map_err(|e| PipelineError::Input(e.to_string()))?;
    DocIndex::from_extracted(&name, &e)
}

/// "summarize", "overview", "key points", "tl;dr", or no real question.
pub fn is_summary_request(q: &str) -> bool {
    let l = q.trim().to_lowercase();
    if l.split_whitespace().count() < 2 && !l.contains('?') {
        return true;
    }
    [
        "summar",
        "overview",
        "key point",
        "tl;dr",
        "tldr",
        "main point",
        "gist",
        "what is this about",
        "what's this about",
        "what is it about",
    ]
    .iter()
    .any(|k| l.contains(k))
}

// ---- map step ---------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Notes {
    pub points: Vec<Point>,
    pub numbers: Vec<Figure>,
    pub terms: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub text: String,
    pub page: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Figure {
    pub label: String,
    pub value: String,
    pub page: u32,
}

/// Parse and check one map reply: points exist and every page is inside the chunk's range.
pub fn parse_notes(v: &Value, first: u32, last: u32) -> Result<Notes, Vec<String>> {
    let n: Notes = serde_json::from_value(v.clone()).map_err(|e| vec![e.to_string()])?;
    let mut errs = Vec::new();
    if n.points.is_empty() {
        errs.push("no points".to_owned());
    }
    for p in &n.points {
        if !(first..=last).contains(&p.page) {
            errs.push(format!("page {} is outside {first}-{last}", p.page));
        }
    }
    for f in &n.numbers {
        if !(first..=last).contains(&f.page) {
            errs.push(format!("page {} is outside {first}-{last}", f.page));
        }
    }
    if errs.is_empty() { Ok(n) } else { Err(errs) }
}

/// Notes without the model: the first two sentences of the chunk.
pub fn fallback_notes(c: &Chunk) -> Notes {
    let body: String = c
        .text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ");
    let mut points = Vec::new();
    for s in body
        .split_inclusive(['.', '!', '?'])
        .map(str::trim)
        .filter(|s| s.len() > 20)
        .take(2)
    {
        points.push(Point {
            text: s.chars().take(300).collect(),
            page: c.page_start,
        });
    }
    if points.is_empty() {
        points.push(Point {
            text: body.chars().take(200).collect(),
            page: c.page_start,
        });
    }
    Notes {
        points,
        numbers: Vec::new(),
        terms: Vec::new(),
    }
}

/// Spread `max` picks evenly over `n` items (keeps the first and last).
pub fn sample_indices(n: usize, max: usize) -> Vec<usize> {
    if n <= max {
        return (0..n).collect();
    }
    (0..max).map(|i| i * (n - 1) / (max - 1)).collect()
}

pub struct SummaryRequest {
    pub path: PathBuf,
    pub user_request: Option<String>,
}

pub async fn doc_summarize(
    cx: &PipelineCx<'_>,
    req: SummaryRequest,
) -> Result<PipelineOutput, PipelineError> {
    let doc = load_doc(cx, req.path).await?;
    summarize_index(cx, &doc, req.user_request).await
}

pub async fn summarize_index(
    cx: &PipelineCx<'_>,
    doc: &DocIndex,
    user_request: Option<String>,
) -> Result<PipelineOutput, PipelineError> {
    let schema = llm_schema(include_str!("../../../../schemas/llm/doc-map-notes.schema.json"));
    let picks = sample_indices(doc.chunks.len(), MAX_MAP_CHUNKS);
    let total = picks.len();
    let mut notes: Vec<Notes> = Vec::new();
    let mut fallbacks = 0usize;
    for (n, &i) in picks.iter().enumerate() {
        cx.check_cancel()?;
        cx.say(format!("Reading part {} of {total}…", n + 1));
        let c = &doc.chunks[i];
        let (prompt, sampling) = render_prompt(
            "doc_map",
            &json!({"doc_title": doc.title, "chunk_index": n + 1, "chunk_count": total, "page_start": c.page_start, "page_end": c.page_end, "text": c.text}),
        )?;
        let check = JsonCheck {
            pages: Some((c.page_start, c.page_end)),
            ..JsonCheck::default()
        };
        let parsed = cx
            .llm
            .json(prompt, &schema, sampling, &check, cx.cancel)
            .await
            .and_then(|v| parse_notes(&v, c.page_start, c.page_end).ok());
        notes.push(match parsed {
            Some(n) => n,
            None => {
                fallbacks += 1;
                fallback_notes(c)
            }
        });
    }
    cx.check_cancel()?;
    cx.say("Writing the summary…");
    let facts = notes_facts(&notes);
    let vars = json!({"doc_title": doc.title, "page_count": doc.pages, "notes": notes, "user_request": user_request.unwrap_or_default()});
    let plan = stream_plan("doc_reduce", &vars, facts)?;
    let mut out = PipelineOutput::stream(plan, Artifact::None);
    if fallbacks > 0 {
        out.notes.push(format!(
            "{fallbacks} of {total} parts were summarized without the model."
        ));
    }
    if doc.chunks.len() > total {
        out.notes.push(format!(
            "This is a long document, so {total} of {} parts were read.",
            doc.chunks.len()
        ));
    }
    Ok(out)
}

/// Text the summary's numbers can be checked against.
fn notes_facts(notes: &[Notes]) -> String {
    let mut s = String::new();
    for n in notes {
        for p in &n.points {
            s.push_str(&format!("{} [p. {}]\n", p.text, p.page));
        }
        for f in &n.numbers {
            s.push_str(&format!("{}: {} [p. {}]\n", f.label, f.value, f.page));
        }
    }
    s
}

// ---- Q&A --------------------------------------------------------------------------------------

pub struct QaRequest {
    pub path: PathBuf,
    pub question: String,
}

pub async fn doc_qa(cx: &PipelineCx<'_>, req: QaRequest) -> Result<PipelineOutput, PipelineError> {
    let doc = load_doc(cx, req.path).await?;
    answer_from_index(cx, &doc, &req.question).await
}

pub async fn answer_from_index(
    cx: &PipelineCx<'_>,
    doc: &DocIndex,
    question: &str,
) -> Result<PipelineOutput, PipelineError> {
    if is_summary_request(question) {
        return summarize_index(
            cx,
            doc,
            (!question.trim().is_empty()).then(|| question.to_owned()),
        )
        .await;
    }
    cx.say("Finding the relevant parts…");
    let top = doc.top(question, QA_CHUNKS);
    if top.is_empty() {
        return Ok(PipelineOutput::text(
            "The document doesn't say. I couldn't find any part of it that matches your question. Try different words, or ask for a summary.",
            Artifact::None,
        ));
    }
    let chunks: Vec<Value> = top
        .iter()
        .map(|c| json!({"page_start": c.page_start, "page_end": c.page_end, "text": c.text}))
        .collect();
    let facts = top.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join("\n");
    let plan = stream_plan(
        "doc_qa",
        &json!({"doc_title": doc.title, "chunks": chunks, "question": question}),
        facts,
    )?;
    Ok(PipelineOutput::stream(plan, Artifact::None))
}

/// Result lines for the `doc_search` tool: `[p. N] snippet`.
pub fn search_lines(doc: &DocIndex, query: &str, k: usize) -> Vec<String> {
    doc.bm25
        .search(query, k)
        .into_iter()
        .map(|h| {
            let c = &doc.chunks[h.index];
            let pages = if c.page_start == c.page_end {
                format!("p. {}", c.page_start)
            } else {
                format!("pp. {}-{}", c.page_start, c.page_end)
            };
            let snippet: String = c.text.split_whitespace().take(40).collect::<Vec<_>>().join(" ");
            format!("[{pages}] {snippet}…")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::testkit::{Scripted, gate_for, write};
    use super::*;
    use nexus_analyze::extract::Page;
    use std::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    fn cx<'a>(
        llm: &'a Scripted,
        gate: &'a crate::tools::EngineGatekeeper,
        cancel: &'a CancellationToken,
        log: &'static Mutex<Vec<String>>,
    ) -> PipelineCx<'a> {
        PipelineCx {
            llm,
            gate,
            cancel,
            status: Box::leak(Box::new(move |s: String| log.lock().unwrap().push(s))),
            home: None,
        }
    }

    fn pages(n: usize) -> Vec<Page> {
        (1..=n)
            .map(|i| Page {
                number: i as u32,
                text: format!(
                    "Page {i} talks about topic{i}. The budget for topic{i} is {} dollars. {}",
                    i * 100,
                    "Filler sentence about nothing in particular. ".repeat(30)
                ),
            })
            .collect()
    }

    fn index(n: usize) -> DocIndex {
        DocIndex::from_extracted(
            "report.pdf",
            &Extracted {
                kind: "pdf".into(),
                title: None,
                pages: pages(n),
                scanned_pages: vec![],
            },
        )
        .unwrap()
    }

    fn good_notes(page: u32) -> Option<Value> {
        Some(
            json!({"points": [{"text": format!("Point from page {page}"), "page": page}], "numbers": [{"label": "budget", "value": "$500", "page": page}], "terms": ["topic"]}),
        )
    }

    #[test]
    fn intent_detection() {
        for q in [
            "summarize this",
            "Give me an overview",
            "key points?",
            "TL;DR",
            "",
            "summary",
        ] {
            assert!(is_summary_request(q), "{q}");
        }
        for q in ["What is the budget for topic3?", "when does the warranty end?"] {
            assert!(!is_summary_request(q), "{q}");
        }
    }

    #[test]
    fn notes_are_checked_against_the_chunk_pages() {
        let ok = good_notes(3).unwrap();
        assert!(parse_notes(&ok, 3, 4).is_ok());
        assert!(parse_notes(&ok, 5, 6).is_err());
        assert!(parse_notes(&json!({"points": [], "numbers": [], "terms": []}), 1, 9).is_err());
        assert!(parse_notes(&json!({"nope": 1}), 1, 9).is_err());
    }

    #[test]
    fn sampling_keeps_the_ends() {
        assert_eq!(sample_indices(5, 60), [0, 1, 2, 3, 4]);
        let s = sample_indices(200, 60);
        assert_eq!((s.len(), s[0], *s.last().unwrap()), (60, 0, 199));
        assert!(s.windows(2).all(|w| w[0] < w[1]));
    }

    #[tokio::test]
    async fn map_reduce_summarizes_with_page_refs() {
        let doc = index(8);
        assert!(doc.chunks.len() >= 3);
        let replies: Vec<Option<Value>> = doc.chunks.iter().map(|c| good_notes(c.page_start)).collect();
        let llm = Scripted::new(replies);
        let (gate, cancel) = (gate_for(std::path::Path::new("/")), CancellationToken::new());
        let log: &'static Mutex<Vec<String>> = Box::leak(Box::new(Mutex::new(vec![])));
        let out = summarize_index(&cx(&llm, &gate, &cancel, log), &doc, Some("short please".into()))
            .await
            .unwrap();
        assert_eq!(llm.calls(), doc.chunks.len());
        let plan = out.stream.unwrap();
        assert!(plan.system.contains("final summary"));
        assert!(
            plan.user.contains("User request: short please")
                && plan.user.contains("Document: report.pdf (8 pages)")
                && plan.user.contains("Point from page 1 [p. 1]")
                && plan.user.contains("$500: budget [p. 1]"),
            "{}",
            plan.user
        );
        assert!(out.notes.is_empty());
        assert!(
            log.lock()
                .unwrap()
                .iter()
                .any(|s| s.starts_with("Reading part 1 of"))
        );
    }

    #[tokio::test]
    async fn failed_map_calls_fall_back_to_the_text() {
        let doc = index(4);
        let llm = Scripted::new(vec![
            None,
            Some(json!({"points": [{"text": "bad page", "page": 99}], "numbers": [], "terms": []})),
        ]);
        let (gate, cancel) = (gate_for(std::path::Path::new("/")), CancellationToken::new());
        let log: &'static Mutex<Vec<String>> = Box::leak(Box::new(Mutex::new(vec![])));
        let out = summarize_index(&cx(&llm, &gate, &cancel, log), &doc, None)
            .await
            .unwrap();
        assert_eq!(out.notes.len(), 1);
        assert!(out.notes[0].contains(&format!("{} of {} parts", doc.chunks.len(), doc.chunks.len())));
        let user = out.stream.unwrap().user;
        assert!(user.contains("Page 1 talks about topic1."), "{user}");
        assert!(!user.contains("bad page"));
    }

    #[tokio::test]
    async fn qa_finds_the_right_page_and_cites_it() {
        let doc = index(12);
        let llm = Scripted::new(vec![]);
        let (gate, cancel) = (gate_for(std::path::Path::new("/")), CancellationToken::new());
        let log: &'static Mutex<Vec<String>> = Box::leak(Box::new(Mutex::new(vec![])));
        let out = answer_from_index(
            &cx(&llm, &gate, &cancel, log),
            &doc,
            "What is the budget for topic7?",
        )
        .await
        .unwrap();
        let plan = out.stream.unwrap();
        assert!(plan.user.contains("Question: What is the budget for topic7?"));
        assert!(
            plan.user
                .contains("Page 7 talks about topic7. The budget for topic7 is 700 dollars."),
            "the page with the answer is in the prompt"
        );
        assert!(plan.user.matches("<doc pages=").count() <= QA_CHUNKS);
        assert!(plan.system.contains("[p. N]"));
        assert_eq!(llm.calls(), 0);
    }

    #[tokio::test]
    async fn qa_without_a_match_says_so_and_summary_questions_summarize() {
        let doc = index(3);
        let llm = Scripted::new(doc.chunks.iter().map(|c| good_notes(c.page_start)).collect());
        let (gate, cancel) = (gate_for(std::path::Path::new("/")), CancellationToken::new());
        let log: &'static Mutex<Vec<String>> = Box::leak(Box::new(Mutex::new(vec![])));
        let out = answer_from_index(
            &cx(&llm, &gate, &cancel, log),
            &doc,
            "What is the airspeed velocity of a swallow?",
        )
        .await
        .unwrap();
        assert!(out.stream.is_none() && out.text.unwrap().starts_with("The document doesn't say"));
        let out = answer_from_index(&cx(&llm, &gate, &cancel, log), &doc, "key points")
            .await
            .unwrap();
        assert!(out.stream.unwrap().system.contains("final summary"));
        assert!(llm.calls() > 0);
    }

    #[tokio::test]
    async fn real_files_load_through_the_gatekeeper() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "notes.md",
            format!(
                "# Notes\n\n{}",
                "The warranty lasts twenty-four months. ".repeat(50)
            )
            .as_bytes(),
        );
        let (llm, gate, cancel) = (
            Scripted::new(vec![]),
            gate_for(d.path()),
            CancellationToken::new(),
        );
        let log: &'static Mutex<Vec<String>> = Box::leak(Box::new(Mutex::new(vec![])));
        let out = doc_qa(
            &cx(&llm, &gate, &cancel, log),
            QaRequest {
                path: d.path().join("notes.md"),
                question: "How long does the warranty last?".into(),
            },
        )
        .await
        .unwrap();
        assert!(out.stream.unwrap().user.contains("twenty-four months"));
        write(d.path(), "empty.txt", b"   ");
        assert!(
            doc_qa(
                &cx(&llm, &gate, &cancel, log),
                QaRequest {
                    path: d.path().join("empty.txt"),
                    question: "x y z?".into()
                }
            )
            .await
            .is_err()
        );
        let scanned = DocIndex::from_extracted(
            "s.pdf",
            &Extracted {
                kind: "pdf".into(),
                title: None,
                pages: vec![Page {
                    number: 1,
                    text: String::new(),
                }],
                scanned_pages: vec![1],
            },
        );
        assert!(matches!(scanned, Err(PipelineError::Input(m)) if m.contains("scanned")));
    }

    #[test]
    fn search_lines_cite_pages() {
        let doc = index(6);
        let lines = search_lines(&doc, "budget topic4", 3);
        assert!(
            lines[0].starts_with("[p. 4]") || lines[0].contains("p. 4") || lines[0].contains("pp."),
            "{lines:?}"
        );
        assert!(lines.iter().all(|l| l.ends_with('…')));
    }
}
