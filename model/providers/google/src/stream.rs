use crate::{
    NativeResponse,
    content::{nonempty, present, validate_response},
};
use caidex_model_core::{ProviderError, ProviderResult, SseDecoder, SseEvent};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeStreamState {
    Open,
    Completed,
    Failed,
    Cancelled,
    Truncated,
    Invalid,
}

pub struct ContentEvent {
    frame: SseEvent,
    wire: Value,
}
impl ContentEvent {
    pub fn frame(&self) -> &SseEvent {
        &self.frame
    }
    pub fn wire(&self) -> &Value {
        &self.wire
    }
}
impl fmt::Debug for ContentEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ContentEvent([WIRE OMITTED])")
    }
}

/// Chunks are the lossless native record. The response is a derived view:
/// ordered Parts are appended unchanged; scalar metadata uses its latest value.
pub struct NativeStreamResponse {
    chunks: Vec<Value>,
    response: NativeResponse,
}
impl NativeStreamResponse {
    pub fn chunks(&self) -> &[Value] {
        &self.chunks
    }
    pub fn response(&self) -> &NativeResponse {
        &self.response
    }
}
impl fmt::Debug for NativeStreamResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeStreamResponse([WIRE OMITTED])")
    }
}

/// Google has no separate message_stop frame. Candidate stops plus a clean
/// transport EOF complete the response; metadata after a stop remains readable.
pub struct ContentStream {
    decoder: SseDecoder,
    state: NativeStreamState,
    remaining: usize,
    expected_candidates: usize,
    chunks: Vec<Value>,
    metadata: Value,
    candidates: BTreeMap<u64, Value>,
    stopped: HashSet<u64>,
    block_reason: Option<String>,
    completed: Option<NativeStreamResponse>,
}
fn invalid() -> ProviderError {
    ProviderError::new(502, "google_invalid_stream")
}
impl ContentStream {
    pub fn new(frame: usize, total: usize, expected_candidates: usize) -> ProviderResult<Self> {
        if total == 0 || expected_candidates == 0 {
            return Err(ProviderError::new(400, "invalid_stream_limit"));
        }
        Ok(Self {
            decoder: SseDecoder::new(frame)
                .map_err(|_| ProviderError::new(400, "invalid_stream_limit"))?,
            state: NativeStreamState::Open,
            remaining: total,
            expected_candidates,
            chunks: Vec::new(),
            metadata: json!({}),
            candidates: BTreeMap::new(),
            stopped: HashSet::new(),
            block_reason: None,
            completed: None,
        })
    }
    pub fn state(&self) -> NativeStreamState {
        self.state
    }
    pub fn completed_response(&self) -> Option<&NativeStreamResponse> {
        self.completed.as_ref()
    }
    pub(crate) fn take_completed_response(&mut self) -> Option<NativeStreamResponse> {
        self.completed.take()
    }
    pub fn push(&mut self, bytes: &[u8]) -> ProviderResult<Vec<ContentEvent>> {
        if self.state != NativeStreamState::Open {
            return Err(ProviderError::new(502, "google_stream_closed"));
        }
        let result = self.push_inner(bytes);
        if result.is_err() {
            if self.state != NativeStreamState::Failed {
                self.state = NativeStreamState::Invalid;
            }
            self.decoder.finish();
            self.chunks.clear();
            self.candidates.clear();
            self.metadata = Value::Null;
        }
        result
    }
    fn push_inner(&mut self, bytes: &[u8]) -> ProviderResult<Vec<ContentEvent>> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or_else(|| ProviderError::new(502, "google_stream_too_large"))?;
        let mut events = Vec::new();
        for frame in self.decoder.push(bytes).map_err(|_| invalid())? {
            let wire: Value = serde_json::from_str(&frame.data).map_err(|_| invalid())?;
            if let Some(error) = wire.get("error") {
                if !error.is_object() {
                    return Err(invalid());
                }
                self.state = NativeStreamState::Failed;
                let (status, code) = match error["code"].as_u64() {
                    Some(401 | 403) => (401, "provider_authentication_failed"),
                    Some(429) => (429, "provider_rate_limited"),
                    Some(400 | 422) => (400, "provider_request_rejected"),
                    Some(500..=599) => (502, "provider_unavailable"),
                    _ => (502, "provider_stream_error"),
                };
                return Err(ProviderError::new(status, code));
            }
            if frame.event != "message" {
                return Err(invalid());
            }
            validate_response(&wire, false).map_err(|_| invalid())?;
            self.append(&wire)?;
            self.chunks.push(wire.clone());
            events.push(ContentEvent { frame, wire });
        }
        Ok(events)
    }
    fn append(&mut self, wire: &Value) -> ProviderResult<()> {
        for name in ["responseId", "modelVersion"] {
            if let Some(next) = present(wire, name)
                && present(&self.metadata, name).is_some_and(|old| old != next)
            {
                return Err(invalid());
            }
        }
        let incoming = wire["candidates"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let reason = nonempty(&wire["promptFeedback"]["blockReason"]);
        if let Some(blocked) = &self.block_reason {
            if !incoming.is_empty() || reason.is_some_and(|r| r != blocked) {
                return Err(invalid());
            }
        } else if let Some(reason) = reason.filter(|r| *r != "BLOCK_REASON_UNSPECIFIED") {
            if !self.candidates.is_empty() {
                return Err(invalid());
            }
            self.block_reason = Some(reason.to_owned());
        }
        for candidate in incoming {
            let index = present(candidate, "index")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if index >= self.expected_candidates as u64 {
                return Err(invalid());
            }
            let parts = candidate["content"]["parts"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let reason =
                nonempty(&candidate["finishReason"]).filter(|r| *r != "FINISH_REASON_UNSPECIFIED");
            let current = self.candidates.entry(index).or_insert_with(|| json!({}));
            if self.stopped.contains(&index)
                && (!parts.is_empty() || reason.is_some_and(|r| current["finishReason"] != r))
            {
                return Err(invalid());
            }
            for (key, value) in candidate.as_object().unwrap() {
                if value.is_null()
                    || key == "content"
                    || (key == "finishReason" && reason.is_none())
                {
                    continue;
                }
                current[key] = value.clone();
            }
            if let Some(content) = present(candidate, "content") {
                if !current["content"].is_object() {
                    current["content"] = json!({"parts":[]});
                }
                for (key, value) in content.as_object().unwrap() {
                    if key != "parts" {
                        current["content"][key] = value.clone();
                    }
                }
                current["content"]["parts"]
                    .as_array_mut()
                    .unwrap()
                    .extend_from_slice(parts);
            }
            if reason.is_some() {
                self.stopped.insert(index);
            }
        }
        for (key, value) in wire.as_object().unwrap() {
            if key == "candidates" || value.is_null() {
                continue;
            }
            if key == "usageMetadata" {
                if !self.metadata[key].is_object() {
                    self.metadata[key] = json!({});
                }
                self.metadata[key]
                    .as_object_mut()
                    .unwrap()
                    .extend(value.as_object().unwrap().clone());
            } else {
                self.metadata[key] = value.clone();
            }
        }
        Ok(())
    }
    pub fn cancel(&mut self) {
        if self.state == NativeStreamState::Open {
            self.state = NativeStreamState::Cancelled;
            self.decoder.finish();
        }
    }
    pub fn finish(&mut self) -> ProviderResult<NativeStreamState> {
        if self.state == NativeStreamState::Completed {
            return Ok(self.state);
        }
        if self.state != NativeStreamState::Open {
            return Err(ProviderError::new(
                if self.state == NativeStreamState::Cancelled {
                    503
                } else {
                    502
                },
                "google_stream_closed",
            ));
        }
        let incomplete = self.decoder.has_pending_frame()
            || self.chunks.is_empty()
            || (self.block_reason.is_none()
                && (self.candidates.len() != self.expected_candidates
                    || self.stopped.len() != self.expected_candidates));
        self.decoder.finish();
        if incomplete {
            self.state = NativeStreamState::Truncated;
            return Err(ProviderError::new(502, "google_stream_truncated"));
        }
        let mut response = self.metadata.clone();
        if let Some(reason) = &self.block_reason {
            response["promptFeedback"]["blockReason"] = reason.clone().into();
        }
        if !self.candidates.is_empty() {
            response["candidates"] = self.candidates.values().cloned().collect::<Vec<_>>().into();
        }
        let response = NativeResponse::parse(response).map_err(|_| {
            self.state = NativeStreamState::Invalid;
            invalid()
        })?;
        self.completed = Some(NativeStreamResponse {
            chunks: std::mem::take(&mut self.chunks),
            response,
        });
        self.state = NativeStreamState::Completed;
        Ok(self.state)
    }
}
