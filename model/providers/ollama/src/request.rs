use crate::ModelDetails;
use base64::Engine;
use caidex_model_core::{
    CanonicalRequest, CapabilitySupport, ModelCapabilities, ProviderError, ProviderResult,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(crate) struct Options {
    pub native_history: bool,
    pub native_tools: bool,
    pub images: bool,
    pub structured_output: bool,
    pub runtime_context: bool,
    pub verbosity_instructions: HashMap<String, String>,
}

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
fn text(content: &Value, assistant: bool, images: bool, max_bytes: usize) -> ProviderResult<()> {
    if content.is_string() {
        return Ok(());
    }
    for part in content.as_array().ok_or_else(invalid)? {
        if part["type"] == "input_image" {
            image(part, images, max_bytes)?;
            continue;
        }
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
    capabilities: &ModelCapabilities,
    options: &Options,
    max_bytes: usize,
) -> ProviderResult<CanonicalRequest> {
    let mut wire = request.wire().clone();
    if wire.to_string().len() > max_bytes {
        return Err(ProviderError::new(413, "invalid_or_oversized_body"));
    }
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
            "text",
        ],
    )?;
    thinking(&mut wire, details, capabilities.reasoning)?;
    output_format(
        &mut wire,
        options.structured_output
            && capabilities.structured_output != CapabilitySupport::Unsupported,
    )?;
    let images = options.images && capabilities.vision != CapabilitySupport::Unsupported;
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
    if !options.native_tools
        && let Some(tools) = wire.get("tools")
    {
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
        let mut pending_thinking = false;
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
                "reasoning" if options.native_history => {
                    // Only verified capsules are expanded by the provider;
                    // arbitrary caller reasoning never reaches this branch.
                    if pending_thinking
                        || !item["encrypted_content"].is_string()
                        || !item["summary"].is_array()
                    {
                        return Err(invalid());
                    }
                    pending_thinking = true;
                }
                "message" => {
                    fields(item, &["type", "id", "status", "role", "content"])?;
                    if !matches!(item["role"].as_str(), Some("user" | "system" | "assistant")) {
                        return Err(unsupported());
                    }
                    if pending_thinking && item["role"] != "assistant" {
                        return Err(invalid());
                    }
                    if item["role"] == "assistant" {
                        pending_thinking = false;
                    }
                    text(
                        &item["content"],
                        item["role"] == "assistant",
                        images,
                        max_bytes,
                    )?;
                }
                "function_call" => {
                    pending_thinking = false;
                    if options.native_tools {
                        continue;
                    }
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
                    if pending_thinking {
                        return Err(invalid());
                    }
                    if options.native_tools {
                        text(&item["output"], true, images, max_bytes)?;
                        continue;
                    }
                    fields(item, &["type", "id", "status", "call_id", "output"])?;
                    string(item, "call_id")?;
                    let id = item["call_id"].as_str().expect("validated ID");
                    if !pending.remove(id) {
                        return Err(invalid());
                    }
                    text(&item["output"], true, images, max_bytes)?;
                }
                "tool_search_call" if options.native_tools => pending_thinking = false,
                "tool_search_output" if options.native_tools => {
                    if pending_thinking {
                        return Err(invalid());
                    }
                }
                _ => return Err(unsupported()),
            }
        }
        if !pending.is_empty() || pending_thinking {
            return Err(invalid());
        }
    }
    CanonicalRequest::new(wire, request.dialect()).map_err(|_| invalid())
}

fn image(part: &Value, enabled: bool, max_bytes: usize) -> ProviderResult<()> {
    if !enabled {
        return Err(unsupported());
    }
    fields(part, &["type", "image_url", "detail"])?;
    if part
        .get("detail")
        .is_some_and(|detail| !detail.is_null() && detail != "auto")
    {
        // Native decoding ignores detail; never pretend low/high is applied.
        return Err(unsupported());
    }
    let url = part["image_url"].as_str().ok_or_else(invalid)?;
    if url.len() > max_bytes {
        return Err(invalid());
    }
    let data = url.strip_prefix("data:").ok_or_else(unsupported)?;
    let (header, data) = data.split_once(',').ok_or_else(invalid)?;
    let mime = header.strip_suffix(";base64").ok_or_else(invalid)?;
    if !matches!(
        mime,
        "" | "image/png" | "image/jpeg" | "image/jpg" | "image/webp"
    ) {
        return Err(unsupported());
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|_| invalid())?;
    if decoded.is_empty() || decoded.len() > max_bytes {
        return Err(invalid());
    }
    Ok(())
}

fn output_format(wire: &mut Value, enabled: bool) -> ProviderResult<()> {
    let Some(text) = wire.get("text").filter(|value| !value.is_null()) else {
        wire.as_object_mut()
            .expect("validated object")
            .remove("text");
        return Ok(());
    };
    fields(text, &["format"])?;
    let Some(format) = text.get("format").filter(|value| !value.is_null()) else {
        wire.as_object_mut()
            .expect("validated object")
            .remove("text");
        return Ok(());
    };
    if format["type"] == "text" {
        return fields(format, &["type"]);
    }
    if !enabled {
        return Err(unsupported());
    }
    match format["type"].as_str() {
        Some("json_object") => {
            fields(format, &["type"])?;
            // The native Responses decoder ignores json_object. An object
            // schema reaches the same native JSON grammar without that loss.
            wire["text"] = json!({"format":{"type":"json_schema","name":"caidex_json_object","schema":{"type":"object"}}});
        }
        Some("json_schema") => {
            fields(format, &["type", "name", "schema", "strict", "description"])?;
            let name = format["name"].as_str().ok_or_else(invalid)?;
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                || !format["schema"].is_object()
                || format
                    .get("strict")
                    .is_some_and(|strict| !strict.is_null() && !strict.is_boolean())
            {
                return Err(invalid());
            }
            // Native ignores strict; the delivery guard enforces it. Wrapper
            // guidance has no native mapping, so it still requires rejection.
            if format
                .get("description")
                .is_some_and(|description| !description.is_null())
            {
                return Err(unsupported());
            }
        }
        _ => return Err(unsupported()),
    }
    Ok(())
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
