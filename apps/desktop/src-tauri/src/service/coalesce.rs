//! Delta coalescing: the webview cannot render one message per token, so consecutive text deltas of
//! the same kind are merged and sent at most every 16 ms or once they reach 32 characters. Order is
//! always preserved: any other event (or a change between thoughts and answer) flushes first.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use nexus_core::chat::ChatEvent;
use nexus_core::sink::Sink;

pub const FLUSH_AFTER: Duration = Duration::from_millis(16);
pub const FLUSH_CHARS: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Content,
    Reasoning,
}

#[derive(Default)]
struct Pending {
    kind: Option<Kind>,
    text: String,
    /// Bumped on every flush so a stale timer does nothing.
    generation: u64,
}

pub struct CoalescingSink {
    inner: Arc<dyn Sink<ChatEvent>>,
    pending: Arc<Mutex<Pending>>,
    after: Duration,
    chars: usize,
}

fn make(kind: Kind, text: String) -> ChatEvent {
    match kind {
        Kind::Content => ChatEvent::Content { delta: text },
        Kind::Reasoning => ChatEvent::Reasoning { delta: text },
    }
}

fn flush_locked(p: &mut Pending, inner: &dyn Sink<ChatEvent>) -> bool {
    p.generation += 1;
    match p.kind.take() {
        Some(kind) if !p.text.is_empty() => inner.send(make(kind, std::mem::take(&mut p.text))),
        _ => true,
    }
}

impl CoalescingSink {
    pub fn new(inner: Arc<dyn Sink<ChatEvent>>) -> Self {
        Self::with_limits(inner, FLUSH_AFTER, FLUSH_CHARS)
    }

    pub fn with_limits(inner: Arc<dyn Sink<ChatEvent>>, after: Duration, chars: usize) -> Self {
        Self {
            inner,
            pending: Arc::new(Mutex::new(Pending::default())),
            after,
            chars,
        }
    }

    /// Send whatever is buffered. Call once the turn ends.
    pub fn flush(&self) {
        if let Ok(mut p) = self.pending.lock() {
            flush_locked(&mut p, self.inner.as_ref());
        }
    }

    fn push(&self, kind: Kind, delta: String) -> bool {
        let Ok(mut p) = self.pending.lock() else {
            return false;
        };
        let mut ok = true;
        if p.kind.is_some_and(|k| k != kind) {
            ok = flush_locked(&mut p, self.inner.as_ref());
        }
        let first = p.text.is_empty();
        p.kind = Some(kind);
        p.text.push_str(&delta);
        if p.text.chars().count() >= self.chars {
            return flush_locked(&mut p, self.inner.as_ref()) && ok;
        }
        if first {
            let gen_at_start = p.generation;
            let (pending, inner, after) = (self.pending.clone(), self.inner.clone(), self.after);
            tokio::spawn(async move {
                tokio::time::sleep(after).await;
                if let Ok(mut p) = pending.lock()
                    && p.generation == gen_at_start
                {
                    flush_locked(&mut p, inner.as_ref());
                }
            });
        }
        ok
    }
}

impl Sink<ChatEvent> for CoalescingSink {
    fn send(&self, event: ChatEvent) -> bool {
        match event {
            ChatEvent::Content { delta } => self.push(Kind::Content, delta),
            ChatEvent::Reasoning { delta } => self.push(Kind::Reasoning, delta),
            other => {
                let Ok(mut p) = self.pending.lock() else {
                    return false;
                };
                let ok = flush_locked(&mut p, self.inner.as_ref());
                drop(p);
                self.inner.send(other) && ok
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::sink::VecSink;

    fn c(s: &str) -> ChatEvent {
        ChatEvent::Content { delta: s.into() }
    }
    fn r(s: &str) -> ChatEvent {
        ChatEvent::Reasoning { delta: s.into() }
    }

    #[tokio::test]
    async fn small_deltas_are_merged_and_flushed_by_the_timer() {
        let out = VecSink::<ChatEvent>::default();
        let sink = CoalescingSink::with_limits(Arc::new(out.clone()), Duration::from_millis(15), 32);
        for t in ["a", "b", "c"] {
            sink.send(c(t));
        }
        assert!(out.events().is_empty(), "held back");
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(out.events(), vec![c("abc")]);
    }

    #[tokio::test]
    async fn reaching_the_size_limit_flushes_immediately() {
        let out = VecSink::<ChatEvent>::default();
        let sink = CoalescingSink::with_limits(Arc::new(out.clone()), Duration::from_secs(10), 8);
        sink.send(c("1234"));
        sink.send(c("5678"));
        assert_eq!(out.events(), vec![c("12345678")]);
    }

    #[tokio::test]
    async fn other_events_and_kind_changes_flush_first_keeping_order() {
        let out = VecSink::<ChatEvent>::default();
        let sink = CoalescingSink::with_limits(Arc::new(out.clone()), Duration::from_secs(10), 100);
        sink.send(r("think "));
        sink.send(r("more"));
        sink.send(c("ans"));
        sink.send(ChatEvent::Status {
            text: "reading".into(),
        });
        sink.send(c("wer"));
        sink.send(ChatEvent::Done {
            finish_reason: "stop".into(),
            tokens_out: 1,
            tps: 1.0,
            ttft_ms: 1,
        });
        let ev = out.events();
        assert_eq!(
            &ev[..4],
            &[
                r("think more"),
                c("ans"),
                ChatEvent::Status {
                    text: "reading".into()
                },
                c("wer")
            ]
        );
        assert!(matches!(ev[4], ChatEvent::Done { .. }));
        assert_eq!(ev.len(), 5);
    }

    #[tokio::test]
    async fn a_stale_timer_does_not_double_send() {
        let out = VecSink::<ChatEvent>::default();
        let sink = CoalescingSink::with_limits(Arc::new(out.clone()), Duration::from_millis(15), 100);
        sink.send(c("x"));
        sink.flush();
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(out.events(), vec![c("x")]);
    }

    #[tokio::test]
    async fn ordering_is_preserved_for_a_long_mixed_stream() {
        let out = VecSink::<ChatEvent>::default();
        let sink = CoalescingSink::new(Arc::new(out.clone()));
        let (mut want_c, mut want_r) = (String::new(), String::new());
        for i in 0..2000 {
            let piece = format!("{i},");
            if i % 7 < 2 {
                want_r.push_str(&piece);
                sink.send(r(&piece));
            } else {
                want_c.push_str(&piece);
                sink.send(c(&piece));
            }
            if i % 100 == 0 {
                tokio::task::yield_now().await;
            }
        }
        sink.flush();
        let (mut got_c, mut got_r) = (String::new(), String::new());
        for e in out.events() {
            match e {
                ChatEvent::Content { delta } => got_c.push_str(&delta),
                ChatEvent::Reasoning { delta } => got_r.push_str(&delta),
                _ => {}
            }
        }
        assert_eq!((got_c, got_r), (want_c, want_r));
        assert!(out.events().len() < 2000, "events were merged");
    }
}
