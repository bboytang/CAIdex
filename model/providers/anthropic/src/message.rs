use crate::{invalid_message, string, validate_block, validate_usage};
use caidex_model_core::ProviderResult;
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::HashSet, fmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageOutcome {
    EndTurn,
    StopSequence,
    ToolUse,
    MaxTokens,
    ContextWindowExceeded,
    PauseTurn,
    Refusal,
    Unknown,
}

/// Native content, including signed thinking and future fields, is never
/// rebuilt from display text. Serialization is explicit history storage.
#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct NativeMessage(Value);
impl NativeMessage {
    pub fn parse(wire: Value) -> ProviderResult<Self> {
        validate_start(&wire)?;
        if string(&wire, "stop_reason").is_none() {
            return Err(invalid_message());
        }
        let mut ids = HashSet::new();
        for block in wire["content"].as_array().unwrap() {
            validate_block(block, true)?;
            if matches!(block["type"].as_str(), Some("tool_use" | "server_tool_use"))
                && !ids.insert(block["id"].as_str().unwrap())
            {
                return Err(invalid_message());
            }
        }
        Ok(Self(wire))
    }
    pub fn id(&self) -> &str {
        self.0["id"].as_str().unwrap()
    }
    pub fn model(&self) -> &str {
        self.0["model"].as_str().unwrap()
    }
    pub fn content(&self) -> &[Value] {
        self.0["content"].as_array().unwrap()
    }
    pub fn wire(&self) -> &Value {
        &self.0
    }
    pub fn outcome(&self) -> MessageOutcome {
        if self.0["stop_details"]["type"] == "refusal" {
            return MessageOutcome::Refusal;
        }
        match self.0["stop_reason"].as_str().unwrap() {
            "end_turn" => MessageOutcome::EndTurn,
            "stop_sequence" => MessageOutcome::StopSequence,
            "tool_use" => MessageOutcome::ToolUse,
            "max_tokens" => MessageOutcome::MaxTokens,
            "model_context_window_exceeded" => MessageOutcome::ContextWindowExceeded,
            "pause_turn" => MessageOutcome::PauseTurn,
            "refusal" => MessageOutcome::Refusal,
            _ => MessageOutcome::Unknown,
        }
    }
    /// Native Messages input shape, not a Responses item. All original content
    /// blocks stay in order, including opaque server-tool and thinking blocks.
    pub fn replay_message(&self) -> Value {
        json!({"role":"assistant", "content":self.content()})
    }
}
impl fmt::Debug for NativeMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeMessage([WIRE OMITTED])")
    }
}
pub(crate) fn validate_start(wire: &Value) -> ProviderResult<()> {
    if wire["type"] != "message"
        || wire["role"] != "assistant"
        || string(wire, "id").is_none()
        || string(wire, "model").is_none()
        || !wire["content"].is_array()
        || wire
            .get("stop_reason")
            .is_some_and(|v| !v.is_null() && string(wire, "stop_reason").is_none())
    {
        return Err(invalid_message());
    }
    validate_usage(&wire["usage"])
}
