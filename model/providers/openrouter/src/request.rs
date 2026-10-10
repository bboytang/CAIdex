use caidex_model_core::{
    CanonicalRequest, CapabilitySupport, ContextHeaders, ProviderError, ProviderResult,
    ResponsesDialect,
};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
/// Source is bounded before consuming the Runtime's prompt-only tool prefix.
pub(crate) fn classic(
    request: CanonicalRequest,
) -> ProviderResult<(CanonicalRequest, Option<Value>)> {
    if request.dialect() == ResponsesDialect::Classic {
        return Ok((request, None));
    }
    let mut wire = request.wire().clone();
    if wire
        .get("parallel_tool_calls")
        .is_some_and(|v| !v.is_boolean())
    {
        return Err(invalid());
    }
    let input = wire["input"].as_array_mut().ok_or_else(invalid)?;
    let mut prefix = Value::Null;
    for (index, item) in input.iter().enumerate() {
        if item["type"] != "additional_tools" {
            continue;
        }
        fields(item, &["type", "id", "role", "tools"])?;
        if index != 0
            || item["role"] != "developer"
            || !item["tools"].is_array()
            || item.get("id").is_some_and(|v| {
                v.as_str()
                    .is_none_or(|s| s.trim().is_empty() || s.chars().any(char::is_control))
            })
        {
            return Err(invalid());
        }
        prefix = item.clone();
    }
    if !prefix.is_null() {
        input.remove(0);
        wire["tools"] = prefix["tools"].clone();
    }
    Ok((
        CanonicalRequest::new(wire, ResponsesDialect::Classic).map_err(|_| invalid())?,
        Some(json!({"additional_tools":prefix})),
    ))
}
pub(crate) fn valid_effort(value: &str) -> bool {
    matches!(
        value,
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
    )
}
pub(crate) fn valid_summary(value: &str) -> bool {
    matches!(value, "auto" | "concise" | "detailed")
}
pub(crate) fn valid_context(value: &str) -> bool {
    matches!(value, "auto" | "all_turns" | "current_turn")
}
/// Extract only configured native history controls before the single effort compilation.
pub(crate) fn history_controls(
    wire: &mut Value,
    runtime: bool,
    summaries: Option<&BTreeSet<String>>,
    contexts: Option<&BTreeSet<String>>,
    support: CapabilitySupport,
) -> ProviderResult<Value> {
    let mut native = json!({});
    if !runtime {
        return Ok(native);
    }
    if let Some(include) = wire.get("include") {
        if !include.is_null() {
            let values = include.as_array().ok_or_else(invalid)?;
            if !(values.is_empty()
                || values.len() == 1 && values[0] == "reasoning.encrypted_content")
            {
                return Err(unsupported());
            }
            if !values.is_empty() && support == CapabilitySupport::Unsupported {
                return Err(ProviderError::new(400, "unsupported_reasoning"));
            }
        }
        native["include"] = include.clone();
    }
    wire.as_object_mut().unwrap().remove("include");
    if let Some(reasoning) = wire.get_mut("reasoning") {
        fields(reasoning, &["effort", "summary", "context"])?;
        let controls = reasoning.get("summary").is_some() || reasoning.get("context").is_some();
        for (key, choices, valid) in [
            ("summary", summaries, valid_summary as fn(&str) -> bool),
            ("context", contexts, valid_context as fn(&str) -> bool),
        ] {
            if let Some(value) = reasoning.get(key) {
                if !value.is_null() {
                    let value = value.as_str().ok_or_else(invalid)?;
                    if !valid(value) || !choices.is_some_and(|s| s.contains(value)) {
                        return Err(unsupported());
                    }
                    if support == CapabilitySupport::Unsupported {
                        return Err(ProviderError::new(400, "unsupported_reasoning"));
                    }
                }
                native["reasoning"][key] = value.clone();
            }
            reasoning.as_object_mut().unwrap().remove(key);
        }
        if controls && reasoning.as_object().unwrap().is_empty() {
            wire.as_object_mut().unwrap().remove("reasoning");
        }
    }
    Ok(native)
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
    verbosity: Option<&HashMap<String, String>>,
    tiers: Option<&HashMap<String, String>>,
) -> ProviderResult<CanonicalRequest> {
    if request.wire().to_string().len() > budget {
        return Err(ProviderError::new(413, "invalid_or_oversized_body"));
    }
    if request.dialect() != ResponsesDialect::Classic {
        return Err(ProviderError::new(400, "unsupported_dialect"));
    }
    let mut wire = request.wire().clone();
    consume_context(&mut wire, runtime_context)?;
    if let Some(text) = wire.get("text") {
        if !runtime_context && verbosity.is_none() {
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
            if let Some(level) = text.get("verbosity")
                && !(level.is_null() && runtime_context)
            {
                let level = level.as_str().ok_or_else(invalid)?;
                let instruction = verbosity
                    .and_then(|m| m.get(level))
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
        // Only explicit guidance and neutral text are consumed; structured output stays closed.
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
            "service_tier",
        ],
    )?;
    if let Some(tier) = wire.get("service_tier") {
        let source = tier.as_str().ok_or_else(invalid)?;
        let native = tiers.and_then(|m| m.get(source)).ok_or_else(unsupported)?;
        wire["service_tier"] = native.clone().into();
    }
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
            validate_message(item, runtime_context)?;
        }
    }
    // Router defaults may silently ignore parameters or retry another backend.
    wire["provider"] = json!({"require_parameters":true,"allow_fallbacks":false});
    wire["store"] = false.into();
    CanonicalRequest::new(wire, ResponsesDialect::Classic).map_err(|_| invalid())
}
pub(crate) fn consume_context(wire: &mut Value, runtime_context: bool) -> ProviderResult<()> {
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
    Ok(())
}
pub(crate) fn validate_message(item: &Value, runtime_context: bool) -> ProviderResult<()> {
    fields(item, &["type", "role", "content", "id", "status"])?;
    let role = item["role"].as_str().ok_or_else(invalid)?;
    if !matches!(role, "system" | "developer" | "user" | "assistant")
        || item.get("type").is_some_and(|t| t != "message")
    {
        return Err(unsupported());
    }
    for key in ["id", "status"] {
        if let Some(v) = item.get(key) {
            let runtime_id = runtime_context && key == "id" && matches!(role, "developer" | "user");
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
                if part
                    .get(key)
                    .is_some_and(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()))
                {
                    return Err(unsupported());
                }
            }
        }
    }
    Ok(())
}
pub(crate) fn headers(headers: &ContextHeaders) -> ProviderResult<()> {
    if headers.get("x-codex-turn-state").is_some() {
        return Err(ProviderError::new(502, "openrouter_unexpected_turn_state"));
    }
    Ok(())
}
pub(crate) fn output(wire: &Value, native_tools: bool) -> ProviderResult<()> {
    let tool = |v: &Value| {
        v["type"].as_str().is_some_and(|k| {
            !(native_tools && matches!(k, "function_call" | "custom_tool_call"))
                && (k.ends_with("_call")
                    || k.ends_with("_call_output")
                    || k.starts_with("mcp_")
                    || k.starts_with("tool_search_"))
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
                && !(native_tools
                    && matches!(
                        k,
                        "response.function_call_arguments.delta"
                            | "response.function_call_arguments.done"
                            | "response.custom_tool_call_input.delta"
                            | "response.custom_tool_call_input.done"
                    ))
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
