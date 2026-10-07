use caidex_model_core::{ProviderError, ProviderResult};
use serde_json::{Value, json};
use std::collections::BTreeSet;
fn invalid() -> ProviderError {
    ProviderError::new(400, "invalid_google_output_format")
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "unsupported_google_output_format")
}
fn unsupported_schema() -> ProviderError {
    ProviderError::new(400, "unsupported_google_output_schema")
}
pub(crate) fn apply(
    wire: &mut Value,
    source: &Value,
    options: &crate::RequestOptions<'_>,
    has_tools: bool,
) -> ProviderResult<()> {
    let Some(text) = source.get("text").filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let fields = text.as_object().ok_or_else(invalid)?;
    if fields
        .keys()
        .any(|k| !matches!(k.as_str(), "format" | "verbosity"))
        || fields.get("verbosity").is_some_and(|v| !v.is_null())
    {
        return Err(unsupported());
    }
    let Some(format) = fields.get("format").filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let fields = format.as_object().ok_or_else(invalid)?;
    if format["type"] == "text" {
        return if fields.len() == 1 {
            Ok(())
        } else {
            Err(unsupported())
        };
    }
    if !options.supports_structured_outputs {
        return Err(unsupported());
    }
    let schema = match format["type"].as_str() {
        Some("json_object") if fields.len() == 1 => json!({"type":"object"}),
        Some("json_schema") => {
            if fields.keys().any(|k| {
                !matches!(
                    k.as_str(),
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
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                || fields
                    .get("strict")
                    .is_some_and(|v| !v.is_null() && !v.is_boolean())
            {
                return Err(invalid());
            }
            if format["strict"] != true || fields.get("description").is_some_and(|v| !v.is_null()) {
                // Wrapper descriptions and non-strict modes have no established
                // equivalent. Schema annotations are preserved below.
                return Err(unsupported());
            }
            let schema = &format["schema"];
            validate_schema(
                schema,
                schema,
                0,
                &mut BTreeSet::new(),
                &mut BTreeSet::new(),
            )?;
            schema.clone()
        }
        _ => return Err(unsupported()),
    };
    if has_tools && !options.supports_structured_outputs_with_tools {
        return Err(ProviderError::new(400, "unsupported_google_output_tools"));
    }
    wire["generationConfig"]["responseFormat"] =
        json!({"text":{"mimeType":"APPLICATION_JSON","schema":schema}});
    Ok(())
}

// This is a native supported-subset gate, not a JSON Schema evaluator. Google
// can ignore unsupported keywords: passing them through would weaken strict.
// References stay verbatim; no expansion, IO, or modification of signed Parts.
fn validate_schema(
    schema: &Value,
    root: &Value,
    depth: usize,
    active: &mut BTreeSet<usize>,
    done: &mut BTreeSet<usize>,
) -> ProviderResult<()> {
    let node = schema as *const Value as usize;
    // ponytail: bounded recursive validation, iterative traversal if real
    // supported schemas need >64 active levels; byte limits still apply.
    if depth > 64 || active.contains(&node) {
        // Native recursive refs are finitely unrolled, not strict equivalence.
        return Err(unsupported_schema());
    }
    if done.contains(&node) {
        return Ok(());
    }
    active.insert(node);
    let fields = schema.as_object().ok_or_else(invalid)?;
    if fields.contains_key("$ref") && fields.keys().any(|k| !k.starts_with('$')) {
        return Err(unsupported_schema());
    }
    for (key, value) in fields {
        match key.as_str() {
            "type" => {
                let types: Vec<&Value> = match value {
                    Value::String(_) => vec![value],
                    Value::Array(values) if !values.is_empty() => values.iter().collect(),
                    _ => return Err(invalid()),
                };
                let mut unique = BTreeSet::new();
                for kind in types {
                    let kind = kind.as_str().ok_or_else(invalid)?;
                    if !matches!(
                        kind,
                        "object" | "array" | "string" | "integer" | "number" | "boolean" | "null"
                    ) || !unique.insert(kind)
                    {
                        return Err(invalid());
                    }
                }
            }
            "title" | "description" => {
                value.as_str().ok_or_else(invalid)?;
            }
            "format" => {
                if !matches!(
                    value.as_str().ok_or_else(invalid)?,
                    "date" | "time" | "date-time"
                ) {
                    return Err(unsupported_schema());
                }
            }
            "enum" => {
                let values = value.as_array().ok_or_else(invalid)?;
                if values.is_empty() {
                    return Err(invalid());
                }
                if values.iter().any(|v| !v.is_string() && !v.is_number()) {
                    return Err(unsupported_schema());
                }
            }
            "minimum" | "maximum" => {
                if !value.is_number() {
                    return Err(invalid());
                }
            }
            "minItems" | "maxItems" => {
                value.as_u64().ok_or_else(invalid)?;
            }
            "required" => {
                let values = value.as_array().ok_or_else(invalid)?;
                let mut unique = BTreeSet::new();
                for name in values {
                    if !unique.insert(name.as_str().ok_or_else(invalid)?) {
                        return Err(invalid());
                    }
                }
            }
            "properties" | "$defs" => {
                for child in value.as_object().ok_or_else(invalid)?.values() {
                    validate_schema(child, root, depth + 1, active, done)?;
                }
            }
            "items" => validate_schema(value, root, depth + 1, active, done)?,
            "additionalProperties" if value.is_boolean() => {}
            "additionalProperties" => validate_schema(value, root, depth + 1, active, done)?,
            "prefixItems" | "anyOf" => {
                let values = value.as_array().ok_or_else(invalid)?;
                if key == "anyOf" && values.is_empty() {
                    return Err(invalid());
                }
                for child in values {
                    validate_schema(child, root, depth + 1, active, done)?;
                }
            }
            "$ref" => {
                let reference = value.as_str().ok_or_else(invalid)?;
                // Local JSON pointers only. IDs/anchors/external resolution have
                // no validated equivalent and must not trigger network access.
                let pointer = local_pointer(reference)?;
                let target = root.pointer(&pointer).ok_or_else(invalid)?;
                validate_schema(target, root, depth + 1, active, done)?;
            }
            // In particular, native oneOf means anyOf: do not pretend exclusive.
            _ => return Err(unsupported_schema()),
        }
    }
    active.remove(&node);
    done.insert(node);
    Ok(())
}

// A JSON Schema $ref is a URI fragment, not a literal JSON Pointer string.
// Decode once before ~0/~1 resolution; the transmitted schema stays untouched.
fn local_pointer(reference: &str) -> ProviderResult<String> {
    let fragment = reference.strip_prefix('#').ok_or_else(unsupported_schema)?;
    let mut bytes = fragment.bytes();
    let mut decoded = Vec::with_capacity(fragment.len());
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes
                .next()
                .and_then(|v| char::from(v).to_digit(16))
                .ok_or_else(invalid)?;
            let low = bytes
                .next()
                .and_then(|v| char::from(v).to_digit(16))
                .ok_or_else(invalid)?;
            decoded.push(((high << 4) | low) as u8);
        } else {
            decoded.push(byte);
        }
    }
    let pointer = String::from_utf8(decoded).map_err(|_| invalid())?;
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return Err(unsupported_schema());
    }
    // Value::pointer replaces valid escapes but doesn't reject other ~ syntax.
    let mut bytes = pointer.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'~' && !matches!(bytes.next(), Some(b'0' | b'1')) {
            return Err(invalid());
        }
    }
    Ok(pointer)
}
