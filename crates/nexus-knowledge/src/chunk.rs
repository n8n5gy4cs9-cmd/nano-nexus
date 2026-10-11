//! Heading-aware chunker: 600-800 token chunks (target 700), ~10% overlap, a breadcrumb per chunk
//! (`Doc title > H1 > H2`), and the page where the chunk starts. Fenced code is never split into
//! headings. A new heading starts a new chunk once the current one is at least a third full.

use nexus_analyze::chunk::estimate_tokens;

use crate::extract::KbPage;

/// Chunk sizes in estimated tokens.
#[derive(Clone, Copy, Debug)]
pub struct ChunkOpts {
    pub target: usize,
    pub overlap: usize,
}

impl Default for ChunkOpts {
    fn default() -> Self {
        Self {
            target: 700,
            overlap: 70,
        }
    }
}

/// A chunk ready to store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkOut {
    pub ord: u32,
    pub heading_path: String,
    pub page: Option<u32>,
    pub text: String,
    pub tokens: u32,
}

#[derive(Clone, Debug)]
struct Block {
    page: Option<u32>,
    path: String,
    text: String,
    tokens: usize,
}

fn heading_of(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_start();
    let level = t.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&level) && t[level..].starts_with(' ') {
        let title = t[level..].trim();
        (!title.is_empty()).then_some((level, title))
    } else {
        None
    }
}

/// Split `text` into pieces of at most `max` tokens, preferring sentence ends, then spaces.
fn split_long(text: &str, max: usize) -> Vec<String> {
    if estimate_tokens(text) <= max {
        return vec![text.to_owned()];
    }
    let max_chars = (max as f64 * 3.2) as usize;
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in text.split_inclusive(char::is_whitespace) {
        if cur.chars().count() + word.chars().count() > max_chars && !cur.is_empty() {
            // Back up to the last sentence end in the current piece when there is one.
            let cut = cur
                .rfind(['.', '!', '?', '\n'])
                .map(|i| i + 1)
                .filter(|i| *i > cur.len() / 2);
            match cut {
                Some(i) => {
                    let rest = cur[i..].to_owned();
                    cur.truncate(i);
                    out.push(std::mem::take(&mut cur));
                    cur = rest;
                }
                None => out.push(std::mem::take(&mut cur)),
            }
        }
        cur.push_str(word);
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out.into_iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

fn blocks(doc_title: &str, pages: &[KbPage], target: usize) -> Vec<Block> {
    let mut out = Vec::new();
    // (level, title) stack; the document title is the root of every breadcrumb.
    let mut stack: Vec<(usize, String)> = Vec::new();
    let path = |stack: &[(usize, String)]| {
        let mut p = doc_title.to_owned();
        for (_, t) in stack {
            p.push_str(" > ");
            p.push_str(t);
        }
        p
    };
    let mut in_fence = false;
    for page in pages {
        let mut para: Vec<&str> = Vec::new();
        let flush = |para: &mut Vec<&str>, stack: &[(usize, String)], out: &mut Vec<Block>| {
            if para.is_empty() {
                return;
            }
            let text = para.join("\n");
            para.clear();
            if text.trim().is_empty() {
                return;
            }
            for piece in split_long(&text, target) {
                out.push(Block {
                    page: page.number,
                    path: path(stack),
                    tokens: estimate_tokens(&piece),
                    text: piece,
                });
            }
        };
        for line in page.text.lines() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
            }
            match heading_of(line).filter(|_| !in_fence) {
                Some((level, title)) => {
                    flush(&mut para, &stack, &mut out);
                    // A document whose first heading repeats its title would double the root.
                    while stack.last().is_some_and(|(l, _)| *l >= level) {
                        stack.pop();
                    }
                    if !(stack.is_empty() && title.eq_ignore_ascii_case(doc_title)) {
                        stack.push((level, title.to_owned()));
                    }
                }
                None if line.trim().is_empty() && !in_fence => flush(&mut para, &stack, &mut out),
                None => para.push(line),
            }
        }
        flush(&mut para, &stack, &mut out);
    }
    out
}

/// Chunk a document into stored chunks.
pub fn chunk_document(doc_title: &str, pages: &[KbPage], opts: &ChunkOpts) -> Vec<ChunkOut> {
    let blocks = blocks(doc_title, pages, opts.target);
    let mut chunks: Vec<ChunkOut> = Vec::new();
    let mut cur: Vec<Block> = Vec::new();
    let mut cur_tokens = 0usize;

    fn emit(chunks: &mut Vec<ChunkOut>, cur: &[Block]) {
        let Some(first) = cur.first() else { return };
        let text = cur
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        chunks.push(ChunkOut {
            ord: chunks.len() as u32,
            heading_path: first.path.clone(),
            page: cur.iter().find_map(|b| b.page),
            tokens: estimate_tokens(&text) as u32,
            text,
        });
    }

    // `fresh` counts blocks that are not overlap seeds, so a seed alone is never emitted.
    let mut fresh = 0usize;
    for b in blocks {
        let section_change = cur.last().is_some_and(|l| l.path != b.path);
        let full = cur_tokens + b.tokens > opts.target;
        if fresh > 0 && (full || (section_change && cur_tokens >= opts.target / 3)) {
            emit(&mut chunks, &cur);
            // Overlap: trailing blocks of the same section, up to `overlap` tokens.
            let mut seed: Vec<Block> = Vec::new();
            let mut t = 0usize;
            if !section_change {
                for prev in cur.iter().rev() {
                    if t + prev.tokens > opts.overlap {
                        break;
                    }
                    t += prev.tokens;
                    seed.push(prev.clone());
                }
                seed.reverse();
            }
            cur = seed;
            cur_tokens = t;
            fresh = 0;
        }
        cur_tokens += b.tokens;
        fresh += 1;
        cur.push(b);
    }
    emit(&mut chunks, &cur);
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(n: Option<u32>, t: &str) -> KbPage {
        KbPage {
            number: n,
            text: t.to_owned(),
        }
    }

    #[test]
    fn breadcrumbs_follow_heading_levels() {
        let p = page(
            None,
            "# Manual\n\nIntro.\n\n## Routing\n\nBasics.\n\n### Sends\n\nSend text here.\n\n## Mixing\n\nMix text.",
        );
        let c = chunk_document(
            "Manual",
            &[p],
            &ChunkOpts {
                target: 5,
                overlap: 0,
            },
        );
        let paths: Vec<_> = c.iter().map(|c| c.heading_path.as_str()).collect();
        assert!(paths.contains(&"Manual > Routing > Sends"));
        assert!(paths.contains(&"Manual > Mixing"));
        assert!(!paths.iter().any(|p| p.contains("Manual > Manual")));
    }

    #[test]
    fn sizes_overlap_and_pages() {
        let para = "word ".repeat(150); // ~ 235 tokens
        let text = (0..8)
            .map(|_| para.trim().to_owned())
            .collect::<Vec<_>>()
            .join("\n\n");
        let pages = vec![page(Some(3), &text), page(Some(4), &text)];
        let c = chunk_document("Book", &pages, &ChunkOpts::default());
        assert!(c.len() >= 4);
        assert!(
            c.iter().all(|c| c.tokens <= 800),
            "{:?}",
            c.iter().map(|c| c.tokens).collect::<Vec<_>>()
        );
        assert_eq!(c[0].page, Some(3));
        assert!(c.iter().any(|c| c.page == Some(4)));
        assert!(c.iter().enumerate().all(|(i, c)| c.ord as usize == i));
    }

    #[test]
    fn overlap_repeats_tail_in_next_chunk() {
        let text = (0..6)
            .map(|i| format!("w{i} ").repeat(30).trim().to_owned())
            .collect::<Vec<_>>()
            .join("\n\n");
        let c = chunk_document(
            "T",
            &[page(None, &text)],
            &ChunkOpts {
                target: 80,
                overlap: 30,
            },
        );
        assert!(c.len() >= 2);
        assert!(c[0].text.starts_with("w0"));
        assert!(c[1].text.starts_with("w1"), "{}", c[1].text);
        assert!(c[1].text.contains("w2"));
    }

    #[test]
    fn fenced_code_hashes_are_not_headings() {
        let p = page(None, "# T\n\n```sh\n# not a heading\nls\n```\n\nafter");
        let c = chunk_document("T", &[p], &ChunkOpts::default());
        assert_eq!(c.len(), 1);
        assert!(c[0].text.contains("# not a heading"));
        assert_eq!(c[0].heading_path, "T");
    }

    #[test]
    fn giant_paragraph_is_split() {
        let p = page(None, &"sentence one. ".repeat(2000));
        let c = chunk_document("T", &[p], &ChunkOpts::default());
        assert!(c.len() > 2);
        assert!(c.iter().all(|c| c.tokens <= 800));
    }

    #[test]
    fn empty_input_gives_no_chunks() {
        assert!(chunk_document("T", &[page(None, "  \n\n ")], &ChunkOpts::default()).is_empty());
    }
}
