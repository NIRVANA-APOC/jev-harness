//! Hand-written Server-Sent Events decoder.
//!
//! Only what `POST /v1/responses` with `stream: true` actually produces is
//! implemented: a stream of `data:` lines, each carrying one complete JSON
//! event, terminated by a blank line. `event:`, `id:`, `retry:` and comment
//! (`:`) lines are accepted and ignored — the event name is redundant with the
//! `type` field inside the JSON payload, which is what the crate dispatches on.

use crate::error::Error;

/// Incremental SSE decoder.
///
/// Bytes are fed in arbitrary chunk sizes and complete `data:` payloads come
/// back out. Nothing is decoded until a full line (`\n`) has arrived, so a
/// chunk boundary may fall anywhere — inside a line, inside a multi-byte UTF-8
/// character, or between the two newlines of a frame separator.
#[derive(Debug, Default)]
pub struct SseDecoder {
    /// Bytes of lines that are not complete yet.
    buf: Vec<u8>,
    /// `data` lines of the frame currently being assembled, joined by `\n`.
    data: String,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one chunk and returns the payloads of every frame it completed.
    ///
    /// A frame is dispatched by a blank line and must carry at least one
    /// non-empty `data` line; a stream that ends without a trailing blank line
    /// leaves that last frame undelivered, as the SSE specification requires.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, Error> {
        self.buf.extend_from_slice(chunk);

        let Some(last_newline) = self.buf.iter().rposition(|byte| *byte == b'\n') else {
            return Ok(Vec::new());
        };
        let complete: Vec<u8> = self.buf.drain(..=last_newline).collect();
        // Drop the trailing '\n' so that the final (empty) split element is not
        // mistaken for the blank line that dispatches the frame.
        let complete = &complete[..complete.len() - 1];

        let mut payloads = Vec::new();
        for line in complete.split(|byte| *byte == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            self.line(line, &mut payloads)?;
        }
        Ok(payloads)
    }

    fn line(&mut self, line: &[u8], payloads: &mut Vec<String>) -> Result<(), Error> {
        if line.is_empty() {
            if !self.data.is_empty() {
                payloads.push(std::mem::take(&mut self.data));
            }
            return Ok(());
        }

        // A line starting with ':' is a comment and is ignored.
        let text = std::str::from_utf8(line).map_err(|source| Error::Utf8 {
            context: "sse chunk",
            source,
        })?;

        let (field, value) = match text.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (text, ""),
        };

        if field == "data" {
            if !self.data.is_empty() {
                self.data.push('\n');
            }
            self.data.push_str(value);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(decoder: &mut SseDecoder, chunk: &[u8]) -> Vec<String> {
        decoder.push(chunk).expect("valid utf-8")
    }

    #[test]
    fn decodes_one_frame_per_blank_line() {
        let mut decoder = SseDecoder::new();
        let payloads = feed(
            &mut decoder,
            b"event: response.created\ndata: {\"a\":1}\n\ndata: {\"b\":2}\n\n",
        );
        assert_eq!(payloads, vec!["{\"a\":1}", "{\"b\":2}"]);
    }

    #[test]
    fn waits_for_a_full_line() {
        let mut decoder = SseDecoder::new();
        assert!(feed(&mut decoder, b"data: {\"a\"").is_empty());
        assert!(feed(&mut decoder, b":1}\n").is_empty());
        assert_eq!(feed(&mut decoder, b"\n"), vec!["{\"a\":1}"]);
    }

    #[test]
    fn splits_multibyte_characters_across_chunks() {
        let full = "data: {\"delta\":\"你好\"}\n\n".as_bytes();
        // Split in the middle of the first three-byte character.
        let cut = full.iter().position(|b| *b == b'"').unwrap() + 9;
        let mut decoder = SseDecoder::new();
        assert!(feed(&mut decoder, &full[..cut]).is_empty());
        assert_eq!(
            feed(&mut decoder, &full[cut..]),
            vec!["{\"delta\":\"你好\"}"]
        );
    }

    #[test]
    fn accepts_crlf_line_endings() {
        let mut decoder = SseDecoder::new();
        assert_eq!(
            feed(&mut decoder, b"data: {\"a\":1}\r\n\r\n"),
            vec!["{\"a\":1}"]
        );

        // A CRLF pair split across two chunks still terminates one line.
        let mut decoder = SseDecoder::new();
        assert!(feed(&mut decoder, b"data: {\"a\":1}\r").is_empty());
        assert_eq!(feed(&mut decoder, b"\n\r\n"), vec!["{\"a\":1}"]);

        let mut decoder = SseDecoder::new();
        assert_eq!(
            feed(&mut decoder, b"data: {\"a\":1}\n\ndata: {\"b\":2}\n\n"),
            vec!["{\"a\":1}", "{\"b\":2}"]
        );
    }

    #[test]
    fn joins_consecutive_data_lines() {
        let mut decoder = SseDecoder::new();
        assert_eq!(
            feed(&mut decoder, b"data: one\ndata: two\n\n"),
            vec!["one\ntwo"]
        );
    }

    #[test]
    fn ignores_comments_and_other_fields() {
        let mut decoder = SseDecoder::new();
        let payloads = feed(
            &mut decoder,
            b": keep-alive\nid: 7\nretry: 100\ndata: x\n\n",
        );
        assert_eq!(payloads, vec!["x"]);
    }

    #[test]
    fn event_name_alone_dispatches_nothing() {
        let mut decoder = SseDecoder::new();
        assert!(feed(&mut decoder, b"event: response.created\n\n").is_empty());
    }

    #[test]
    fn trailing_frame_without_blank_line_is_not_delivered() {
        let mut decoder = SseDecoder::new();
        assert!(feed(&mut decoder, b"data: {\"a\":1}\n").is_empty());
        assert!(feed(&mut decoder, b"data: not-json").is_empty());
    }

    #[test]
    fn rejects_invalid_utf8() {
        let mut decoder = SseDecoder::new();
        let err = decoder.push(b"data: \xff\n\n").unwrap_err();
        assert!(matches!(err, Error::Utf8 { .. }));
    }
}
