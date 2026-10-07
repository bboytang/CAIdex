use crate::{NativeMessage, message::validate_start, string, validate_block, validate_usage};
use caidex_model_core::{ProviderError, ProviderResult, SseDecoder, SseEvent};
use serde_json::Value;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeStreamState {
    Open,
    Completed,
    Failed,
    Cancelled,
    Truncated,
    Invalid,
}

pub struct MessageEvent {
    frame: SseEvent,
    wire: Value,
    stopped_block: Option<Value>,
}
impl MessageEvent {
    pub fn kind(&self) -> &str {
        self.wire["type"].as_str().unwrap()
    }
    pub fn wire(&self) -> &Value {
        &self.wire
    }
    pub(crate) fn stopped_block(&self) -> Option<&Value> {
        self.stopped_block.as_ref()
    }
    pub fn frame(&self) -> &SseEvent {
        &self.frame
    }
}
impl fmt::Debug for MessageEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MessageEvent([WIRE OMITTED])")
    }
}
struct Block {
    wire: Value,
    partial_json: String,
    stopped: bool,
    unknown_delta: bool,
}

/// Native block assembly over the existing SSE framer. EOF/ping/HTTP 200 never
/// synthesize success. Transport cancellation and Redactor remain HTTP duties.
pub struct MessageStream {
    decoder: SseDecoder,
    state: NativeStreamState,
    remaining: usize,
    message: Option<Value>,
    blocks: Vec<Block>,
    message_delta: bool,
    completed: Option<NativeMessage>,
}
fn invalid() -> ProviderError {
    ProviderError::new(502, "anthropic_invalid_stream")
}
impl MessageStream {
    pub fn new(max_frame_bytes: usize, max_stream_bytes: usize) -> ProviderResult<Self> {
        if max_stream_bytes == 0 {
            return Err(ProviderError::new(400, "invalid_stream_limit"));
        }
        Ok(Self {
            decoder: SseDecoder::new(max_frame_bytes)
                .map_err(|_| ProviderError::new(400, "invalid_stream_limit"))?,
            state: NativeStreamState::Open,
            remaining: max_stream_bytes,
            message: None,
            blocks: Vec::new(),
            message_delta: false,
            completed: None,
        })
    }
    pub fn state(&self) -> NativeStreamState {
        self.state
    }
    pub fn completed_message(&self) -> Option<&NativeMessage> {
        self.completed.as_ref()
    }
    pub fn push(&mut self, bytes: &[u8]) -> ProviderResult<Vec<MessageEvent>> {
        if matches!(
            self.state,
            NativeStreamState::Invalid
                | NativeStreamState::Cancelled
                | NativeStreamState::Truncated
        ) {
            return Err(ProviderError::new(502, "anthropic_stream_closed"));
        }
        let result = self.push_inner(bytes);
        if result.is_err() {
            self.state = NativeStreamState::Invalid;
            self.completed = None;
            self.decoder.finish();
        }
        result
    }
    fn push_inner(&mut self, bytes: &[u8]) -> ProviderResult<Vec<MessageEvent>> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or_else(|| ProviderError::new(502, "anthropic_stream_too_large"))?;
        let frames = self.decoder.push(bytes).map_err(|_| invalid())?;
        let mut events = Vec::with_capacity(frames.len());
        for frame in frames {
            if self.state != NativeStreamState::Open {
                return Err(invalid());
            }
            let wire: Value = serde_json::from_str(&frame.data).map_err(|_| invalid())?;
            let kind = string(&wire, "type").ok_or_else(invalid)?;
            if frame.event != "message" && frame.event != kind {
                return Err(invalid());
            }
            match kind {
                "ping" => (),
                "error" => {
                    if !wire["error"].is_object() {
                        return Err(invalid());
                    }
                    self.state = NativeStreamState::Failed;
                }
                "message_start" => {
                    if self.message.is_some() {
                        return Err(invalid());
                    }
                    validate_start(&wire["message"]).map_err(|_| invalid())?;
                    if !wire["message"]["content"].as_array().unwrap().is_empty()
                        || !wire["message"]["stop_reason"].is_null()
                    {
                        return Err(invalid());
                    }
                    self.message = Some(wire["message"].clone());
                }
                "content_block_start" => {
                    if self.message.is_none()
                        || self.message_delta
                        || index(&wire)? != self.blocks.len()
                    {
                        return Err(invalid());
                    }
                    validate_block(&wire["content_block"], false).map_err(|_| invalid())?;
                    self.blocks.push(Block {
                        wire: wire["content_block"].clone(),
                        partial_json: String::new(),
                        stopped: false,
                        unknown_delta: false,
                    });
                }
                "content_block_delta" => {
                    let block = self
                        .blocks
                        .get_mut(index(&wire)?)
                        .filter(|block| !block.stopped)
                        .ok_or_else(invalid)?;
                    update_block(block, &wire["delta"])?;
                }
                "content_block_stop" => {
                    let block = self
                        .blocks
                        .get_mut(index(&wire)?)
                        .filter(|block| !block.stopped)
                        .ok_or_else(invalid)?;
                    if block.unknown_delta {
                        return Err(ProviderError::new(502, "anthropic_unsupported_delta"));
                    }
                    if !block.partial_json.is_empty() {
                        let input: Value =
                            serde_json::from_str(&block.partial_json).map_err(|_| invalid())?;
                        if !input.is_object() {
                            return Err(invalid());
                        }
                        block.wire["input"] = input;
                        block.partial_json.clear();
                    }
                    validate_block(&block.wire, true).map_err(|_| invalid())?;
                    block.stopped = true;
                }
                "message_delta" => {
                    if self.blocks.iter().any(|block| !block.stopped) {
                        return Err(invalid());
                    }
                    let message = self.message.as_mut().ok_or_else(invalid)?;
                    let delta = wire["delta"].as_object().ok_or_else(invalid)?;
                    if delta.keys().any(|key| {
                        ["id", "type", "role", "model", "content", "usage"].contains(&key.as_str())
                    }) {
                        return Err(invalid());
                    }
                    message.as_object_mut().unwrap().extend(delta.clone());
                    if let Some(usage) = wire.get("usage") {
                        merge_usage(&mut message["usage"], usage)?;
                    }
                    self.message_delta = true;
                }
                "message_stop" => {
                    if !self.message_delta || self.blocks.iter().any(|block| !block.stopped) {
                        return Err(invalid());
                    }
                    let mut message = self.message.take().ok_or_else(invalid)?;
                    message["content"] = self
                        .blocks
                        .drain(..)
                        .map(|block| block.wire)
                        .collect::<Vec<_>>()
                        .into();
                    self.completed = Some(NativeMessage::parse(message).map_err(|_| invalid())?);
                    self.state = NativeStreamState::Completed;
                }
                // Unknown event types remain available to the native adapter;
                // they cannot alter identity, block content or completion.
                _ => (),
            }
            let stopped_block = if kind == "content_block_stop" {
                Some(self.blocks[index(&wire)?].wire.clone())
            } else {
                None
            };
            events.push(MessageEvent {
                frame,
                wire,
                stopped_block,
            });
        }
        Ok(events)
    }
    pub fn cancel(&mut self) {
        if self.state == NativeStreamState::Open {
            self.state = NativeStreamState::Cancelled;
            self.decoder.finish();
        }
    }
    pub fn finish(&mut self) -> ProviderResult<NativeStreamState> {
        self.decoder.finish();
        match self.state {
            NativeStreamState::Open => {
                self.state = NativeStreamState::Truncated;
                Err(ProviderError::new(502, "anthropic_stream_truncated"))
            }
            NativeStreamState::Invalid | NativeStreamState::Truncated => {
                Err(ProviderError::new(502, "anthropic_stream_closed"))
            }
            state => Ok(state),
        }
    }
}
fn index(wire: &Value) -> ProviderResult<usize> {
    wire["index"]
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
        .ok_or_else(invalid)
}
fn update_block(block: &mut Block, delta: &Value) -> ProviderResult<()> {
    let kind = string(delta, "type").ok_or_else(invalid)?;
    let block_type = block.wire["type"].as_str().unwrap();
    let (field, value) = match kind {
        "text_delta" if block_type == "text" => ("text", delta["text"].as_str()),
        "thinking_delta" if block_type == "thinking" => ("thinking", delta["thinking"].as_str()),
        "signature_delta" if block_type == "thinking" => ("signature", delta["signature"].as_str()),
        "input_json_delta" if matches!(block_type, "tool_use" | "server_tool_use") => {
            if !block.wire["input"].as_object().unwrap().is_empty() {
                return Err(invalid());
            }
            block
                .partial_json
                .push_str(delta["partial_json"].as_str().ok_or_else(invalid)?);
            return Ok(());
        }
        "citations_delta" if block_type == "text" => {
            let citation = delta
                .get("citation")
                .filter(|value| value.is_object())
                .ok_or_else(invalid)?;
            if block.wire["citations"].is_null() {
                block.wire["citations"] = Vec::<Value>::new().into();
            }
            block.wire["citations"]
                .as_array_mut()
                .ok_or_else(invalid)?
                .push(citation.clone());
            return Ok(());
        }
        "text_delta" | "thinking_delta" | "signature_delta" | "input_json_delta"
        | "citations_delta" => return Err(invalid()),
        _ => {
            block.unknown_delta = true;
            return Ok(());
        }
    };
    let value = value.ok_or_else(invalid)?;
    let mut content = block.wire[field].as_str().ok_or_else(invalid)?.to_owned();
    content.push_str(value);
    block.wire[field] = content.into();
    Ok(())
}
fn merge_usage(current: &mut Value, incoming: &Value) -> ProviderResult<()> {
    validate_usage(incoming).map_err(|_| invalid())?;
    for field in [
        "input_tokens",
        "output_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
    ] {
        if let (Some(previous), Some(next)) = (current[field].as_u64(), incoming[field].as_u64())
            && next < previous
        {
            return Err(invalid());
        }
    }
    current
        .as_object_mut()
        .unwrap()
        .extend(incoming.as_object().unwrap().clone());
    Ok(())
}
