//! Minimal, allocation-conscious Server-Sent Events codec.
//!
//! The decoder is fed arbitrary byte chunks (as they arrive from the network)
//! and yields complete events; it never buffers more than one partial line
//! plus the event under construction.

use bytes::Bytes;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseEvent {
    /// The `event:` field, if present.
    pub event: Option<String>,
    /// All `data:` lines joined with `\n`.
    pub data: String,
    pub id: Option<String>,
}

/// Incremental decoder. Enforces a maximum line length so a hostile upstream
/// cannot make the proxy buffer unbounded data.
#[derive(Debug)]
pub struct SseDecoder {
    buf: Vec<u8>,
    current: SseEvent,
    has_data: bool,
    max_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseError {
    LineTooLong(usize),
    InvalidUtf8,
}

impl std::fmt::Display for SseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SseError::LineTooLong(max) => write!(f, "SSE line exceeds {max} bytes"),
            SseError::InvalidUtf8 => f.write_str("SSE stream is not valid UTF-8"),
        }
    }
}

impl std::error::Error for SseError {}

impl Default for SseDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl SseDecoder {
    pub const DEFAULT_MAX_LINE: usize = 16 * 1024 * 1024;

    pub fn new() -> Self {
        Self::with_max_line(Self::DEFAULT_MAX_LINE)
    }

    pub fn with_max_line(max_line: usize) -> Self {
        Self { buf: Vec::new(), current: SseEvent::default(), has_data: false, max_line }
    }

    /// Feeds a chunk and returns every event completed by it.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, SseError> {
        let mut out = Vec::new();
        self.buf.extend_from_slice(chunk);
        let mut start = 0;
        while let Some(pos) = self.buf[start..].iter().position(|&b| b == b'\n') {
            let end = start + pos;
            let mut line = &self.buf[start..end];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            let line = std::str::from_utf8(line).map_err(|_| SseError::InvalidUtf8)?.to_string();
            start = end + 1;
            if let Some(ev) = self.process_line(&line) {
                out.push(ev);
            }
        }
        self.buf.drain(..start);
        if self.buf.len() > self.max_line {
            return Err(SseError::LineTooLong(self.max_line));
        }
        Ok(out)
    }

    /// Flushes a trailing event that was not terminated by a blank line.
    pub fn finish(&mut self) -> Result<Option<SseEvent>, SseError> {
        if !self.buf.is_empty() {
            let rest = std::mem::take(&mut self.buf);
            let line = std::str::from_utf8(&rest).map_err(|_| SseError::InvalidUtf8)?.to_string();
            let line = line.trim_end_matches('\r').to_string();
            if let Some(ev) = self.process_line(&line) {
                return Ok(Some(ev));
            }
        }
        Ok(self.dispatch())
    }

    fn process_line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => self.current.event = Some(value.to_string()),
            "data" => {
                if self.has_data {
                    self.current.data.push('\n');
                }
                self.current.data.push_str(value);
                self.has_data = true;
            }
            "id" => self.current.id = Some(value.to_string()),
            _ => {}
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        if !self.has_data && self.current.event.is_none() {
            self.current = SseEvent::default();
            return None;
        }
        self.has_data = false;
        Some(std::mem::take(&mut self.current))
    }
}

/// Encodes one event. `data` must not contain bare `\r`; embedded newlines are split into
/// multiple `data:` lines per the spec.
pub fn encode(event: Option<&str>, data: &str) -> Bytes {
    let mut out = String::with_capacity(data.len() + 32);
    if let Some(name) = event {
        out.push_str("event: ");
        out.push_str(name);
        out.push('\n');
    }
    for line in data.split('\n') {
        out.push_str("data: ");
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    Bytes::from(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_across_chunk_boundaries() {
        let mut d = SseDecoder::new();
        assert!(d.feed(b"event: a\nda").unwrap().is_empty());
        let evs = d.feed(b"ta: {\"x\":1}\n\ndata: two\r\n\r\n").unwrap();
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].event.as_deref(), Some("a"));
        assert_eq!(evs[0].data, "{\"x\":1}");
        assert_eq!(evs[1].event, None);
        assert_eq!(evs[1].data, "two");
    }

    #[test]
    fn joins_multiline_data_and_skips_comments() {
        let mut d = SseDecoder::new();
        let evs = d.feed(b": keepalive\ndata: a\ndata: b\n\n").unwrap();
        assert_eq!(evs, vec![SseEvent { event: None, data: "a\nb".into(), id: None }]);
    }

    #[test]
    fn finish_flushes_unterminated_event() {
        let mut d = SseDecoder::new();
        assert!(d.feed(b"data: [DONE]").unwrap().is_empty());
        assert_eq!(d.finish().unwrap().unwrap().data, "[DONE]");
    }

    #[test]
    fn rejects_oversized_line() {
        let mut d = SseDecoder::with_max_line(8);
        assert_eq!(d.feed(b"data: 0123456789").unwrap_err(), SseError::LineTooLong(8));
    }

    #[test]
    fn encode_splits_newlines() {
        assert_eq!(&encode(Some("x"), "a\nb")[..], b"event: x\ndata: a\ndata: b\n\n");
    }
}
