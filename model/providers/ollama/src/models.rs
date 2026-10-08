use caidex_model_core::{CapabilitySupport, ModelCapabilities, ProviderError, ProviderResult};
use serde_json::Value;
use std::{collections::HashSet, fmt};

/// Model-bound native declaration. This is executor-owned catalog evidence,
/// not proof of model inference, the source's authenticity or Codex compatibility.
#[derive(Clone)]
pub struct ModelDetails {
    native_model: String,
    wire: Value,
    capabilities: ModelCapabilities,
}
impl ModelDetails {
    pub fn parse(native_model: String, wire: Value) -> ProviderResult<Self> {
        let invalid = || ProviderError::new(502, "ollama_invalid_model_details");
        if native_model.trim().is_empty()
            || native_model.chars().any(char::is_control)
            || !wire.is_object()
            || wire.get("error").is_some_and(|v| !v.is_null())
        {
            return Err(invalid());
        }
        for key in [
            "parameters",
            "template",
            "system",
            "license",
            "modified_at",
            "renderer",
            "parser",
            "requires",
        ] {
            if wire.get(key).is_some_and(|v| !v.is_string()) {
                return Err(invalid());
            }
        }
        for key in ["model_info", "projector_info"] {
            if wire
                .get(key)
                .is_some_and(|v| !v.is_null() && !v.is_object())
            {
                return Err(invalid());
            }
        }
        if wire.get("details").is_some_and(|v| !v.is_object()) {
            return Err(invalid());
        }
        let mut names = HashSet::new();
        if let Some(values) = wire.get("capabilities") {
            for value in values.as_array().ok_or_else(invalid)? {
                let name = value
                    .as_str()
                    .filter(|v| !v.is_empty() && !v.chars().any(char::is_control))
                    .ok_or_else(invalid)?;
                if !names.insert(name) {
                    return Err(invalid());
                }
            }
        }
        let declared = |name| {
            if !wire
                .as_object()
                .expect("validated object")
                .contains_key("capabilities")
            {
                CapabilitySupport::Unknown
            } else if names.contains(name) {
                CapabilitySupport::Supported
            } else {
                CapabilitySupport::Unsupported
            }
        };
        let mut capabilities = ModelCapabilities {
            text: declared("completion"),
            vision: declared("vision"),
            native_tools: declared("tools"),
            reasoning: declared("thinking"),
            ..ModelCapabilities::default()
        };
        if let Some(thinking) = wire.get("thinking").filter(|v| !v.is_null()) {
            if !thinking.is_object() {
                return Err(invalid());
            }
            let values = thinking["values"]
                .as_array()
                .filter(|v| !v.is_empty())
                .ok_or_else(invalid)?;
            let mut controls = HashSet::new();
            for value in values {
                if !value.is_boolean()
                    && !value
                        .as_str()
                        .is_some_and(|v| !v.is_empty() && !v.chars().any(char::is_control))
                    || !controls.insert(value.to_string())
                {
                    return Err(invalid());
                }
            }
            if !values.contains(&thinking["default"]) {
                return Err(invalid());
            }
            capabilities.reasoning = if values.iter().any(|v| v != &Value::Bool(false)) {
                CapabilitySupport::Supported
            } else {
                CapabilitySupport::Unsupported
            };
        }
        Ok(Self {
            native_model,
            wire,
            capabilities,
        })
    }
    pub fn native_model(&self) -> &str {
        &self.native_model
    }
    /// Keep unknown metadata, templates and precise numbers; never use a
    /// remote_host field to choose a network destination or discover a key.
    pub fn wire(&self) -> &Value {
        &self.wire
    }
    pub fn declared_capabilities(&self) -> &ModelCapabilities {
        &self.capabilities
    }
    pub fn thinking_values(&self) -> Option<&[Value]> {
        self.wire["thinking"]["values"]
            .as_array()
            .map(Vec::as_slice)
    }
    pub fn thinking_default(&self) -> Option<&Value> {
        self.wire
            .get("thinking")
            .filter(|v| !v.is_null())
            .and_then(|v| v.get("default"))
    }
    pub(crate) fn supports_thinking(&self, value: &Value) -> bool {
        self.thinking_values()
            .is_some_and(|values| values.contains(value))
    }
}
impl fmt::Debug for ModelDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ModelDetails([WIRE OMITTED])")
    }
}
