//! Document chunking (ANALYZER §2): ~700-token chunks with ~80 tokens of overlap that keep page numbers.

use serde::{Deserialize, Serialize};

use crate::extract::Page;

pub const TARGET_TOKENS: usize = 700;
pub const OVERLAP_TOKENS: usize = 80;

/// `ceil(chars / 3.2)`, the same estimate the app uses when the tokenizer is unavailable.
pub fn estimate_tokens(text: &str) -> usize {
    (text.chars().count() as f64 / 3.2).ceil() as usize
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Chunk {
    pub index: u32,
    pub page_start: u32,
    pub page_end: u32,
    /// The closest heading above the chunk's first block.
    pub heading: Option<String>,
    pub text: String,
    pub tokens: u32,
}

#[derive(Clone, Debug)]
struct Block {
    page: u32,
    text: String,
    tokens: usize,
    heading: Option<String>,
}

/// Split text that is longer than `max` tokens at sentence ends, then at spaces.
fn split_long(text: &str, max: usize) -> Vec<String> {
    if estimate_tokens(text) <= max {
        return vec![text.to_owned()];
    }
    let max_chars = (max as f64 * 3.2) as usize;
    let mut out = Vec::new();
    let mut cur = String::new();
    for sentence in text.split_inclusive(['.', '!', '?', '\n']) {
        if sentence.chars().count() > max_chars {
            if !cur.trim().is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            let mut piece = String::new();
            for word in sentence.split_inclusive(' ') {
                if piece.chars().count() + word.chars().count() > max_chars && !piece.is_empty() {
                    out.push(std::mem::take(&mut piece));
                }
                piece.push_str(word);
            }
            cur = piece;
        } else if cur.chars().count() + sentence.chars().count() > max_chars && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            cur.push_str(sentence);
        } else {
            cur.push_str(sentence);
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out.into_iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

fn blocks(pages: &[Page], target: usize) -> Vec<Block> {
    let mut out = Vec::new();
    let mut heading: Option<String> = None;
    for p in pages {
        for para in p.text.split("\n\n").map(str::trim).filter(|s| !s.is_empty()) {
            if para.starts_with('#') {
                heading = Some(
                    para.trim_start_matches('#')
                        .trim()
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_owned(),
                );
            }
            for piece in split_long(para, target) {
                out.push(Block {
                    page: p.number,
                    tokens: estimate_tokens(&piece),
                    text: piece,
                    heading: heading.clone(),
                });
            }
        }
    }
    out
}

/// Chunk a document. A heading starts a new chunk when the current one is already at least half full.
pub fn chunk_doc(pages: &[Page], target: usize, overlap: usize) -> Vec<Chunk> {
    let bl = blocks(pages, target);
    let mut chunks = Vec::new();
    let mut cur: Vec<&Block> = Vec::new();
    let mut cur_tokens = 0usize;
    let emit = |cur: &[&Block], chunks: &mut Vec<Chunk>| {
        if cur.is_empty() {
            return;
        }
        let text = cur
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        chunks.push(Chunk {
            index: chunks.len() as u32,
            page_start: cur.iter().map(|b| b.page).min().unwrap_or(1),
            page_end: cur.iter().map(|b| b.page).max().unwrap_or(1),
            heading: cur.first().and_then(|b| b.heading.clone()),
            tokens: estimate_tokens(&text) as u32,
            text,
        });
    };
    for b in &bl {
        let starts_section = b.text.starts_with('#');
        let overflow = cur_tokens + b.tokens > target;
        if !cur.is_empty() && (overflow || (starts_section && cur_tokens >= target / 2)) {
            emit(&cur, &mut chunks);
            // Carry the tail of the previous chunk as overlap (not across a heading boundary).
            let mut carry: Vec<&Block> = Vec::new();
            let mut t = 0;
            if !starts_section {
                for prev in cur.iter().rev() {
                    if t + prev.tokens > overlap * 2 {
                        break;
                    }
                    carry.push(prev);
                    t += prev.tokens;
                    if t >= overlap {
                        break;
                    }
                }
                carry.reverse();
            }
            cur_tokens = carry.iter().map(|b| b.tokens).sum();
            cur = carry;
        }
        cur.push(b);
        cur_tokens += b.tokens;
    }
    emit(&cur, &mut chunks);
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(n: u32, text: &str) -> Page {
        Page {
            number: n,
            text: text.to_owned(),
        }
    }

    fn para(words: usize, tag: &str) -> String {
        (0..words)
            .map(|i| format!("{tag}{i}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn a_short_document_is_one_chunk() {
        let c = chunk_doc(
            &[page(1, "Hello there.\n\nSecond paragraph.")],
            TARGET_TOKENS,
            OVERLAP_TOKENS,
        );
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].page_start, c[0].page_end), (1, 1));
        assert!(c[0].text.contains("Second paragraph."));
    }

    #[test]
    fn chunks_stay_near_the_target_and_cover_every_page() {
        let pages: Vec<Page> = (1..=10)
            .map(|n| {
                page(
                    n,
                    &format!(
                        "{}\n\n{}",
                        para(120, &format!("p{n}a")),
                        para(120, &format!("p{n}b"))
                    ),
                )
            })
            .collect();
        let c = chunk_doc(&pages, 700, 80);
        assert!(c.len() >= 4);
        for ch in &c {
            assert!(
                ch.tokens <= 700 + 80,
                "chunk {} has {} tokens",
                ch.index,
                ch.tokens
            );
            assert!(ch.page_start <= ch.page_end);
        }
        assert_eq!(c[0].page_start, 1);
        assert_eq!(c.last().unwrap().page_end, 10);
        assert!(c.iter().enumerate().all(|(i, ch)| ch.index == i as u32));
        // Every word appears in some chunk.
        let all: String = c.iter().map(|ch| ch.text.as_str()).collect::<Vec<_>>().join(" ");
        assert!(all.contains("p10b119") && all.contains("p1a0"));
    }

    #[test]
    fn consecutive_chunks_overlap() {
        let pages = vec![page(
            1,
            &(0..30)
                .map(|i| para(40, &format!("w{i}x")))
                .collect::<Vec<_>>()
                .join("\n\n"),
        )];
        let c = chunk_doc(&pages, 300, 80);
        assert!(c.len() >= 3);
        for w in c.windows(2) {
            let last_para = w[0].text.rsplit("\n\n").next().unwrap();
            assert!(
                w[1].text.contains(last_para),
                "the next chunk repeats the end of the previous one"
            );
        }
    }

    #[test]
    fn headings_start_chunks_and_are_remembered() {
        let body = para(300, "x");
        let pages = vec![page(
            1,
            &format!("# Intro\n\n{body}\n\n# Methods\n\n{body}\n\nDetails here."),
        )];
        let c = chunk_doc(&pages, 700, 80);
        assert!(c.len() >= 2);
        assert_eq!(c[0].heading.as_deref(), Some("Intro"));
        assert!(c.iter().any(|ch| ch.heading.as_deref() == Some("Methods")));
    }

    #[test]
    fn one_giant_paragraph_is_split() {
        let c = chunk_doc(&[page(3, &para(5000, "g"))], 700, 80);
        assert!(c.len() >= 5);
        assert!(c.iter().all(|ch| ch.tokens <= 700 + 80 && ch.page_start == 3));
    }

    #[test]
    fn empty_input() {
        assert!(chunk_doc(&[], 700, 80).is_empty());
        assert!(chunk_doc(&[page(1, "   ")], 700, 80).is_empty());
    }
}
