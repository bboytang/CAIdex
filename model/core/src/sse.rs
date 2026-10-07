use crate::{Error, Result};
use std::{fmt, mem};

#[derive(Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
    pub id: String,
    pub retry_ms: Option<u64>,
}
impl fmt::Debug for SseEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SseEvent([WIRE OMITTED])")
    }
}

/// Incremental SSE framing with LF, CRLF, CR, BOM, comments and multiline data.
/// Invalid UTF-8 fails closed instead of replacing opaque provider bytes. Only
/// blank-line-terminated events dispatch. Limits apply to each frame, counting
/// a paired CRLF as one line terminator.
pub struct SseDecoder {
    line: Vec<u8>,
    data: String,
    event: String,
    id: String,
    retry_ms: Option<u64>,
    skip_lf: bool,
    first_line: bool,
    frame_bytes: usize,
    max_frame_bytes: usize,
    closed: bool,
}
impl SseDecoder {
    pub fn new(max_frame_bytes: usize) -> Result<Self> {
        if max_frame_bytes == 0 {
            return Err(Error::InvalidLimit);
        }
        Ok(Self {
            line: Vec::new(),
            data: String::new(),
            event: String::new(),
            id: String::new(),
            retry_ms: None,
            skip_lf: false,
            first_line: true,
            frame_bytes: 0,
            max_frame_bytes,
            closed: false,
        })
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<SseEvent>> {
        if self.closed {
            return Err(Error::StreamClosed);
        }
        let result = self.push_inner(bytes);
        if result.is_err() {
            self.finish();
        }
        result
    }
    pub(crate) fn is_closed(&self) -> bool {
        self.closed
    }
    /// A protocol requiring a complete tail can check before finish discards it.
    pub fn has_pending_frame(&self) -> bool {
        !self.line.is_empty() || !self.data.is_empty() || !self.event.is_empty()
    }
    fn push_inner(&mut self, bytes: &[u8]) -> Result<Vec<SseEvent>> {
        let mut events = Vec::new();
        for &byte in bytes {
            if self.skip_lf {
                self.skip_lf = false;
                if byte == b'\n' {
                    continue;
                }
            }
            if self.frame_bytes == self.max_frame_bytes {
                return Err(Error::FrameTooLarge);
            }
            self.frame_bytes += 1;
            match byte {
                b'\r' | b'\n' => {
                    self.skip_lf = byte == b'\r';
                    if let Some(event) = self.complete_line()? {
                        events.push(event);
                    }
                }
                _ => self.line.push(byte),
            }
        }
        Ok(events)
    }
    fn complete_line(&mut self) -> Result<Option<SseEvent>> {
        let line = mem::take(&mut self.line);
        let mut text = std::str::from_utf8(&line).map_err(|_| Error::InvalidUtf8)?;
        if self.first_line {
            self.first_line = false;
            text = text.strip_prefix('\u{feff}').unwrap_or(text);
        }
        if text.is_empty() {
            self.frame_bytes = 0;
            let event = mem::take(&mut self.event);
            if self.data.is_empty() {
                return Ok(None);
            }
            self.data.pop(); // data fields always append exactly one LF.
            return Ok(Some(SseEvent {
                event: if event.is_empty() {
                    "message".into()
                } else {
                    event
                },
                data: mem::take(&mut self.data),
                id: self.id.clone(),
                retry_ms: self.retry_ms,
            }));
        }
        if text.starts_with(':') {
            return Ok(None);
        }
        let (field, value) = text.split_once(':').unwrap_or((text, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "data" => {
                self.data.push_str(value);
                self.data.push('\n');
            }
            "event" => {
                self.event = value.into();
            }
            "id" if !value.contains('\0') => {
                self.id = value.into();
            }
            "retry" if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
                if let Ok(value) = value.parse() {
                    self.retry_ms = Some(value);
                }
            }
            _ => (),
        }
        Ok(None)
    }
    /// EOF discards an unfinished frame; it never synthesizes an event.
    pub fn finish(&mut self) {
        self.closed = true;
        self.line.clear();
        self.data.clear();
        self.event.clear();
    }
}
