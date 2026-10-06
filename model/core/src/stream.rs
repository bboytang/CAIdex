use crate::{Error, ResponseEvent, Result, SseDecoder, SseEvent};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamState {
    Open,
    Completed,
    Interrupted,
    Incomplete,
    Failed,
    Cancelled,
    Truncated,
    Invalid,
}

#[derive(Clone, Debug)]
pub struct StreamEvent {
    pub frame: SseEvent,
    pub response: ResponseEvent,
}

/// Responses lifecycle validation over SSE. Never reconnects/replays a model
/// POST. Cancellation here closes parsing; the transport must also abort I/O.
pub struct ResponsesStream {
    decoder: SseDecoder,
    state: StreamState,
    last_sequence: Option<u64>,
    response_id: Option<String>,
}
impl ResponsesStream {
    pub fn new(max_frame_bytes: usize) -> Result<Self> {
        Ok(Self {
            decoder: SseDecoder::new(max_frame_bytes)?,
            state: StreamState::Open,
            last_sequence: None,
            response_id: None,
        })
    }
    pub fn state(&self) -> StreamState {
        self.state
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<StreamEvent>> {
        if self.decoder.is_closed()
            || matches!(
                self.state,
                StreamState::Cancelled | StreamState::Truncated | StreamState::Invalid
            )
        {
            return Err(Error::StreamClosed);
        }
        let result = self.push_inner(bytes);
        if result.is_err() {
            self.state = StreamState::Invalid;
            self.decoder.finish();
        }
        result
    }
    fn push_inner(&mut self, bytes: &[u8]) -> Result<Vec<StreamEvent>> {
        let frames = self.decoder.push(bytes)?;
        let mut events = Vec::with_capacity(frames.len());
        for frame in frames {
            if self.state != StreamState::Open {
                return Err(Error::StreamClosed);
            }
            let wire: Value = serde_json::from_str(&frame.data).map_err(|_| Error::InvalidEvent)?;
            let response = ResponseEvent::new(wire)?;
            if frame.event != "message" && frame.event != response.kind() {
                return Err(Error::InvalidEvent);
            }
            let wire = response.wire();
            if let Some(sequence) = wire.get("sequence_number") {
                let sequence = sequence.as_u64().ok_or(Error::InvalidSequence)?;
                if self
                    .last_sequence
                    .is_some_and(|previous| sequence <= previous)
                {
                    return Err(Error::InvalidSequence);
                }
                self.last_sequence = Some(sequence);
            }
            if let Some(id) = wire.get("response").and_then(|value| value.get("id")) {
                let id = id
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or(Error::InvalidEvent)?;
                if self
                    .response_id
                    .as_deref()
                    .is_some_and(|previous| previous != id)
                {
                    return Err(Error::ResponseMismatch);
                }
                self.response_id = Some(id.into());
            }
            if let Some(terminal) = response.terminal() {
                self.state = terminal;
            }
            events.push(StreamEvent { frame, response });
        }
        Ok(events)
    }
    pub fn cancel(&mut self) {
        if self.state == StreamState::Open {
            self.state = StreamState::Cancelled;
            self.decoder.finish();
        }
    }
    pub fn finish(&mut self) -> Result<StreamState> {
        self.decoder.finish();
        if self.state == StreamState::Open {
            self.state = StreamState::Truncated;
            return Err(Error::UnexpectedEnd);
        }
        Ok(self.state)
    }
}
