use caidex_model_core::{
    CanonicalRequest, CapabilitySupport, ContextHeaders, ProviderError, ProviderResult,
    ResponsesDialect,
};
use serde_json::{Value, json};
use std::collections::HashMap;
pub(crate) fn valid_effort(value: &str) -> bool {
    matches!(
        value,
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
    )
}
fn invalid() -> ProviderError {
    ProviderError::new(400, "openrouter_invalid_request")
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "openrouter_unsupported_request")
}
fn fields(v: &Value, allowed: &[&str]) -> ProviderResult<()> {
    if v.as_object()
        .ok_or_else(invalid)?
        .keys()
        .any(|k| !allowed.contains(&k.as_str()))
    {
        return Err(unsupported());
    }
    Ok(())
}
pub(crate) fn compile(
    request: CanonicalRequest,
    budget: usize,
    runtime_context: bool,
    efforts: Option<&HashMap<String, String>>,
    reasoning_support: CapabilitySupport,
) -> ProviderResult<CanonicalRequest> {
    if request.wire().to_string().len() > budget {
        return Err(ProviderError::new(413, "invalid_or_oversized_body"));
    }
    if request.dialect() != ResponsesDialect::Classic {
        return Err(ProviderError::new(400, "unsupported_dialect"));
    }
    let mut wire = request.wire().clone();
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
        if let Some(text) = wire.get("text").filter(|v| !v.is_null()) {
            fields(text, &["format", "verbosity"])?;
            if let Some(format) = text.get("format") {
                fields(format, &["type"])?;
                if format["type"] != "text" {
                    return Err(unsupported());
                }
            }
            if text.get("verbosity").is_some_and(|v| !v.is_null()) {
                return Err(unsupported());
            }
        }
        // Only neutral text is consumed; meaningful output controls remain fail-closed.
        wire.as_object_mut().unwrap().remove("text");
    }
    fields(
        &wire,
        &[
            "model",
            "input",
            "stream",
            "instructions",
            "max_output_tokens",
            "temperature",
            "top_p",
            "store",
            "previous_response_id",
            "background",
            "reasoning",
        ],
    )?;
    if let Some(reasoning) = wire.get("reasoning") {
        fields(reasoning, &["effort"])?;
        let source = reasoning["effort"].as_str().ok_or_else(invalid)?;
        let native = efforts
            .and_then(|m| m.get(source))
            .ok_or_else(unsupported)?;
        if native != "none" && reasoning_support == CapabilitySupport::Unsupported {
            return Err(ProviderError::new(400, "unsupported_reasoning"));
        }
        wire["reasoning"]["effort"] = native.clone().into();
    }
    for key in ["store", "background"] {
        if wire.get(key).is_some_and(|v| !v.is_null() && v != false) {
            return Err(unsupported());
        }
    }
    if wire
        .get("previous_response_id")
        .is_some_and(|v| !v.is_null())
    {
        return Err(unsupported());
    }
    if wire
        .get("instructions")
        .is_some_and(|v| !v.is_null() && !v.is_string())
        || wire
            .get("max_output_tokens")
            .is_some_and(|v| !v.is_null() && v.as_u64().is_none_or(|n| n == 0))
    {
        return Err(invalid());
    }
    for (key, upper) in [("temperature", 2.0), ("top_p", 1.0)] {
        if wire
            .get(key)
            .is_some_and(|v| !v.is_null() && v.as_f64().is_none_or(|n| !(0.0..=upper).contains(&n)))
        {
            return Err(invalid());
        }
    }
    if let Some(items) = wire["input"].as_array() {
        for item in items {
            fields(item, &["type", "role", "content", "id", "status"])?;
            let role = item["role"].as_str().ok_or_else(invalid)?;
            if !matches!(role, "system" | "developer" | "user" | "assistant")
                || item.get("type").is_some_and(|t| t != "message")
            {
                return Err(unsupported());
            }
            for key in ["id", "status"] {
                if let Some(v) = item.get(key) {
                    let runtime_id =
                        runtime_context && key == "id" && matches!(role, "developer" | "user");
                    if (role != "assistant" && !runtime_id) || item["type"] != "message" {
                        return Err(unsupported());
                    }
                    if key == "id"
                        && v.as_str()
                            .is_none_or(|s| s.trim().is_empty() || s.chars().any(char::is_control))
                        || key == "status" && v != "completed"
                    {
                        return Err(invalid());
                    }
                }
            }
            if !item["content"].is_string() {
                for part in item["content"].as_array().ok_or_else(invalid)? {
                    fields(part, &["type", "text", "annotations", "logprobs"])?;
                    if !matches!(part["type"].as_str(), Some("input_text" | "output_text")) {
                        return Err(unsupported());
                    }
                    if !part["text"].is_string() {
                        return Err(invalid());
                    }
                    for key in ["annotations", "logprobs"] {
                        if part.get(key).is_some_and(|v| {
                            !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty())
                        }) {
                            return Err(unsupported());
                        }
                    }
                }
            }
        }
    }
    // Router defaults may silently ignore parameters or retry another backend.
    wire["provider"] = json!({"require_parameters":true,"allow_fallbacks":false});
    wire["store"] = false.into();
    CanonicalRequest::new(wire, ResponsesDialect::Classic).map_err(|_| invalid())
}
pub(crate) fn headers(headers: &ContextHeaders) -> ProviderResult<()> {
    if headers.get("x-codex-turn-state").is_some() {
        return Err(ProviderError::new(502, "openrouter_unexpected_turn_state"));
    }
    Ok(())
}
pub(crate) fn output(wire: &Value) -> ProviderResult<()> {
    let tool = |v: &Value| {
        v["type"].as_str().is_some_and(|k| {
            k.ends_with("_call")
                || k.ends_with("_call_output")
                || k.starts_with("mcp_")
                || k.starts_with("tool_search_")
        })
    };
    if tool(wire)
        || tool(&wire["item"])
        || wire["output"]
            .as_array()
            .is_some_and(|a| a.iter().any(tool))
        || wire["response"]["output"]
            .as_array()
            .is_some_and(|a| a.iter().any(tool))
        || wire["type"].as_str().is_some_and(|k| {
            k.starts_with("response.")
                && (k.contains("_call")
                    || k.starts_with("response.mcp_")
                    || k.starts_with("response.tool_search_"))
        })
    {
        return Err(ProviderError::new(502, "openrouter_unexpected_tool"));
    }
    if wire["store"] == true || wire["response"]["store"] == true {
        return Err(ProviderError::new(502, "openrouter_unexpected_storage"));
    }
    Ok(())
}
