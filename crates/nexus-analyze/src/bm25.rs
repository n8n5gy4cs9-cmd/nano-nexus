//! BM25 ranking (k1 = 1.2, b = 0.75) over chunks, with a lowercase unicode word tokenizer and English
//! stopwords (ANALYZER §2). Used by `doc_qa` and the `doc_search` tool.

use std::collections::HashMap;

use crate::chunk::Chunk;

const K1: f64 = 1.2;
const B: f64 = 0.75;

const STOPWORDS: &[&str] = &[
    "a", "about", "above", "after", "again", "all", "also", "am", "an", "and", "any", "are", "as", "at",
    "be", "because", "been", "before", "being", "below", "between", "both", "but", "by", "can", "could",
    "did", "do", "does", "doing", "down", "during", "each", "few", "for", "from", "further", "had", "has",
    "have", "having", "he", "her", "here", "hers", "him", "his", "how", "i", "if", "in", "into", "is", "it",
    "its", "just", "me", "more", "most", "my", "no", "nor", "not", "now", "of", "off", "on", "once", "only",
    "or", "other", "our", "ours", "out", "over", "own", "same", "she", "should", "so", "some", "such",
    "than", "that", "the", "their", "theirs", "them", "then", "there", "these", "they", "this", "those",
    "through", "to", "too", "under", "until", "up", "very", "was", "we", "were", "what", "when", "where",
    "which", "while", "who", "whom", "why", "will", "with", "would", "you", "your", "yours",
];

/// Lowercase words (letters and digits), without stopwords and single letters.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .filter(|w| {
            (w.chars().count() > 1 || w.chars().all(|c| c.is_ascii_digit()))
                && !STOPWORDS.contains(&w.as_str())
        })
        .collect()
}

#[derive(Debug)]
pub struct Bm25 {
    /// Per document: term → count.
    docs: Vec<HashMap<String, u32>>,
    lens: Vec<usize>,
    /// Term → number of documents containing it.
    df: HashMap<String, usize>,
    avg_len: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub index: usize,
    pub score: f64,
}

impl Bm25 {
    pub fn new<'a>(texts: impl IntoIterator<Item = &'a str>) -> Self {
        let mut docs = Vec::new();
        let mut lens = Vec::new();
        let mut df: HashMap<String, usize> = HashMap::new();
        for t in texts {
            let toks = tokenize(t);
            lens.push(toks.len());
            let mut tf: HashMap<String, u32> = HashMap::new();
            for w in toks {
                *tf.entry(w).or_default() += 1;
            }
            for w in tf.keys() {
                *df.entry(w.clone()).or_default() += 1;
            }
            docs.push(tf);
        }
        let avg_len = if lens.is_empty() {
            0.0
        } else {
            lens.iter().sum::<usize>() as f64 / lens.len() as f64
        };
        Self {
            docs,
            lens,
            df,
            avg_len,
        }
    }

    pub fn from_chunks(chunks: &[Chunk]) -> Self {
        Self::new(chunks.iter().map(|c| c.text.as_str()))
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// The best `k` documents for `query` (score > 0), highest first.
    pub fn search(&self, query: &str, k: usize) -> Vec<Hit> {
        let n = self.docs.len() as f64;
        let terms = tokenize(query);
        let mut hits: Vec<Hit> = self
            .docs
            .iter()
            .enumerate()
            .filter_map(|(i, tf)| {
                let mut score = 0.0;
                for t in &terms {
                    let Some(f) = tf.get(t).copied() else { continue };
                    let n_t = *self.df.get(t).unwrap_or(&0) as f64;
                    let idf = (1.0 + (n - n_t + 0.5) / (n_t + 0.5)).ln();
                    let f = f64::from(f);
                    let norm = if self.avg_len == 0.0 {
                        1.0
                    } else {
                        1.0 - B + B * self.lens[i] as f64 / self.avg_len
                    };
                    score += idf * (f * (K1 + 1.0)) / (f + K1 * norm);
                }
                (score > 0.0).then_some(Hit { index: i, score })
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.index.cmp(&b.index))
        });
        hits.truncate(k);
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_lowercases_and_drops_noise() {
        assert_eq!(
            tokenize("The Quick, brown fox's 2026 plan: A-B"),
            ["quick", "brown", "fox", "2026", "plan"]
        );
        assert_eq!(tokenize("Überraschung für Alle"), ["überraschung", "für", "alle"]);
        assert!(tokenize("the and of").is_empty());
    }

    #[test]
    fn ranks_the_relevant_chunk_first() {
        let docs = [
            "The warranty covers manufacturing defects for twenty-four months from purchase.",
            "Shipping takes five business days within the country.",
            "To reset the device hold the power button for ten seconds.",
            "Our return policy allows refunds within thirty days.",
        ];
        let idx = Bm25::new(docs);
        let hits = idx.search("how long is the warranty period", 3);
        assert_eq!(hits[0].index, 0);
        let hits = idx.search("reset power button", 3);
        assert_eq!(hits[0].index, 2);
        assert!(idx.search("zebra", 3).is_empty());
        assert!(idx.search("the of", 3).is_empty(), "only stopwords");
    }

    #[test]
    fn rare_terms_count_more_and_length_is_normalized() {
        let docs = [
            "common common common rare",
            "common common common common common common common common common common",
        ];
        let idx = Bm25::new(docs);
        let hits = idx.search("rare common", 2);
        assert_eq!(hits[0].index, 0);
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn limits_and_empty_index() {
        let idx = Bm25::new(["alpha beta", "alpha gamma", "alpha delta"]);
        assert_eq!(idx.search("alpha", 2).len(), 2);
        assert_eq!(idx.len(), 3);
        let empty = Bm25::new(std::iter::empty());
        assert!(empty.is_empty() && empty.search("x", 3).is_empty());
    }

    #[test]
    fn works_over_chunks() {
        let chunks = vec![
            Chunk {
                index: 0,
                page_start: 1,
                page_end: 1,
                heading: None,
                text: "apples and oranges".into(),
                tokens: 5,
            },
            Chunk {
                index: 1,
                page_start: 2,
                page_end: 2,
                heading: None,
                text: "the revenue grew twelve percent".into(),
                tokens: 5,
            },
        ];
        let hits = Bm25::from_chunks(&chunks).search("revenue growth", 1);
        assert_eq!(chunks[hits[0].index].page_start, 2);
    }
}
