use caidex_model_core::{CanonicalRequest, CapabilitySupport, ProviderError, ProviderResult};
use serde_json::Value;
use std::collections::HashMap;

fn invalid() -> ProviderError {
    ProviderError::new(400, "deepseek_invalid_request")
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "deepseek_unsupported_request")
}
fn fields(value: &Value, allowed: &[&str]) -> ProviderResult<()> {
    if value
        .as_object()
        .ok_or_else(invalid)?
        .keys()
        .any(|key| !allowed.contains(&key.as_str()))
    {
        return Err(unsupported());
    }
    Ok(())
}
fn content(value: &Value) -> ProviderResult<()> {
    if value.is_string() {
        return Ok(());
    }
    for part in value.as_array().ok_or_else(invalid)? {
        fields(part, &["type", "text", "annotations", "logprobs"])?;
        if !matches!(part["type"].as_str(), Some("input_text" | "output_text")) {
            return Err(unsupported());
        }
        if !part["text"].is_string() {
            return Err(invalid());
        }
        for key in ["annotations", "logprobs"] {
            if part
                .get(key)
                .is_some_and(|v| v.as_array().is_none_or(|v| !v.is_empty()))
            {
                return Err(unsupported());
            }
        }
    }
    Ok(())
}
/// Runtime explicitly requests the existing whole-reasoning display and bound
/// local carrier. This neither produces concise summaries nor encrypts history.
pub(crate) fn compile_history_controls(
    request: CanonicalRequest,
    enabled: bool,
    reasoning_support: CapabilitySupport,
    max_bytes: usize,
) -> ProviderResult<CanonicalRequest> {
    if !enabled {
        return Ok(request);
    }
    let mut wire = request.wire().clone();
    if wire.to_string().len() > max_bytes {
        return Err(ProviderError::new(413, "invalid_or_oversized_body"));
    }
    if let Some(include) = wire.get("include").filter(|v| !v.is_null()) {
        let values = include.as_array().ok_or_else(invalid)?;
        if !(values.is_empty() || values.len() == 1 && values[0] == "reasoning.encrypted_content") {
            return Err(unsupported());
        }
    }
    wire.as_object_mut().unwrap().remove("include");
    if let Some(reasoning) = wire.get_mut("reasoning") {
        fields(reasoning, &["effort", "summary", "context"])?;
        let controls = reasoning.get("summary").is_some() || reasoning.get("context").is_some();
        if let Some(summary) = reasoning.get("summary").filter(|v| !v.is_null()) {
            if summary != "auto" {
                return Err(unsupported());
            }
            if reasoning_support == CapabilitySupport::Unsupported {
                return Err(ProviderError::new(400, "unsupported_reasoning"));
            }
        }
        if reasoning
            .get("context")
            .is_some_and(|v| !v.is_null() && v != "all_turns")
        {
            return Err(unsupported());
        }
        let object = reasoning.as_object_mut().unwrap();
        object.remove("summary");
        object.remove("context");
        if controls && object.is_empty() {
            wire.as_object_mut().unwrap().remove("reasoning");
        }
    }
    CanonicalRequest::new(wire, request.dialect()).map_err(|_| invalid())
}

/// Apply executor-owned mappings once, before either validation pass. A native
/// level may itself be a source mapped to another level on the next turn.
pub(crate) fn compile_effort(
    request: CanonicalRequest,
    mappings: &HashMap<String, String>,
    max_bytes: usize,
) -> ProviderResult<CanonicalRequest> {
    let Some(reasoning) = request.wire().get("reasoning") else {
        return Ok(request);
    };
    if request.wire().to_string().len() > max_bytes {
        return Err(ProviderError::new(413, "invalid_or_oversized_body"));
    }
    if mappings.is_empty() {
        return Err(unsupported());
    }
    fields(reasoning, &["effort"])?;
    let source = reasoning["effort"].as_str().ok_or_else(invalid)?;
    let native = mappings.get(source).ok_or_else(unsupported)?;
    let mut wire = request.wire().clone();
    wire["reasoning"]["effort"] = native.clone().into();
    CanonicalRequest::new(wire, request.dialect()).map_err(|_| invalid())
}

/// The native API silently ignores unsupported controls and downgrades
/// developer messages. Reject them before authentication, rather than claim
/// an equivalent response from a successful HTTP request.
pub(crate) fn compile(
    request: CanonicalRequest,
    max_bytes: usize,
    runtime_context: bool,
    verbosity_instructions: &HashMap<String, String>,
    native_history: bool,
    native_tools: bool,
    reasoning_controls: bool,
) -> ProviderResult<CanonicalRequest> {
    let mut wire = request.wire().clone();
    if wire.to_string().len() > max_bytes {
        return Err(ProviderError::new(413, "invalid_or_oversized_body"));
    }
    if runtime_context {
        for key in ["client_metadata", "prompt_cache_key"] {
            if let Some(value) = wire.get(key).filter(|value| !value.is_null()) {
                let valid = if key == "client_metadata" {
                    value
                        .as_object()
                        .is_some_and(|map| map.values().all(Value::is_string))
                } else {
                    value.as_str().is_some_and(|value| {
                        !value.trim().is_empty() && !value.chars().any(char::is_control)
                    })
                };
                if !valid {
                    return Err(invalid());
                }
            }
            wire.as_object_mut().unwrap().remove(key);
        }
        if let Some(input) = wire["input"].as_array_mut() {
            let mut conversation = false;
            for item in input {
                if item.get("type").is_none() || item["type"] == "message" {
                    match item["role"].as_str() {
                        Some("developer") if !conversation => item["role"] = "system".into(),
                        Some("developer") => return Err(unsupported()),
                        Some("system") => (),
                        _ => conversation = true,
                    }
                } else {
                    conversation = true;
                }
            }
        }
    }
    if let Some(verbosity) = wire.get("text").and_then(|text| text.get("verbosity")) {
        if verbosity.is_null() && runtime_context {
            wire["text"].as_object_mut().unwrap().remove("verbosity");
        } else {
            let instruction = verbosity_instructions
                .get(verbosity.as_str().ok_or_else(invalid)?)
                .ok_or_else(unsupported)?;
            let original = match wire.get("instructions") {
                None | Some(Value::Null) => "",
                Some(Value::String(value)) => value,
                _ => return Err(invalid()),
            };
            wire["instructions"] = if original.is_empty() {
                instruction.clone()
            } else {
                format!("{original}\n{instruction}")
            }
            .into();
            wire["text"].as_object_mut().unwrap().remove("verbosity");
        }
    }
    fields(
        &wire,
        &[
            "model",
            "input",
            "instructions",
            "stream",
            "store",
            "background",
            "max_output_tokens",
            "text",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
            "reasoning",
        ],
    )?;
    if let Some(reasoning) = wire.get("reasoning") {
        if !reasoning_controls {
            return Err(unsupported());
        }
        fields(reasoning, &["effort"])?;
        if !matches!(
            reasoning["effort"].as_str(),
            Some("none" | "low" | "high" | "max")
        ) {
            return Err(invalid());
        }
    }
    if !native_tools
        && ["tools", "tool_choice", "parallel_tool_calls"]
            .iter()
            .any(|key| wire.get(*key).is_some())
    {
        return Err(unsupported());
    }
    if wire
        .get("instructions")
        .is_some_and(|v| !v.is_null() && !v.is_string())
    {
        return Err(invalid());
    }
    for key in ["store", "background"] {
        if wire.get(key).is_some_and(|v| !v.is_null() && v != false) {
            return Err(unsupported());
        }
    }
    if wire
        .get("max_output_tokens")
        .is_some_and(|v| !v.is_null() && v.as_u64().is_none_or(|n| n == 0))
    {
        return Err(invalid());
    }
    if let Some(text) = wire.get("text").filter(|v| !v.is_null()) {
        fields(text, &["format"])?;
        if let Some(format) = text.get("format") {
            fields(format, &["type"])?;
            if format["type"] != "text" {
                return Err(unsupported());
            }
        }
    }
    if let Some(input) = wire["input"].as_array() {
        for item in input {
            if native_history && item["type"] == "reasoning" {
                // Expansion verifies complete executor-bound native capsules;
                // arbitrary reasoning cannot pass through history::expand.
                continue;
            }
            if native_tools
                && matches!(
                    item["type"].as_str(),
                    Some(
                        "function_call"
                            | "function_call_output"
                            | "custom_tool_call"
                            | "custom_tool_call_output"
                    )
                )
            {
                continue;
            }
            fields(item, &["type", "role", "content", "id", "status"])?;
            if item.get("type").is_some_and(|v| v != "message")
                || !matches!(item["role"].as_str(), Some("user" | "assistant" | "system"))
            {
                return Err(unsupported());
            }
            if item.get("id").is_some_and(|v| {
                v.as_str()
                    .is_none_or(|s| s.trim().is_empty() || s.chars().any(char::is_control))
            }) {
                return Err(invalid());
            }
            if item.get("status").is_some_and(|v| v != "completed") {
                return Err(unsupported());
            }
            content(&item["content"])?;
        }
    }
    wire["store"] = false.into();
    CanonicalRequest::new(wire, request.dialect()).map_err(|_| invalid())
}
