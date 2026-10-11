//! Context budgeting (INFERENCE-CONTRACT §6, ASSISTANTS §4): count tokens (cached), decide how much
//! history fits, and say when older turns need a summary.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Mutex;

use nexus_llm::{LlmClient, Supervisor};

/// Something that can count tokens for the loaded model.
pub trait Tokenize: Send + Sync {
    /// Exact token count, or `None` when the model is unavailable (the caller then estimates).
    fn count(&self, text: &str) -> impl Future<Output = Option<u32>> + Send;
}

impl Tokenize for LlmClient {
    async fn count(&self, text: &str) -> Option<u32> {
        self.tokenize(text).await.ok().map(|t| t.len() as u32)
    }
}

/// Counts through whatever server the supervisor currently runs (none while not ready: estimates).
impl Tokenize for Supervisor {
    async fn count(&self, text: &str) -> Option<u32> {
        let client = self.client().await?;
        client.count(text).await
    }
}

/// Never asks the model; always estimates.
pub struct EstimateOnly;

impl Tokenize for EstimateOnly {
    async fn count(&self, _text: &str) -> Option<u32> {
        None
    }
}

/// `ceil(chars / 3.2)`: the fallback when `/tokenize` is unavailable.
pub fn estimate_tokens(text: &str) -> u32 {
    (text.chars().count() as f64 / 3.2).ceil() as u32
}

struct Lru {
    map: HashMap<u64, u32>,
    order: VecDeque<u64>,
    cap: usize,
}

impl Lru {
    fn get(&mut self, k: u64) -> Option<u32> {
        let v = *self.map.get(&k)?;
        self.order.retain(|x| *x != k);
        self.order.push_back(k);
        Some(v)
    }

    fn put(&mut self, k: u64, v: u32) {
        if self.map.insert(k, v).is_some() {
            self.order.retain(|x| *x != k);
        } else if self.map.len() > self.cap
            && let Some(old) = self.order.pop_front()
        {
            self.map.remove(&old);
        }
        self.order.push_back(k);
    }
}

/// Token counter with an LRU cache keyed by a hash of the text.
pub struct TokenCounter<T: Tokenize> {
    tokenizer: T,
    cache: Mutex<Lru>,
}

impl<T: Tokenize> TokenCounter<T> {
    pub fn new(tokenizer: T) -> Self {
        Self::with_capacity(tokenizer, 512)
    }

    pub fn with_capacity(tokenizer: T, cap: usize) -> Self {
        Self {
            tokenizer,
            cache: Mutex::new(Lru {
                map: HashMap::new(),
                order: VecDeque::new(),
                cap,
            }),
        }
    }

    fn key(text: &str) -> u64 {
        let mut h = DefaultHasher::new();
        text.len().hash(&mut h);
        text.hash(&mut h);
        h.finish()
    }

    /// Token count of `text`: cached, else exact from the model, else estimated. Estimates are not cached.
    pub async fn count(&self, text: &str) -> u32 {
        if text.is_empty() {
            return 0;
        }
        let key = Self::key(text);
        if let Some(v) = self.cache.lock().ok().and_then(|mut c| c.get(key)) {
            return v;
        }
        match self.tokenizer.count(text).await {
            Some(n) => {
                if let Ok(mut c) = self.cache.lock() {
                    c.put(key, n);
                }
                n
            }
            None => estimate_tokens(text),
        }
    }
}

/// Per-slice limits, scaled from the 8192-token table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub ctx: u32,
    pub system: u32,
    pub attachments: u32,
    pub history: u32,
    pub summary: u32,
    /// Tokens reserved for the answer (`max_tokens`; 2048 with thinking on).
    pub answer: u32,
    pub margin: u32,
}

impl Limits {
    /// The contract table at `ctx` tokens. Thinking doubles the answer reserve, taken from attachments.
    pub fn for_ctx(ctx: u32, thinking: bool) -> Self {
        let scale = |base: u32| ((base as u64 * ctx as u64) / 8192) as u32;
        let answer = scale(if thinking { 2048 } else { 1024 });
        let mut attachments = scale(3000);
        if thinking {
            attachments = attachments.saturating_sub(scale(1024));
        }
        Self {
            ctx,
            system: scale(600),
            attachments,
            history: scale(2400),
            summary: scale(300),
            answer,
            margin: scale(200).max(200),
        }
    }

    /// Everything except history and summary.
    pub fn fixed(&self, system_tokens: u32, attachment_tokens: u32) -> u32 {
        system_tokens + attachment_tokens.min(self.attachments) + self.answer + self.margin
    }

    /// A smaller budget for the one retry after a context overflow (history and attachments halved).
    pub fn tighter(&self) -> Self {
        Self {
            history: self.history / 2,
            attachments: self.attachments / 2,
            ..*self
        }
    }
}

/// One past message, as the budget sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryItem {
    pub tokens: u32,
}

/// The rolling summary of older history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SummaryInfo {
    pub tokens: u32,
    /// Index (into the history slice) of the last message the summary covers.
    pub upto: usize,
}

/// What to send.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryPlan {
    /// Indices to include, oldest first.
    pub keep: Vec<usize>,
    /// Indices that did not fit (oldest first).
    pub dropped: Vec<usize>,
    /// Include the stored summary (it covers every dropped message).
    pub include_summary: bool,
    /// Dropped messages exist that no summary covers: enqueue a summary job.
    pub needs_summary: bool,
    /// Tokens of history (plus summary when included).
    pub tokens: u32,
}

/// Choose which history fits. Newest messages first, until either the token budget or `max_turns`
/// (a turn is a user + assistant pair, so `2 * max_turns` messages) is reached.
pub fn plan_history(
    limits: &Limits,
    fixed_tokens: u32,
    items: &[HistoryItem],
    summary: Option<SummaryInfo>,
    max_turns: u32,
) -> HistoryPlan {
    let free = limits.ctx.saturating_sub(fixed_tokens);
    let mut budget = free.min(limits.history);
    // Leave room for a summary if one will be needed or exists.
    let summary_room = summary.map(|s| s.tokens.min(limits.summary)).unwrap_or(0);
    let max_messages = (max_turns as usize).saturating_mul(2).max(1);

    let mut keep_rev = Vec::new();
    let mut used = 0u32;
    for (i, item) in items.iter().enumerate().rev() {
        if keep_rev.len() >= max_messages {
            break;
        }
        // Reserve summary space only once something is going to be dropped; checked below by retry.
        if used + item.tokens > budget {
            break;
        }
        used += item.tokens;
        keep_rev.push(i);
    }
    keep_rev.reverse();
    let first_kept = keep_rev.first().copied().unwrap_or(items.len());
    let mut dropped: Vec<usize> = (0..first_kept).collect();

    let mut include_summary = false;
    let mut needs_summary = false;
    if !dropped.is_empty() {
        let covered = summary.is_some_and(|s| s.upto + 1 >= first_kept);
        if covered {
            // The summary takes room from the history budget: trim the oldest kept messages if needed.
            include_summary = true;
            budget = budget.saturating_sub(summary_room);
            while used > budget && keep_rev.len() > 1 {
                let oldest = keep_rev.remove(0);
                used -= items[oldest].tokens;
                dropped.push(oldest);
            }
            // After trimming, the summary must still cover what was dropped.
            let new_first = keep_rev.first().copied().unwrap_or(items.len());
            if summary.is_some_and(|s| s.upto + 1 < new_first) {
                needs_summary = true;
            }
        } else {
            needs_summary = true;
            // Use a stale summary as a stopgap if there is one; it still tells the model something.
            include_summary = summary.is_some();
            if include_summary {
                budget = budget.saturating_sub(summary_room);
                while used > budget && keep_rev.len() > 1 {
                    let oldest = keep_rev.remove(0);
                    used -= items[oldest].tokens;
                    dropped.push(oldest);
                }
            }
        }
        dropped.sort_unstable();
        dropped.dedup();
    }
    HistoryPlan {
        keep: keep_rev,
        dropped,
        include_summary,
        needs_summary,
        tokens: used + if include_summary { summary_room } else { 0 },
    }
}

/// What the composer's context meter shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextMeter {
    pub used: u32,
    pub max: u32,
}

impl ContextMeter {
    /// Tokens already committed to the next request: system + attachments + history + the new message.
    pub fn new(limits: &Limits, system: u32, attachments: u32, history: u32, new_message: u32) -> Self {
        Self {
            used: system + attachments + history + new_message,
            max: limits.ctx,
        }
    }

    pub fn fraction(&self) -> f32 {
        if self.max == 0 {
            0.0
        } else {
            (self.used as f32 / self.max as f32).min(1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Counting(AtomicU32);
    impl Tokenize for Counting {
        async fn count(&self, text: &str) -> Option<u32> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Some(text.split_whitespace().count() as u32)
        }
    }

    #[tokio::test]
    async fn counts_are_cached() {
        let c = TokenCounter::new(Counting(AtomicU32::new(0)));
        assert_eq!(c.count("a b c").await, 3);
        assert_eq!(c.count("a b c").await, 3);
        assert_eq!(c.count("").await, 0);
        assert_eq!(c.tokenizer.0.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn lru_evicts_least_recently_used() {
        let c = TokenCounter::with_capacity(Counting(AtomicU32::new(0)), 2);
        for t in ["one", "two", "three"] {
            c.count(t).await;
        }
        let before = c.tokenizer.0.load(Ordering::SeqCst);
        c.count("three").await; // cached
        assert_eq!(c.tokenizer.0.load(Ordering::SeqCst), before);
        c.count("one").await; // evicted, counted again
        assert_eq!(c.tokenizer.0.load(Ordering::SeqCst), before + 1);
    }

    #[tokio::test]
    async fn falls_back_to_the_estimate_and_does_not_cache_it() {
        let c = TokenCounter::new(EstimateOnly);
        assert_eq!(c.count("abcdefgh").await, 3); // ceil(8 / 3.2)
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn limits_match_the_contract_at_8192() {
        let l = Limits::for_ctx(8192, false);
        assert_eq!(
            (l.system, l.attachments, l.history, l.summary, l.answer, l.margin),
            (600, 3000, 2400, 300, 1024, 200)
        );
        let t = Limits::for_ctx(8192, true);
        assert_eq!((t.answer, t.attachments), (2048, 3000 - 1024));
        let big = Limits::for_ctx(16384, false);
        assert_eq!((big.history, big.answer), (4800, 2048));
    }

    fn items(n: usize, each: u32) -> Vec<HistoryItem> {
        vec![HistoryItem { tokens: each }; n]
    }

    #[test]
    fn everything_fits() {
        let l = Limits::for_ctx(8192, false);
        let p = plan_history(&l, l.fixed(500, 0), &items(4, 100), None, 8);
        assert_eq!(p.keep, vec![0, 1, 2, 3]);
        assert!(p.dropped.is_empty() && !p.include_summary && !p.needs_summary);
        assert_eq!(p.tokens, 400);
    }

    #[test]
    fn newest_first_until_the_history_budget() {
        let l = Limits::for_ctx(8192, false); // history cap 2400
        let p = plan_history(&l, l.fixed(500, 0), &items(10, 500), None, 50);
        assert_eq!(p.keep, vec![6, 7, 8, 9]);
        assert_eq!(p.dropped, vec![0, 1, 2, 3, 4, 5]);
        assert!(p.needs_summary && !p.include_summary);
    }

    #[test]
    fn history_turns_caps_the_message_count() {
        let l = Limits::for_ctx(8192, false);
        let p = plan_history(&l, l.fixed(100, 0), &items(20, 10), None, 3);
        assert_eq!(p.keep, vec![14, 15, 16, 17, 18, 19]);
        assert_eq!(p.dropped.len(), 14);
    }

    #[test]
    fn a_covering_summary_is_included_and_no_new_one_is_needed() {
        let l = Limits::for_ctx(8192, false);
        let it = items(10, 500);
        let p = plan_history(
            &l,
            l.fixed(500, 0),
            &it,
            Some(SummaryInfo { tokens: 250, upto: 5 }),
            50,
        );
        assert!(p.include_summary && !p.needs_summary);
        assert!(p.tokens <= l.history);
        assert!(p.keep.iter().all(|i| *i >= 6));
    }

    #[test]
    fn a_stale_summary_is_a_stopgap_and_triggers_a_new_one() {
        let l = Limits::for_ctx(8192, false);
        let it = items(10, 500);
        let p = plan_history(
            &l,
            l.fixed(500, 0),
            &it,
            Some(SummaryInfo { tokens: 250, upto: 1 }),
            50,
        );
        assert!(p.include_summary && p.needs_summary);
    }

    #[test]
    fn small_ctx_and_big_fixed_part_shrink_history() {
        let l = Limits::for_ctx(4096, false);
        let fixed = l.fixed(600, 1500);
        let p = plan_history(&l, fixed, &items(20, 100), None, 50);
        let free = l.ctx - fixed;
        assert!(p.tokens <= free.min(l.history));
        assert!(!p.dropped.is_empty());
    }

    #[test]
    fn nothing_fits_keeps_nothing_and_asks_for_a_summary() {
        let l = Limits::for_ctx(8192, false);
        let p = plan_history(&l, l.ctx, &items(3, 100), None, 8);
        assert!(p.keep.is_empty());
        assert_eq!(p.dropped, vec![0, 1, 2]);
        assert!(p.needs_summary);
    }

    #[test]
    fn tighter_halves_history_and_attachments() {
        let l = Limits::for_ctx(8192, false);
        let t = l.tighter();
        assert_eq!((t.history, t.attachments, t.answer), (1200, 1500, 1024));
    }

    #[test]
    fn fifty_turns_never_exceed_the_context() {
        let l = Limits::for_ctx(8192, false);
        let it = items(100, 180);
        let fixed = l.fixed(550, 800);
        let p = plan_history(
            &l,
            fixed,
            &it,
            Some(SummaryInfo {
                tokens: 300,
                upto: 40,
            }),
            8,
        );
        assert!(
            fixed + p.tokens <= l.ctx,
            "fixed {fixed} + history {} > ctx",
            p.tokens
        );
        assert!(p.keep.len() <= 16);
        let meter = ContextMeter::new(&l, 550, 800, p.tokens, 120);
        assert!(meter.used < meter.max && meter.fraction() > 0.0 && meter.fraction() <= 1.0);
    }
}
