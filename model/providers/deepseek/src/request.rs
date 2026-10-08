use caidex_model_core::{CanonicalRequest, ProviderError, ProviderResult};
use serde_json::Value;

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
/// The native API silently ignores unsupported controls and downgrades
/// developer messages. Reject them before authentication, rather than claim
/// an equivalent response from a successful HTTP request.
pub(crate) fn compile(
    request: CanonicalRequest,
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
            "instructions",
            "stream",
            "store",
            "background",
            "max_output_tokens",
            "text",
        ],
    )?;
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
