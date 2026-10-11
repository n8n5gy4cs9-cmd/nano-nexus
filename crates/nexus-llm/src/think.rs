//! Fallback `<think>…</think>` splitter (INFERENCE-CONTRACT §4.2).
//!
//! If the chat template ignored `enable_thinking` and the server did not extract the thoughts, they
//! arrive inside `content`. This state machine routes that span to [`Segment::Reasoning`], holding
//! back any tail that could be the start of a tag split across deltas.

const OPEN: &str = "<think>";
const CLOSE: &str = "</think>";

/// A routed piece of text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Content(String),
    Reasoning(String),
}

/// Streaming splitter; feed deltas with [`ThinkSplitter::push`], then call [`ThinkSplitter::finish`].
#[derive(Debug, Default)]
pub struct ThinkSplitter {
    in_think: bool,
    held: String,
}

impl ThinkSplitter {
    pub fn new() -> Self {
        Self::default()
    }

    fn emit(&self, text: &str, out: &mut Vec<Segment>) {
        if text.is_empty() {
            return;
        }
        let seg = if self.in_think {
            Segment::Reasoning(text.to_owned())
        } else {
            Segment::Content(text.to_owned())
        };
        match (out.last_mut(), &seg) {
            (Some(Segment::Content(a)), Segment::Content(b))
            | (Some(Segment::Reasoning(a)), Segment::Reasoning(b)) => a.push_str(b),
            _ => out.push(seg),
        }
    }

    /// Feed one content delta.
    pub fn push(&mut self, delta: &str) -> Vec<Segment> {
        self.held.push_str(delta);
        let mut out = Vec::new();
        loop {
            let tag = if self.in_think { CLOSE } else { OPEN };
            if let Some(pos) = self.held.find(tag) {
                let before: String = self.held[..pos].to_owned();
                self.emit(&before, &mut out);
                self.held.drain(..pos + tag.len());
                self.in_think = !self.in_think;
                continue;
            }
            // Hold back the longest tail that is a proper prefix of the tag.
            let keep = (1..tag.len())
                .rev()
                .find(|&n| self.held.len() >= n && self.held.ends_with(&tag[..n]))
                .unwrap_or(0);
            let split = self.held.len() - keep;
            let ready: String = self.held[..split].to_owned();
            self.emit(&ready, &mut out);
            self.held.drain(..split);
            return out;
        }
    }

    /// End of stream: release whatever was held back (an unfinished tag is plain text).
    pub fn finish(&mut self) -> Vec<Segment> {
        let mut out = Vec::new();
        let rest = std::mem::take(&mut self.held);
        self.emit(&rest, &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(chunks: &[&str]) -> Vec<Segment> {
        let mut s = ThinkSplitter::new();
        let mut out = Vec::new();
        for c in chunks {
            out.extend(s.push(c));
        }
        out.extend(s.finish());
        merge(out)
    }

    fn merge(segs: Vec<Segment>) -> Vec<Segment> {
        let mut out: Vec<Segment> = Vec::new();
        for s in segs {
            match (out.last_mut(), &s) {
                (Some(Segment::Content(a)), Segment::Content(b))
                | (Some(Segment::Reasoning(a)), Segment::Reasoning(b)) => a.push_str(b),
                _ => out.push(s),
            }
        }
        out
    }

    fn c(s: &str) -> Segment {
        Segment::Content(s.into())
    }
    fn r(s: &str) -> Segment {
        Segment::Reasoning(s.into())
    }

    #[test]
    fn plain_content_passes_through() {
        assert_eq!(run(&["hello ", "world"]), vec![c("hello world")]);
    }

    #[test]
    fn think_span_is_routed_to_reasoning() {
        assert_eq!(run(&["<think>why</think>answer"]), vec![r("why"), c("answer")]);
    }

    #[test]
    fn split_tags_across_deltas() {
        assert_eq!(
            run(&["<thi", "nk>hm", "m</thi", "nk>ok"]),
            vec![r("hmm"), c("ok")]
        );
    }

    #[test]
    fn every_split_position_of_a_sample() {
        let text = "pre<think>deep thought</think>post <b>bold</b> a<b";
        let expected = run(&[text]);
        assert_eq!(
            expected,
            vec![c("pre"), r("deep thought"), c("post <b>bold</b> a<b")]
        );
        for i in 0..=text.len() {
            if !text.is_char_boundary(i) {
                continue;
            }
            assert_eq!(run(&[&text[..i], &text[i..]]), expected, "split at {i}");
        }
    }

    #[test]
    fn char_at_a_time_with_multibyte() {
        let text = "é<think>✓✓</think>ü";
        let chunks: Vec<String> = text.chars().map(String::from).collect();
        let refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
        assert_eq!(run(&refs), vec![c("é"), r("✓✓"), c("ü")]);
    }

    #[test]
    fn unfinished_tag_at_end_is_text() {
        assert_eq!(run(&["a <thi"]), vec![c("a <thi")]);
    }

    #[test]
    fn unterminated_think_stays_reasoning() {
        assert_eq!(run(&["<think>still going"]), vec![r("still going")]);
    }
}
