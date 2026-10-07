use caidex_model_core::{ProviderError, ProviderResult};
use serde_json::{Value, json};

fn invalid() -> ProviderError {
    ProviderError::new(400, "invalid_anthropic_output_format")
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "unsupported_anthropic_output_format")
}
pub(crate) fn apply(wire: &mut Value, source: &Value, supported: bool) -> ProviderResult<()> {
    let Some(text) = source.get("text").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let text = text.as_object().ok_or_else(invalid)?;
    if text
        .iter()
        .any(|(key, value)| key != "format" && !(key == "verbosity" && value.is_null()))
    {
        return Err(unsupported());
    }
    let Some(format) = text.get("format").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let object = format.as_object().ok_or_else(invalid)?;
    if format["type"] == "text" {
        return if object.len() == 1 {
            Ok(())
        } else {
            Err(unsupported())
        };
    }
    if format["type"] != "json_schema" || !supported {
        return Err(unsupported());
    }
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "type" | "name" | "schema" | "strict" | "description"
        )
    }) {
        return Err(unsupported());
    }
    let name = format["name"].as_str().ok_or_else(invalid)?;
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(invalid());
    }
    if !format["schema"].is_object() {
        return Err(invalid());
    }
    if format
        .get("strict")
        .is_some_and(|value| !value.is_null() && !value.is_boolean())
    {
        return Err(invalid());
    }
    // Native JSON outputs enforce a schema. Non-strict requests and wrapper
    // descriptions need a separate semantic mapping rather than silent removal.
    if format["strict"] != true
        || format
            .get("description")
            .is_some_and(|value| !value.is_null())
    {
        return Err(unsupported());
    }
    // Keep constraints, references, annotations and future schema keywords
    // verbatim. Provider rejection is preferable to weakening the schema.
    if wire.get("output_config").is_none() {
        wire["output_config"] = json!({});
    }
    wire["output_config"]["format"] = json!({"type":"json_schema","schema":format["schema"]});
    Ok(())
}
