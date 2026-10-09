use caidex_model_core::{CanonicalRequest, ProviderError, ProviderResult, ResponsesDialect};
use serde_json::Value;

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
/// Native Responses ignores unknown controls. Bound and validate the source
/// before credentials; never silently drop safety, tools or history fields.
pub(crate) fn compile(
    request: CanonicalRequest,
    max_bytes: usize,
) -> ProviderResult<CanonicalRequest> {
    if request.dialect() != ResponsesDialect::Classic {
        return Err(ProviderError::new(400, "unsupported_dialect"));
    }
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
            "store",
            "background",
            "max_output_tokens",
            "temperature",
            "top_p",
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
    if let Some(input) = wire["input"].as_array() {
        for item in input {
            fields(item, &["type", "role", "content", "id", "status"])?;
            let role = item["role"].as_str().ok_or_else(invalid)?;
            if item.get("type").is_some_and(|v| v != "message")
                || !matches!(role, "user" | "assistant" | "system" | "developer")
            {
                return Err(unsupported());
            }
            for key in ["id", "status"] {
                if let Some(value) = item.get(key) {
                    if role != "assistant" || item["type"] != "message" {
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
            if (item.get("id").is_some() || item.get("status").is_some())
                && (item.get("id").is_none()
                    || item.get("status").is_none()
                    || !item["content"].is_array())
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
