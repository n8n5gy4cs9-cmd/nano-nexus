//! Incremental Server-Sent-Events decoder (INFERENCE-CONTRACT §4.2).
//!
//! Bytes go in, complete `data:` payloads come out. Lines are split on `\n` (a trailing `\r` is
//! stripped) and UTF-8 is decoded only on complete lines, so multi-byte characters split across
//! network chunks survive.

/// One decoded SSE event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SseItem {
    /// The joined `data:` lines of one event.
    Data(String),
    /// The `[DONE]` sentinel.
    Done,
}

/// Stateful decoder; feed it chunks with [`SseDecoder::push`].
#[derive(Debug, Default)]
pub struct SseDecoder {
    buf: Vec<u8>,
    data: Vec<String>,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes; returns every event completed by them.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseItem> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
            line.pop(); // '\n'
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            self.line(&String::from_utf8_lossy(&line), &mut out);
        }
        out
    }

    /// Flush at end of stream: an event without a trailing blank line is still delivered.
    pub fn finish(&mut self) -> Vec<SseItem> {
        let mut out = Vec::new();
        if !self.buf.is_empty() {
            let rest = std::mem::take(&mut self.buf);
            let line = String::from_utf8_lossy(&rest).trim_end_matches('\r').to_owned();
            self.line(&line, &mut out);
        }
        self.dispatch(&mut out);
        out
    }

    fn line(&mut self, line: &str, out: &mut Vec<SseItem>) {
        if line.is_empty() {
            self.dispatch(out);
        } else if let Some(rest) = line.strip_prefix("data:") {
            self.data.push(rest.strip_prefix(' ').unwrap_or(rest).to_owned());
        }
        // Comments (":") and other fields (event:, id:, retry:) are ignored.
    }

    fn dispatch(&mut self, out: &mut Vec<SseItem>) {
        if self.data.is_empty() {
            return;
        }
        let payload = self.data.drain(..).collect::<Vec<_>>().join("\n");
        if payload.trim() == "[DONE]" {
            out.push(SseItem::Done);
        } else {
            out.push(SseItem::Data(payload));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(s: &str) -> SseItem {
        SseItem::Data(s.to_owned())
    }

    #[test]
    fn basic_events_and_done() {
        let mut d = SseDecoder::new();
        let out = d.push(b"data: {\"a\":1}\n\ndata: {\"b\":2}\n\ndata: [DONE]\n\n");
        assert_eq!(out, vec![data("{\"a\":1}"), data("{\"b\":2}"), SseItem::Done]);
    }

    #[test]
    fn crlf_line_endings() {
        let mut d = SseDecoder::new();
        assert_eq!(
            d.push(b"data: x\r\n\r\ndata: [DONE]\r\n\r\n"),
            vec![data("x"), SseItem::Done]
        );
    }

    #[test]
    fn partial_lines_across_chunks() {
        let mut d = SseDecoder::new();
        assert!(d.push(b"da").is_empty());
        assert!(d.push(b"ta: {\"k\"").is_empty());
        assert!(d.push(b":1}\n").is_empty());
        assert_eq!(d.push(b"\n"), vec![data("{\"k\":1}")]);
    }

    #[test]
    fn every_split_position_gives_same_result() {
        let input = "data: {\"t\":\"héllo ✓\"}\r\n\r\ndata: two\n\ndata: [DONE]\n\n".as_bytes();
        let expected = {
            let mut d = SseDecoder::new();
            d.push(input)
        };
        assert_eq!(expected.len(), 3);
        for i in 0..=input.len() {
            let mut d = SseDecoder::new();
            let mut got = d.push(&input[..i]);
            got.extend(d.push(&input[i..]));
            assert_eq!(got, expected, "split at {i}");
        }
    }

    #[test]
    fn byte_at_a_time() {
        let input = "data: ✓✓✓\n\ndata: [DONE]\n\n".as_bytes();
        let mut d = SseDecoder::new();
        let got: Vec<_> = input.iter().flat_map(|b| d.push(&[*b])).collect();
        assert_eq!(got, vec![data("✓✓✓"), SseItem::Done]);
    }

    #[test]
    fn multi_line_data_is_joined_and_comments_ignored() {
        let mut d = SseDecoder::new();
        let out = d.push(b": keep-alive\nevent: message\ndata: a\ndata: b\n\n");
        assert_eq!(out, vec![data("a\nb")]);
    }

    #[test]
    fn data_without_space_after_colon() {
        let mut d = SseDecoder::new();
        assert_eq!(d.push(b"data:x\n\n"), vec![data("x")]);
    }

    #[test]
    fn finish_flushes_unterminated_event() {
        let mut d = SseDecoder::new();
        assert!(d.push(b"data: tail").is_empty());
        assert_eq!(d.finish(), vec![data("tail")]);
        assert!(d.finish().is_empty());
    }
}
