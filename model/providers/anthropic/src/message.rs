use crate::{invalid_message, string, validate_block, validate_usage};
use caidex_model_core::{ProviderError, ProviderResult};
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
        let mut fallback_model = None;
        for block in wire["content"].as_array().unwrap() {
            validate_block(block, true)?;
            if block["type"] == "fallback" {
                if fallback_model.is_some_and(|model| block["from"]["model"] != model) {
                    return Err(invalid_message());
                }
                fallback_model = block["to"]["model"].as_str();
            }
            if matches!(block["type"].as_str(), Some("tool_use" | "server_tool_use"))
                && !ids.insert(block["id"].as_str().unwrap())
            {
                return Err(invalid_message());
            }
        }
        if fallback_model.is_some_and(|model| wire["model"] != model) {
            return Err(invalid_message());
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
    /// Native binding reports, including future entries, remain history data.
    /// Missing reports do not prove that the provider preserved input thinking.
    pub fn input_transformations(&self) -> Option<&[Value]> {
        self.0["input_transformations"]
            .as_array()
            .map(Vec::as_slice)
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
    pub(crate) fn last_fallback_index(&self) -> Option<usize> {
        self.content()
            .iter()
            .rposition(|block| block["type"] == "fallback")
    }
    /// Native Messages input shape. Preserve full wire separately; the native
    /// fallback echo contract excludes the declining hops' thinking/client calls
    /// and unpaired server calls. Boundaries and all serving-hop blocks stay put.
    pub fn replay_message(&self) -> Value {
        let Some(boundary) = self.last_fallback_index() else {
            return json!({"role":"assistant", "content":self.content()});
        };
        let server_results: HashSet<_> = self
            .content()
            .iter()
            .filter(|block| {
                block["type"]
                    .as_str()
                    .is_some_and(|kind| kind.ends_with("_tool_result"))
            })
            .filter_map(|block| block["tool_use_id"].as_str())
            .collect();
        let content: Vec<_> = self
            .content()
            .iter()
            .enumerate()
            .filter_map(|(index, block)| {
                let keep = index >= boundary
                    || match block["type"].as_str() {
                        Some("thinking" | "redacted_thinking" | "connector_text" | "tool_use") => {
                            false
                        }
                        Some("server_tool_use") => {
                            server_results.contains(block["id"].as_str().unwrap())
                        }
                        _ => true,
                    };
                keep.then_some(block)
            })
            .collect();
        json!({"role":"assistant", "content":content})
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
    validate_usage(&wire["usage"])?;
    validate_input_transformations(wire)
}

pub(crate) fn validate_input_transformations(wire: &Value) -> ProviderResult<()> {
    let Some(value) = wire.get("input_transformations").filter(|v| !v.is_null()) else {
        return Ok(());
    };
    for entry in value.as_array().ok_or_else(invalid_message)? {
        let kind = string(entry, "type").ok_or_else(invalid_message)?;
        if matches!(kind, "thinking_dropped" | "thinking_mismatch_allowed")
            && (string(entry, "path").is_none() || string(entry, "reason").is_none())
        {
            return Err(invalid_message());
        }
    }
    Ok(())
}

pub(crate) fn require_binding_report(wire: &Value) -> ProviderResult<()> {
    validate_input_transformations(wire)?;
    if !wire["input_transformations"].is_array() {
        return Err(ProviderError::new(502, "anthropic_binding_report_missing"));
    }
    Ok(())
}

/// A successful native generation may still report dropped or unbound input.
/// The Responses adapter must not release executable done/history as if that
/// input had been preserved. Raw native APIs retain the reports for the caller.
pub(crate) fn check_input_bindings(wire: &Value) -> ProviderResult<()> {
    validate_input_transformations(wire)?;
    for entry in wire["input_transformations"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let code = match (entry["type"].as_str(), entry["reason"].as_str()) {
            (
                Some("thinking_dropped"),
                Some(
                    "prefix_binding_mismatch"
                    | "model_binding_mismatch"
                    | "organization_binding_mismatch"
                    | "end_user_binding_mismatch",
                ),
            ) => "anthropic_input_thinking_dropped",
            (Some("thinking_mismatch_allowed"), Some("prefix_binding_mismatch")) => {
                "anthropic_input_binding_mismatch"
            }
            // The native contract permits future types/reasons. Preserve them;
            // do not invent their semantics or infer compatibility evidence.
            _ => continue,
        };
        return Err(ProviderError::new(502, code));
    }
    Ok(())
}
