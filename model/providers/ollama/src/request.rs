use crate::ModelDetails;
use caidex_model_core::{CanonicalRequest, CapabilitySupport, ProviderError, ProviderResult};
use serde_json::Value;
use std::collections::HashSet;

fn invalid() -> ProviderError {
    ProviderError::new(400, "ollama_invalid_request")
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "ollama_unsupported_request")
}
fn fields(wire: &Value, allowed: &[&str]) -> ProviderResult<()> {
    if wire
        .as_object()
        .ok_or_else(invalid)?
        .keys()
        .any(|key| !allowed.contains(&key.as_str()))
    {
        return Err(unsupported());
    }
    Ok(())
}
fn string(wire: &Value, key: &str) -> ProviderResult<()> {
    if wire[key]
        .as_str()
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err(invalid());
    }
    Ok(())
}
fn text(content: &Value, assistant: bool) -> ProviderResult<()> {
    if content.is_string() {
        return Ok(());
    }
    for part in content.as_array().ok_or_else(invalid)? {
        fields(part, &["type", "text", "annotations", "logprobs"])?;
        if part["type"] != "input_text" && !(assistant && part["type"] == "output_text") {
            return Err(unsupported());
        }
        if !part["text"].is_string() {
            return Err(invalid());
        }
        for key in ["annotations", "logprobs"] {
            if part
                .get(key)
                .is_some_and(|value| value.as_array().is_none_or(|values| !values.is_empty()))
            {
                return Err(unsupported());
            }
        }
    }
    Ok(())
}

/// Reject controls the native decoder would ignore; never silently filter
/// meaningful history or pretend that echoed flags implement an API guarantee.
pub(crate) fn compile(
    request: CanonicalRequest,
    details: Option<&ModelDetails>,
    reasoning: CapabilitySupport,
) -> ProviderResult<CanonicalRequest> {
    let mut wire = request.wire().clone();
    fields(
        &wire,
        &[
            "model",
            "input",
            "stream",
            "instructions",
            "tools",
            "temperature",
            "top_p",
            "max_output_tokens",
            "store",
            "background",
            "parallel_tool_calls",
            "tool_choice",
            "think",
            "reasoning",
        ],
    )?;
    thinking(&mut wire, details, reasoning)?;
    for key in ["store", "background"] {
        if let Some(value) = wire.get(key) {
            if !value.is_null() && value != &Value::Bool(false) {
                return Err(unsupported());
            }
            wire.as_object_mut().expect("validated object").remove(key);
        }
    }
    if let Some(value) = wire.get("parallel_tool_calls") {
        if value != &Value::Bool(true) {
            return Err(unsupported());
        }
        // Allowing parallel calls is the native default, not enforcing a limit.
        wire.as_object_mut()
            .expect("validated object")
            .remove("parallel_tool_calls");
    }
    if let Some(value) = wire.get("tool_choice") {
        if value != "auto" {
            return Err(unsupported());
        }
        wire.as_object_mut()
            .expect("validated object")
            .remove("tool_choice");
    }
    if wire
        .get("instructions")
        .is_some_and(|value| !value.is_string())
    {
        return Err(invalid());
    }
    for (key, upper) in [("temperature", 2.0), ("top_p", 1.0)] {
        if wire.get(key).is_some_and(|value| {
            value
                .as_f64()
                .is_none_or(|n| !n.is_finite() || n < 0.0 || n > upper)
        }) {
            return Err(invalid());
        }
    }
    if wire
        .get("max_output_tokens")
        .is_some_and(|value| value.as_u64().is_none_or(|n| n == 0 || n > i64::MAX as u64))
    {
        return Err(invalid());
    }
    let mut names = HashSet::new();
    if let Some(tools) = wire.get("tools") {
        for tool in tools.as_array().ok_or_else(invalid)? {
            fields(
                tool,
                &[
                    "type",
                    "name",
                    "description",
                    "parameters",
                    "strict",
                    "defer_loading",
                ],
            )?;
            if tool["type"] != "function" {
                return Err(unsupported());
            }
            string(tool, "name")?;
            if !names.insert(tool["name"].as_str().expect("validated name")) {
                return Err(invalid());
            }
            if tool
                .get("description")
                .is_some_and(|value| !value.is_string())
                || !tool["parameters"].is_object()
            {
                return Err(invalid());
            }
            for key in ["strict", "defer_loading"] {
                if tool
                    .get(key)
                    .is_some_and(|value| !value.is_null() && value != &Value::Bool(false))
                {
                    return Err(unsupported());
                }
            }
        }
    }
    if let Some(items) = wire["input"].as_array() {
        let mut pending = HashSet::new();
        for item in items {
            if item.get("status").is_some_and(|value| value != "completed") {
                return Err(unsupported());
            }
            if item.get("id").is_some_and(|value| !value.is_string()) {
                return Err(invalid());
            }
            let kind = match item.get("type") {
                None => "message",
                Some(value) => value.as_str().ok_or_else(invalid)?,
            };
            match kind {
                "message" => {
                    fields(item, &["type", "id", "status", "role", "content"])?;
                    if !matches!(item["role"].as_str(), Some("user" | "system" | "assistant")) {
                        return Err(unsupported());
                    }
                    text(&item["content"], item["role"] == "assistant")?;
                }
                "function_call" => {
                    fields(
                        item,
                        &["type", "id", "status", "name", "call_id", "arguments"],
                    )?;
                    string(item, "name")?;
                    string(item, "call_id")?;
                    let arguments: Value =
                        serde_json::from_str(item["arguments"].as_str().ok_or_else(invalid)?)
                            .map_err(|_| invalid())?;
                    if !arguments.is_object()
                        || !pending.insert(item["call_id"].as_str().expect("validated ID"))
                    {
                        return Err(invalid());
                    }
                }
                "function_call_output" => {
                    fields(item, &["type", "id", "status", "call_id", "output"])?;
                    string(item, "call_id")?;
                    let id = item["call_id"].as_str().expect("validated ID");
                    if !pending.remove(id) {
                        return Err(invalid());
                    }
                    text(&item["output"], true)?;
                }
                _ => return Err(unsupported()),
            }
        }
        if !pending.is_empty() {
            return Err(invalid());
        }
    }
    CanonicalRequest::new(wire, request.dialect()).map_err(|_| invalid())
}

fn thinking(
    wire: &mut Value,
    details: Option<&ModelDetails>,
    capability: CapabilitySupport,
) -> ProviderResult<()> {
    let mut control = wire.get("think").filter(|v| !v.is_null()).cloned();
    if let Some(reasoning) = wire.get("reasoning").filter(|v| !v.is_null()) {
        fields(reasoning, &["effort"])?;
        if let Some(effort) = reasoning.get("effort").filter(|v| !v.is_null()) {
            if control.is_some() {
                return Err(invalid());
            }
            let effort = effort.as_str().ok_or_else(invalid)?;
            control = Some(if effort == "none" {
                Value::Bool(false)
            } else {
                Value::String(effort.into())
            });
        }
    }
    if let Some(control) = &control {
        if !control.is_boolean() && !control.is_string() {
            return Err(invalid());
        }
        if !details.is_some_and(|details| details.supports_thinking(control)) {
            return Err(unsupported());
        }
        if capability == CapabilitySupport::Unsupported && control != &Value::Bool(false) {
            return Err(ProviderError::new(400, "ollama_unsupported_capability"));
        }
    }
    // Avoid native effort aliases and fallback: only a declared exact value is
    // sent as think. No control leaves the model's default untouched.
    let object = wire.as_object_mut().expect("validated object");
    object.remove("reasoning");
    if let Some(control) = control {
        object.insert("think".into(), control);
    }
    Ok(())
}
