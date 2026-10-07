use caidex_model_core::{ProviderError, ProviderResult};
use serde_json::Value;
use std::collections::BTreeSet;

/// Execution-side selection of Google's tier, not cross-provider SLA equality.
#[derive(Debug)]
pub struct ServiceTierMapping {
    source: String,
    native: String,
}
impl ServiceTierMapping {
    pub fn new(source: String, native: String) -> ProviderResult<Self> {
        if !matches!(
            (source.as_str(), native.as_str()),
            ("auto" | "default", "standard") | ("priority", "priority") | ("flex", "flex")
        ) {
            return Err(invalid());
        }
        Ok(Self { source, native })
    }
}

fn invalid() -> ProviderError {
    ProviderError::new(400, "invalid_google_runtime_parameter")
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "unsupported_google_runtime_parameter")
}
pub(crate) fn apply(
    wire: &mut Value,
    source: &Value,
    options: &crate::RequestOptions<'_>,
) -> ProviderResult<()> {
    if let Some(include) = source.get("include").filter(|v| !v.is_null()) {
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
        // Local history carrier selection, never sent to generateContent.
    }
    for key in ["prompt_cache_key", "client_metadata"] {
        if let Some(value) = source.get(key).filter(|v| !v.is_null()) {
            if !options.retain_runtime_metadata {
                return Err(unsupported());
            }
            if key == "prompt_cache_key" {
                if value.as_str().is_none_or(str::is_empty) {
                    return Err(invalid());
                }
            } else if value
                .as_object()
                .is_none_or(|map| map.values().any(|v| !v.is_string()))
            {
                return Err(invalid());
            }
            // Retained verbatim in source(), for local attribution only. No
            // Google labels, safety identifier or cache resource is invented.
        }
    }
    let mut tiers = BTreeSet::new();
    if options
        .service_tier_mappings
        .iter()
        .any(|m| !tiers.insert(m.source.as_str()))
    {
        return Err(invalid());
    }
    if let Some(tier) = source.get("service_tier").filter(|v| !v.is_null()) {
        let tier = tier.as_str().ok_or_else(invalid)?;
        let mapping = options
            .service_tier_mappings
            .iter()
            .find(|m| m.source == tier)
            .ok_or_else(unsupported)?;
        wire["serviceTier"] = mapping.native.clone().into();
    }
    if let Some(stream) = source.get("stream_options").filter(|v| !v.is_null())
        && !stream.as_object().ok_or_else(invalid)?.is_empty()
    {
        return Err(unsupported());
    }
    if source.get("access_programs").is_some_and(|v| !v.is_null()) {
        return Err(unsupported());
    }
    Ok(())
}
