use crate::request::Options;
use caidex_model_core::{
    CanonicalRequest, CapabilitySupport, ModelCapabilities, ProviderError, ProviderResult,
};
use serde_json::Value;
use std::collections::HashSet;

fn invalid() -> ProviderError {
    ProviderError::new(400, "ollama_invalid_runtime_parameter")
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "ollama_unsupported_runtime_parameter")
}

/// Normalize execution-owned Runtime policy before native prefix binding. Local
/// attribution is consumed, never sent to Ollama or claimed as durable storage.
pub(crate) fn compile(
    request: CanonicalRequest,
    options: &Options,
    capabilities: &ModelCapabilities,
    max_bytes: usize,
) -> ProviderResult<CanonicalRequest> {
    let mut wire = request.wire().clone();
    // Bound the original body before removing local metadata or expanding history.
    if wire.to_string().len() > max_bytes {
        return Err(ProviderError::new(413, "invalid_or_oversized_body"));
    }
    if options.runtime_context {
        if let Some(include) = wire.get("include").filter(|v| !v.is_null()) {
            let mut seen = HashSet::new();
            for value in include.as_array().ok_or_else(invalid)? {
                let name = value.as_str().ok_or_else(invalid)?;
                if !seen.insert(name) {
                    return Err(invalid());
                }
                if name != "reasoning.encrypted_content" || !options.native_history {
                    return Err(unsupported());
                }
            }
        }
        for key in ["client_metadata", "prompt_cache_key"] {
            if let Some(value) = wire.get(key).filter(|v| !v.is_null()) {
                let valid = if key == "client_metadata" {
                    value
                        .as_object()
                        .is_some_and(|map| map.values().all(Value::is_string))
                } else {
                    value
                        .as_str()
                        .is_some_and(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
                };
                if !valid {
                    return Err(invalid());
                }
            }
        }
        for key in ["client_metadata", "prompt_cache_key", "include"] {
            wire.as_object_mut().unwrap().remove(key);
        }
        if let Some(reasoning) = wire.get_mut("reasoning").filter(|v| !v.is_null()) {
            let object = reasoning.as_object_mut().ok_or_else(invalid)?;
            if let Some(summary) = object.get("summary").filter(|v| !v.is_null()) {
                if summary != "auto" || !options.native_history {
                    return Err(unsupported());
                }
                if capabilities.reasoning == CapabilitySupport::Unsupported {
                    return Err(ProviderError::new(400, "ollama_unsupported_capability"));
                }
                // Explicit display policy: native whole thinking is its summary.
                // No native summary length/control or encryption is promised.
            }
            if let Some(context) = object.get("context").filter(|v| !v.is_null())
                && (context != "all_turns" || !options.native_history)
            {
                return Err(unsupported());
            }
            object.remove("summary");
            object.remove("context");
        }
        if let Some(input) = wire["input"].as_array_mut() {
            let mut conversation = false;
            for item in input {
                if item.get("type").is_none() || item["type"] == "message" {
                    match item["role"].as_str() {
                        Some("developer") => {
                            if conversation {
                                return Err(unsupported());
                            }
                            item["role"] = "system".into();
                        }
                        Some("system") => (),
                        _ => conversation = true,
                    }
                } else {
                    conversation = true;
                }
            }
        }
    }
    if let Some(verbosity) = wire.get("text").and_then(|v| v.get("verbosity")) {
        if verbosity.is_null() && options.runtime_context {
            wire["text"]
                .as_object_mut()
                .ok_or_else(invalid)?
                .remove("verbosity");
        } else {
            let instruction = options
                .verbosity_instructions
                .get(verbosity.as_str().ok_or_else(invalid)?)
                .ok_or_else(unsupported)?;
            let original = match wire.get("instructions") {
                None => "",
                Some(value) => value.as_str().ok_or_else(invalid)?,
            };
            wire["instructions"] = if original.is_empty() {
                instruction.clone()
            } else {
                format!("{original}\n{instruction}")
            }
            .into();
            wire["text"]
                .as_object_mut()
                .ok_or_else(invalid)?
                .remove("verbosity");
        }
    }
    CanonicalRequest::new(wire, request.dialect()).map_err(|_| invalid())
}
