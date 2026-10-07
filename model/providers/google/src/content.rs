use caidex_model_core::{ProviderError, ProviderResult};
use serde::Serialize;
use serde_json::Value;
use std::{collections::HashSet, fmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateOutcome {
    Stop,
    ToolCall,
    MaxTokens,
    Filtered,
    InvalidToolCall,
    Unknown,
}

/// Complete native JSON, not a display reconstruction or an executable call.
#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct NativeResponse(Value);
impl NativeResponse {
    pub fn parse(wire: Value) -> ProviderResult<Self> {
        let invalid = || ProviderError::new(502, "google_invalid_response");
        if !wire.is_object() || wire.get("error").is_some() {
            return Err(invalid());
        }
        for field in ["responseId", "modelVersion"] {
            if present(&wire, field).is_some_and(|v| nonempty(v).is_none()) {
                return Err(invalid());
            }
        }
        if let Some(usage) = present(&wire, "usageMetadata")
            && (!usage.is_object()
                || [
                    "promptTokenCount",
                    "cachedContentTokenCount",
                    "candidatesTokenCount",
                    "toolUsePromptTokenCount",
                    "thoughtsTokenCount",
                    "totalTokenCount",
                ]
                .iter()
                .any(|key| present(usage, key).is_some_and(|v| v.as_u64().is_none())))
        {
            return Err(invalid());
        }
        let feedback = present(&wire, "promptFeedback");
        if feedback.is_some_and(|v| {
            !v.is_object() || present(v, "blockReason").is_some_and(|v| nonempty(v).is_none())
        }) {
            return Err(invalid());
        }
        let blocked = feedback
            .and_then(|v| nonempty(&v["blockReason"]))
            .filter(|v| *v != "BLOCK_REASON_UNSPECIFIED");
        let candidates = match present(&wire, "candidates") {
            None => &[][..],
            Some(Value::Array(values)) => values.as_slice(),
            _ => return Err(invalid()),
        };
        if candidates.is_empty() != blocked.is_some() {
            return Err(invalid());
        }
        let mut indices = HashSet::new();
        for candidate in candidates {
            let reason = nonempty(&candidate["finishReason"]).ok_or_else(invalid)?;
            if !candidate.is_object() || reason == "FINISH_REASON_UNSPECIFIED" {
                return Err(invalid());
            }
            let index = match present(candidate, "index") {
                None => 0,
                Some(value) => value.as_u64().ok_or_else(invalid)?,
            };
            if !indices.insert(index) {
                return Err(invalid());
            }
            if let Some(content) = present(candidate, "content") {
                validate_content(content, true).map_err(|_| invalid())?;
            }
        }
        Ok(Self(wire))
    }
    pub fn wire(&self) -> &Value {
        &self.0
    }
    pub fn candidates(&self) -> &[Value] {
        self.0["candidates"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    pub fn blocked_prompt(&self) -> Option<&str> {
        nonempty(&self.0["promptFeedback"]["blockReason"])
            .filter(|v| *v != "BLOCK_REASON_UNSPECIFIED")
    }
    /// Position is the returned array position, not the provider candidate index.
    pub fn outcome(&self, position: usize) -> Option<CandidateOutcome> {
        let candidate = self.candidates().get(position)?;
        Some(match candidate["finishReason"].as_str().unwrap() {
            "STOP"
                if candidate["content"]["parts"]
                    .as_array()
                    .is_some_and(|parts| {
                        parts
                            .iter()
                            .any(|part| present(part, "functionCall").is_some())
                    }) =>
            {
                CandidateOutcome::ToolCall
            }
            "STOP" => CandidateOutcome::Stop,
            "MAX_TOKENS" => CandidateOutcome::MaxTokens,
            "SAFETY"
            | "RECITATION"
            | "LANGUAGE"
            | "BLOCKLIST"
            | "PROHIBITED_CONTENT"
            | "SPII"
            | "IMAGE_SAFETY"
            | "IMAGE_PROHIBITED_CONTENT"
            | "IMAGE_RECITATION" => CandidateOutcome::Filtered,
            "MALFORMED_FUNCTION_CALL" | "UNEXPECTED_TOOL_CALL" => CandidateOutcome::InvalidToolCall,
            _ => CandidateOutcome::Unknown,
        })
    }
}
impl fmt::Debug for NativeResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeResponse([WIRE OMITTED])")
    }
}
fn present<'a>(wire: &'a Value, field: &str) -> Option<&'a Value> {
    wire.get(field).filter(|v| !v.is_null())
}
fn nonempty(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|v| !v.trim().is_empty() && !v.chars().any(char::is_control))
}
/// Known shape checks only; unknown native fields/parts remain raw data. No
/// content, media URL, executableCode or server tool is fetched/executed here.
pub(crate) fn validate_content(content: &Value, output: bool) -> ProviderResult<()> {
    let invalid = || ProviderError::new(400, "google_invalid_request");
    if !content.is_object()
        || present(content, "role")
            .is_some_and(|v| !matches!(v.as_str(), Some("" | "model")) && (output || v != "user"))
    {
        return Err(invalid());
    }
    let parts = match present(content, "parts") {
        None if output => &[][..],
        Some(Value::Array(parts)) if output || !parts.is_empty() => parts.as_slice(),
        _ => return Err(invalid()),
    };
    for part in parts {
        if !part.is_object()
            || present(part, "thought").is_some_and(|v| !v.is_boolean())
            || present(part, "thoughtSignature").is_some_and(|v| !v.is_string())
            || present(part, "text").is_some_and(|v| !v.is_string())
        {
            return Err(invalid());
        }
        let objects = [
            "inlineData",
            "functionCall",
            "functionResponse",
            "fileData",
            "executableCode",
            "codeExecutionResult",
            "toolCall",
            "toolResponse",
        ];
        if objects
            .iter()
            .any(|key| present(part, key).is_some_and(|v| !v.is_object()))
            || objects
                .iter()
                .filter(|key| present(part, key).is_some())
                .count()
                + usize::from(present(part, "text").is_some())
                > 1
        {
            return Err(invalid());
        }
        for (key, payload) in [("functionCall", "args"), ("functionResponse", "response")] {
            if let Some(call) = present(part, key)
                && (nonempty(&call["name"]).is_none()
                    || present(call, "id").is_some_and(|v| nonempty(v).is_none())
                    || present(call, payload).is_some_and(|v| !v.is_object()))
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}
