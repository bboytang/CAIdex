use caidex_model_core::{
    CanonicalRequest, CapabilitySupport, ProviderError, ProviderResult, ResponsesDialect,
};
use serde_json::Value;
use std::collections::HashMap;

pub(crate) fn valid_effort(effort: &str) -> bool {
    matches!(
        effort,
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
    )
}

fn invalid() -> ProviderError {
    ProviderError::new(400, "qwen_invalid_request")
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "qwen_unsupported_request")
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
/// Source has already been bounded; consume only the explicit Lite tool prefix.
pub(crate) fn classic(
    request: CanonicalRequest,
    enabled: bool,
) -> ProviderResult<(CanonicalRequest, Option<Value>)> {
    if request.dialect() == ResponsesDialect::Classic {
        return Ok((request, None));
    }
    if !enabled {
        return Err(ProviderError::new(400, "unsupported_dialect"));
    }
    let mut wire = request.wire().clone();
    let single = match wire.get("parallel_tool_calls") {
        None | Some(Value::Bool(true)) => false,
        Some(Value::Bool(false)) => true,
        _ => return Err(invalid()),
    };
    wire.as_object_mut().unwrap().remove("parallel_tool_calls");
    let mut policy = serde_json::json!({"lite_single_tool_call":single});
    let mut declarations = Vec::new();
    let input = wire["input"].as_array_mut().ok_or_else(invalid)?;
    for (index, item) in input.iter().enumerate() {
        if item["type"] != "additional_tools" {
            continue;
        }
        fields(item, &["type", "id", "role", "tools"])?;
        if index != 0
            || item["role"] != "developer"
            || item.get("id").is_some_and(|v| !crate::history::valid_id(v))
        {
            return Err(invalid());
        }
        if let Some(id) = item.get("id") {
            policy["additional_tools_id"] = id.clone();
        }
        declarations = item["tools"].as_array().ok_or_else(invalid)?.clone();
    }
    if input
        .first()
        .is_some_and(|item| item["type"] == "additional_tools")
    {
        input.remove(0);
    }
    wire["tools"] = declarations.into();
    Ok((
        CanonicalRequest::new(wire, ResponsesDialect::Classic).map_err(|_| invalid())?,
        Some(policy),
    ))
}
/// Native Responses ignores unknown controls. Bound and validate the source
/// before credentials; never silently drop safety, tools or history fields.
pub(crate) fn compile(
    request: CanonicalRequest,
    max_bytes: usize,
    runtime_context: bool,
    verbosity_instructions: &HashMap<String, String>,
    reasoning_efforts: Option<&HashMap<String, String>>,
    reasoning_support: CapabilitySupport,
    native_history: bool,
) -> ProviderResult<CanonicalRequest> {
    if request.dialect() != ResponsesDialect::Classic {
        return Err(ProviderError::new(400, "unsupported_dialect"));
    }
    let mut wire = request.wire().clone();
    if wire.to_string().len() > max_bytes {
        return Err(ProviderError::new(413, "invalid_or_oversized_body"));
    }
    if runtime_context {
        for key in ["client_metadata", "prompt_cache_key"] {
            if let Some(value) = wire.get(key).filter(|v| !v.is_null()) {
                let valid = if key == "client_metadata" {
                    value
                        .as_object()
                        .is_some_and(|m| m.values().all(Value::is_string))
                } else {
                    value
                        .as_str()
                        .is_some_and(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
                };
                if !valid {
                    return Err(invalid());
                }
            }
            wire.as_object_mut().unwrap().remove(key);
        }
    }
    if let Some(text) = wire.get("text") {
        if !runtime_context && verbosity_instructions.is_empty() {
            return Err(unsupported());
        }
        if !text.is_null() {
            fields(text, &["format", "verbosity"])?;
            if let Some(format) = text.get("format") {
                if !runtime_context {
                    return Err(unsupported());
                }
                fields(format, &["type"])?;
                if format["type"] != "text" {
                    return Err(unsupported());
                }
            }
            if let Some(verbosity) = text.get("verbosity")
                && !(verbosity.is_null() && runtime_context)
            {
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
            }
        } else if !runtime_context {
            return Err(unsupported());
        }
        // Qwen does not process text.format/verbosity; neutral text and explicitly
        // configured guidance are local semantics, not native structured output.
        wire.as_object_mut().unwrap().remove("text");
    }
    // Both policies explicitly select full native summaries and the bound local
    // carrier, not native encryption or concise-summary generation.
    if runtime_context && native_history {
        if let Some(include) = wire.get("include").filter(|v| !v.is_null()) {
            let values = include.as_array().ok_or_else(invalid)?;
            if !(values.is_empty()
                || values.len() == 1 && values[0] == "reasoning.encrypted_content")
            {
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
    }
    fields(
        &wire,
        &[
            "model",
            "input",
            "stream",
            "instructions",
            "store",
            "background",
            "max_output_tokens",
            "temperature",
            "top_p",
            "reasoning",
        ],
    )?;
    if let Some(reasoning) = wire.get("reasoning") {
        fields(reasoning, &["effort"])?;
        let source = reasoning["effort"].as_str().ok_or_else(invalid)?;
        let native = reasoning_efforts
            .and_then(|m| m.get(source))
            .ok_or_else(unsupported)?;
        if native != "none" && reasoning_support == CapabilitySupport::Unsupported {
            return Err(ProviderError::new(400, "unsupported_reasoning"));
        }
        // Map the source once. A native level may also be a mapped source.
        wire["reasoning"]["effort"] = native.clone().into();
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
        .is_some_and(|v| !v.is_null() && v.as_u64().is_none_or(|n| n < 16))
    {
        return Err(invalid());
    }
    for key in ["temperature", "top_p"] {
        if let Some(value) = wire.get(key).filter(|v| !v.is_null()) {
            let n = value.as_f64().ok_or_else(invalid)?;
            if !(if key == "temperature" {
                (0.0..2.0).contains(&n)
            } else {
                n > 0.0 && n <= 1.0
            }) {
                return Err(invalid());
            }
        }
    }
    if !native_history && let Some(input) = wire["input"].as_array() {
        for item in input {
            validate_message(item, false, runtime_context)?;
        }
    }
    wire["store"] = false.into();
    // background=false expresses foreground execution locally; native ignores it.
    wire.as_object_mut().unwrap().remove("background");
    let compiled = CanonicalRequest::new(wire, ResponsesDialect::Classic).map_err(|_| invalid())?;
    if compiled.wire().to_string().len() > max_bytes {
        return Err(ProviderError::new(413, "invalid_or_oversized_body"));
    }
    Ok(compiled)
}

pub(crate) fn validate_message(
    item: &Value,
    lite: bool,
    runtime_context: bool,
) -> ProviderResult<()> {
    fields(item, &["type", "role", "content", "id", "status"])?;
    let role = item["role"].as_str().ok_or_else(invalid)?;
    if item.get("type").is_some_and(|v| v != "message")
        || !matches!(role, "user" | "assistant" | "system" | "developer")
    {
        return Err(unsupported());
    }
    for key in ["id", "status"] {
        if let Some(value) = item.get(key) {
            let runtime_id = runtime_context
                && key == "id"
                && matches!(role, "user" | "developer")
                && item["content"].is_array();
            if (role != "assistant" && !(key == "id" && lite && role == "developer" || runtime_id))
                || item["type"] != "message"
            {
                return Err(unsupported());
            }
            if key == "id"
                && value
                    .as_str()
                    .is_none_or(|s| s.trim().is_empty() || s.chars().any(char::is_control))
            {
                return Err(invalid());
            }
            if key == "status" && value != "completed" {
                return Err(unsupported());
            }
        }
    }
    if role == "assistant"
        && (item.get("id").is_some() || item.get("status").is_some())
        && (item.get("id").is_none() || item.get("status").is_none() || !item["content"].is_array())
    {
        return Err(invalid());
    }
    match &item["content"] {
        Value::String(_) => (),
        Value::Array(parts) => {
            for part in parts {
                fields(part, &["type", "text", "annotations"])?;
                if part["type"]
                    != if role == "assistant" {
                        "output_text"
                    } else {
                        "input_text"
                    }
                {
                    return Err(unsupported());
                }
                if !part["text"].is_string() {
                    return Err(invalid());
                }
                if part.get("annotations").is_some_and(|v| {
                    role != "assistant" || v.as_array().is_none_or(|a| !a.is_empty())
                }) {
                    return Err(unsupported());
                }
            }
        }
        _ => return Err(invalid()),
    }
    Ok(())
}
