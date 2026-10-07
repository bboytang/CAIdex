use caidex_model_core::{ProviderError, ProviderResult};
use serde_json::Value;
use std::collections::BTreeSet;

fn invalid() -> ProviderError {
    ProviderError::new(400, "invalid_anthropic_runtime_parameter")
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "unsupported_anthropic_runtime_parameter")
}
/// Execution-side routing policy, not equality of providers' capacity/SLA.
#[derive(Debug)]
pub struct ServiceTierMapping {
    source: String,
    native: String,
}
impl ServiceTierMapping {
    pub fn new(source: String, native: String) -> ProviderResult<Self> {
        // Native auto cannot guarantee OpenAI priority or flex semantics.
        if !matches!(source.as_str(), "auto" | "default")
            || !matches!(native.as_str(), "auto" | "standard_only")
        {
            return Err(invalid());
        }
        Ok(Self { source, native })
    }
}
pub(crate) fn apply(
    wire: &mut Value,
    source: &Value,
    options: &crate::RequestOptions<'_>,
) -> ProviderResult<()> {
    if let Some(include) = source.get("include").filter(|value| !value.is_null()) {
        let mut seen = BTreeSet::new();
        for value in include.as_array().ok_or_else(invalid)? {
            let value = value.as_str().ok_or_else(invalid)?;
            if value != "reasoning.encrypted_content" {
                return Err(unsupported());
            }
            if !seen.insert(value) {
                return Err(invalid());
            }
        }
        // Native replies project their complete signed history into the CAIdex
        // carrier. This field is never sent to the native Messages endpoint.
    }
    for key in ["prompt_cache_key", "client_metadata"] {
        if let Some(value) = source.get(key).filter(|value| !value.is_null()) {
            if !options.retain_runtime_metadata {
                return Err(unsupported());
            }
            if key == "prompt_cache_key" {
                if value.as_str().is_none_or(str::is_empty) {
                    return Err(invalid());
                }
            } else if value
                .as_object()
                .is_none_or(|object| object.values().any(|value| !value.is_string()))
            {
                return Err(invalid());
            }
            // Preserved verbatim by MessagesRequest.source(), for local runtime
            // attribution only. No native user_id or cache guarantee is invented.
        }
    }
    let mut tiers = BTreeSet::new();
    if options
        .service_tier_mappings
        .iter()
        .any(|mapping| !tiers.insert(mapping.source.as_str()))
    {
        return Err(invalid());
    }
    if let Some(tier) = source.get("service_tier").filter(|value| !value.is_null()) {
        let tier = tier.as_str().ok_or_else(invalid)?;
        let mapping = options
            .service_tier_mappings
            .iter()
            .find(|mapping| mapping.source == tier)
            .ok_or_else(unsupported)?;
        wire["service_tier"] = mapping.native.clone().into();
    }
    if let Some(stream) = source
        .get("stream_options")
        .filter(|value| !value.is_null())
        && !stream.as_object().ok_or_else(invalid)?.is_empty()
    {
        return Err(unsupported());
    }
    if source
        .get("access_programs")
        .is_some_and(|value| !value.is_null())
    {
        return Err(unsupported());
    }
    Ok(())
}
