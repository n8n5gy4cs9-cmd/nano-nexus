//! Retrieval: deterministic query cleanup -> FTS5 BM25 (heading path x3, text x1) -> phrase boost and
//! coverage filter -> per-document diversity -> neighbor expansion -> token budget. Same `search()` API
//! a later embedding backend can sit behind.

use std::collections::HashSet;

use nexus_analyze::bm25::tokenize;
use nexus_analyze::chunk::estimate_tokens;
use nexus_store::Store;
use rusqlite::types::Value;

use crate::types::{KbError, Passage};

/// Search options. Defaults match the `kb_search` tool: top 4, <= 1,200 tokens, max 2 per document.
#[derive(Clone, Debug)]
pub struct SearchOpts {
    /// Restrict to these libraries (still only enabled ones). `None` = every enabled library.
    pub library_ids: Option<Vec<String>>,
    pub k: usize,
    pub max_tokens: usize,
    pub max_per_doc: usize,
    /// Chunks below this many tokens get their next chunk appended.
    pub expand_below: u32,
    /// Share of query terms that must appear in a hit (0..=1). Filters off-topic matches so the
    /// model can say "not in the library".
    pub min_coverage: f64,
    /// How many BM25 candidates to rerank.
    pub candidates: usize,
}

impl Default for SearchOpts {
    fn default() -> Self {
        Self {
            library_ids: None,
            k: 4,
            max_tokens: 1200,
            max_per_doc: 2,
            expand_below: 120,
            min_coverage: 0.5,
            candidates: 40,
        }
    }
}

/// A cleaned query.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Query {
    pub terms: Vec<String>,
    pub phrases: Vec<String>,
}

/// Split into quoted phrases and stopword-free terms.
pub(crate) fn parse_query(q: &str) -> Query {
    let mut phrases = Vec::new();
    let mut rest = String::new();
    let mut in_quote = false;
    let mut cur = String::new();
    for c in q.chars() {
        if c == '"' {
            if in_quote {
                let toks = tokenize(&cur);
                if toks.len() > 1 {
                    phrases.push(toks.join(" "));
                }
                rest.push(' ');
                rest.push_str(&cur);
                rest.push(' ');
                cur.clear();
            }
            in_quote = !in_quote;
        } else if in_quote {
            cur.push(c);
        } else {
            rest.push(c);
        }
    }
    rest.push(' ');
    rest.push_str(&cur); // an unterminated quote is just words
    let mut seen = HashSet::new();
    let terms = tokenize(&rest)
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect();
    Query { terms, phrases }
}

/// FTS5 MATCH expression: every term OR-ed (BM25 rewards the ones that co-occur).
fn match_expr(q: &Query) -> String {
    let mut parts: Vec<String> = q.terms.iter().map(|t| format!("\"{t}\"")).collect();
    parts.extend(q.phrases.iter().map(|p| format!("\"{p}\"")));
    parts.join(" OR ")
}

/// Crude stem so "routing" matches "route": compare 4-char prefixes.
fn stem(w: &str) -> String {
    w.chars().take(4).collect()
}

fn coverage(q: &Query, heading: &str, text: &str) -> f64 {
    if q.terms.is_empty() {
        return 0.0;
    }
    let hay: HashSet<String> = tokenize(&format!("{heading} {text}"))
        .iter()
        .map(|w| stem(w))
        .collect();
    let found = q.terms.iter().filter(|t| hay.contains(&stem(t))).count();
    found as f64 / q.terms.len() as f64
}

fn norm_ws(s: &str) -> String {
    tokenize(s).join(" ")
}

/// Search enabled libraries. Returns passages best-first within the token budget; empty when nothing
/// relevant exists (callers must then answer "not in the library").
pub fn search(store: &Store, query: &str, opts: &SearchOpts) -> Result<Vec<Passage>, KbError> {
    let q = parse_query(query);
    if q.terms.is_empty() {
        return Ok(Vec::new());
    }
    let expr = match_expr(&q);
    let mut sql = String::from(
        "SELECT c.id, c.document_id, c.ord, l.id, l.name, COALESCE(d.title, d.path), d.path,
                c.heading_path, c.page, c.text, c.tokens, bm25(kb_chunks_fts, 3.0, 1.0)
         FROM kb_chunks_fts
         JOIN kb_chunks c ON c.id = kb_chunks_fts.rowid
         JOIN kb_documents d ON d.id = c.document_id
         JOIN kb_libraries l ON l.id = d.library_id
         WHERE kb_chunks_fts MATCH ?1 AND l.enabled = 1",
    );
    let mut args: Vec<Value> = vec![Value::Text(expr)];
    if let Some(ids) = &opts.library_ids {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let marks: Vec<String> = (0..ids.len()).map(|i| format!("?{}", i + 2)).collect();
        sql.push_str(&format!(" AND l.id IN ({})", marks.join(",")));
        args.extend(ids.iter().cloned().map(Value::Text));
    }
    sql.push_str(&format!(" ORDER BY 12 LIMIT {}", opts.candidates.max(opts.k)));

    struct Cand {
        p: Passage,
        ord: i64,
    }
    let cands: Vec<Cand> = store.with_conn(|c| {
        let mut st = c.prepare(&sql)?;
        let rows = st
            .query_map(rusqlite::params_from_iter(args.iter()), |r| {
                Ok(Cand {
                    ord: r.get(2)?,
                    p: Passage {
                        chunk_id: r.get(0)?,
                        document_id: r.get(1)?,
                        library_id: r.get(3)?,
                        library_name: r.get(4)?,
                        document: r.get(5)?,
                        doc_path: r.get(6)?,
                        heading_path: r.get(7)?,
                        page: r.get::<_, Option<i64>>(8)?.map(|p| p as u32),
                        text: r.get(9)?,
                        tokens: r.get::<_, i64>(10)? as u32,
                        score: -r.get::<_, f64>(11)?,
                    },
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;

    let phrases: Vec<String> = q.phrases.clone();
    let mut ranked: Vec<Cand> = cands
        .into_iter()
        .filter(|c| coverage(&q, &c.p.heading_path, &c.p.text) >= opts.min_coverage)
        .map(|mut c| {
            let hay = norm_ws(&format!("{} {}", c.p.heading_path, c.p.text));
            if phrases.iter().any(|p| hay.contains(p.as_str())) {
                c.p.score += c.p.score.abs().max(1.0); // phrase boost: doubles the score
            }
            c
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.p.score
            .total_cmp(&a.p.score)
            .then(a.p.chunk_id.cmp(&b.p.chunk_id))
    });

    let mut out: Vec<Passage> = Vec::new();
    let mut per_doc: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
    let mut used_tokens = 0usize;
    for cand in ranked {
        if out.len() >= opts.k {
            break;
        }
        let n = per_doc.entry(cand.p.document_id).or_insert(0);
        if *n >= opts.max_per_doc {
            continue;
        }
        let mut p = cand.p;
        if p.tokens < opts.expand_below
            && let Some(next) = neighbor(store, p.document_id, cand.ord + 1)?
            && used_tokens + p.tokens as usize + next.1 as usize <= opts.max_tokens
        {
            p.text.push_str("\n\n");
            p.text.push_str(&next.0);
            p.tokens += next.1;
        }
        let cost = p.tokens as usize;
        if used_tokens + cost > opts.max_tokens {
            if out.is_empty() {
                // Never return nothing when something matched: trim the best passage to the budget.
                p.text = truncate_to_tokens(&p.text, opts.max_tokens);
                p.tokens = estimate_tokens(&p.text) as u32;
                used_tokens += p.tokens as usize;
                *n += 1;
                out.push(p);
            }
            continue;
        }
        used_tokens += cost;
        *n += 1;
        out.push(p);
    }
    Ok(out)
}

fn neighbor(store: &Store, document_id: i64, ord: i64) -> Result<Option<(String, u32)>, KbError> {
    use rusqlite::OptionalExtension;
    Ok(store.with_conn(|c| {
        Ok(c.query_row(
            "SELECT text, tokens FROM kb_chunks WHERE document_id = ?1 AND ord = ?2",
            rusqlite::params![document_id, ord],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u32)),
        )
        .optional()?)
    })?)
}

/// Cut `text` to about `max` tokens at a char boundary.
pub(crate) fn truncate_to_tokens(text: &str, max: usize) -> String {
    let max_chars = (max as f64 * 3.2) as usize;
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut s: String = text.chars().take(max_chars).collect();
    s.push('…');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{IndexOpts, index_library};
    use crate::repo;
    use crate::types::LibrarySource;
    use tokio_util::sync::CancellationToken;

    fn lib(s: &Store, name: &str, files: &[(&str, &str)]) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        for (n, t) in files {
            std::fs::write(dir.path().join(n), t).unwrap();
        }
        let l = repo::create_library(
            s,
            name,
            LibrarySource::User,
            &dir.path().to_string_lossy(),
            "reference",
        )
        .unwrap();
        index_library(s, &l.id, &IndexOpts::default(), &CancellationToken::new(), |_| {}).unwrap();
        (dir, l.id)
    }

    const MANUAL: &str = "# Manual\n\n## Routing\n\n### Sends\n\nA send routes a copy of a track to a bus so you can build a reverb return.\n\n## Mixing\n\nUse the faders to balance levels.\n";

    #[test]
    fn query_parsing() {
        let q = parse_query("how do I use \"pre fader\" sends?");
        assert_eq!(q.phrases, vec!["pre fader"]);
        assert!(q.terms.contains(&"sends".to_string()) && q.terms.contains(&"fader".to_string()));
        assert!(!q.terms.contains(&"how".to_string()));
        assert!(parse_query("the and of").terms.is_empty());
    }

    #[test]
    fn finds_the_right_section_with_citation_fields() {
        let s = Store::open_in_memory().unwrap();
        let (_d, _id) = lib(&s, "Reaper", &[("manual.md", MANUAL)]);
        let hits = search(&s, "how do sends work", &SearchOpts::default()).unwrap();
        assert!(!hits.is_empty());
        assert!(
            hits[0].heading_path.contains("Sends"),
            "{:?}",
            hits[0].heading_path
        );
        assert_eq!(hits[0].library_name, "Reaper");
        assert!(hits[0].source().starts_with("Reaper > Manual"));
    }

    #[test]
    fn unrelated_question_returns_nothing() {
        let s = Store::open_in_memory().unwrap();
        lib(&s, "Reaper", &[("manual.md", MANUAL)]);
        assert!(
            search(&s, "capital of France population", &SearchOpts::default())
                .unwrap()
                .is_empty()
        );
        assert!(search(&s, "", &SearchOpts::default()).unwrap().is_empty());
    }

    #[test]
    fn disabled_and_filtered_libraries_are_invisible() {
        let s = Store::open_in_memory().unwrap();
        let (_d1, a) = lib(
            &s,
            "A",
            &[("a.md", "# A\n\nsidechain compression ducks the bass")],
        );
        let (_d2, b) = lib(&s, "B", &[("b.md", "# B\n\nsidechain routing in the mixer")]);
        let all = search(&s, "sidechain", &SearchOpts::default()).unwrap();
        assert_eq!(all.len(), 2);
        repo::set_enabled(&s, &a, false).unwrap();
        let only_b = search(&s, "sidechain", &SearchOpts::default()).unwrap();
        assert_eq!(only_b.len(), 1);
        assert_eq!(only_b[0].library_id, b);
        let opts = SearchOpts {
            library_ids: Some(vec![a.clone()]),
            ..Default::default()
        };
        assert!(search(&s, "sidechain", &opts).unwrap().is_empty());
        repo::set_enabled(&s, &a, true).unwrap();
        repo::set_enabled(&s, &b, false).unwrap();
        assert_eq!(search(&s, "sidechain", &SearchOpts::default()).unwrap().len(), 1);
    }

    #[test]
    fn diversity_limits_chunks_per_document() {
        let s = Store::open_in_memory().unwrap();
        let body = (0..6)
            .map(|i| {
                format!(
                    "## Part {i}\n\n{}",
                    "compressor attack release ratio threshold. ".repeat(60)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        lib(
            &s,
            "L",
            &[
                ("big.md", &format!("# Big\n\n{body}")),
                ("small.md", "# Small\n\ncompressor basics explained"),
            ],
        );
        let hits = search(
            &s,
            "compressor attack",
            &SearchOpts {
                k: 6,
                ..Default::default()
            },
        )
        .unwrap();
        let from_big = hits.iter().filter(|h| h.doc_path == "big.md").count();
        assert!(from_big <= 2, "{from_big}");
        assert!(hits.iter().any(|h| h.doc_path == "small.md"));
    }

    #[test]
    fn budget_is_respected_and_best_passage_is_trimmed() {
        let s = Store::open_in_memory().unwrap();
        lib(
            &s,
            "L",
            &[(
                "a.md",
                &format!("# A\n\n{}", "reverb tail decay time. ".repeat(300)),
            )],
        );
        let hits = search(
            &s,
            "reverb decay",
            &SearchOpts {
                max_tokens: 100,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].tokens <= 101);
    }

    #[test]
    fn phrase_boost_and_heading_weight() {
        let s = Store::open_in_memory().unwrap();
        lib(
            &s,
            "L",
            &[(
                "a.md",
                "# Doc\n\n## Freeze tracks\n\nRender a track in place.\n\n## Other\n\nfreeze appears here once among many other words about tracks",
            )],
        );
        let hits = search(&s, "freeze tracks", &SearchOpts::default()).unwrap();
        assert!(
            hits[0].heading_path.ends_with("Freeze tracks"),
            "{:?}",
            hits[0].heading_path
        );
    }

    #[test]
    fn stemming_matches_inflections() {
        let s = Store::open_in_memory().unwrap();
        lib(&s, "L", &[("a.md", "# A\n\nRouting audio between tracks")]);
        assert!(
            !search(&s, "route audio", &SearchOpts::default())
                .unwrap()
                .is_empty()
        );
    }
}
