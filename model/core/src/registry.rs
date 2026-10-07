use crate::{Error, ResponsesDialect, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Provider-declared capabilities, not a claim that live compatibility passed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CapabilitySupport {
    #[default]
    Unknown,
    Supported,
    Unsupported,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelCapabilities {
    pub text: CapabilitySupport,
    pub vision: CapabilitySupport,
    pub reasoning: CapabilitySupport,
    pub native_tools: CapabilitySupport,
    pub parallel_tools: CapabilitySupport,
    pub structured_output: CapabilitySupport,
    pub streaming: CapabilitySupport,
    pub web_search: CapabilitySupport,
    pub image_generation: CapabilitySupport,
    pub context_window: Option<u64>,
    pub output_limit: Option<u64>,
    pub prompt_profile: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompatibilityLevel {
    Full,
    Compatible,
    Limited,
    Experimental,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceSource {
    Configured,
    ProtocolFixture,
    LiveProvider,
    /// Actual provider inference through the fixed Runtime, not a synthetic
    /// protocol server exercised by a real Runtime process.
    LiveRuntime,
}

/// A reference to an actual versioned report. Registry validation checks its
/// scope, not the truth of a report supplied by trusted executor configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompatibilityReport {
    pub schema_version: u32,
    pub level: CompatibilityLevel,
    pub source: EvidenceSource,
    pub reference: String,
    pub tested_model_version: String,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelMetadata {
    pub id: String,
    pub native_model: String,
    pub display_name: String,
    pub source: EvidenceSource,
    pub dialects: Vec<ResponsesDialect>,
    pub capabilities: ModelCapabilities,
    /// None means unverified; model names and fixture success cannot imply Full.
    pub codex_compatibility: Option<CompatibilityReport>,
}
impl ModelMetadata {
    pub fn configured(id: String, native_model: String, dialects: Vec<ResponsesDialect>) -> Self {
        Self {
            display_name: id.clone(),
            id,
            native_model,
            dialects,
            source: EvidenceSource::Configured,
            capabilities: ModelCapabilities::default(),
            codex_compatibility: None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        let valid = |value: &str| !value.trim().is_empty() && !value.chars().any(char::is_control);
        if !valid(&self.id)
            || !valid(&self.native_model)
            || !valid(&self.display_name)
            || self.dialects.is_empty()
            || self
                .dialects
                .iter()
                .enumerate()
                .any(|(index, dialect)| self.dialects[..index].contains(dialect))
            || self.capabilities.context_window == Some(0)
            || self.capabilities.output_limit == Some(0)
            || self
                .capabilities
                .prompt_profile
                .as_deref()
                .is_some_and(|profile| !valid(profile))
        {
            return Err(Error::InvalidMetadata);
        }
        if let Some(report) = &self.codex_compatibility
            && (report.schema_version != 1
                || !valid(&report.reference)
                || !valid(&report.tested_model_version)
                || report.source == EvidenceSource::Configured
                || matches!(
                    report.level,
                    CompatibilityLevel::Full | CompatibilityLevel::Compatible
                ) && report.source != EvidenceSource::LiveRuntime)
        {
            return Err(Error::InvalidMetadata);
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ModelRegistry(BTreeMap<String, ModelMetadata>);
impl ModelRegistry {
    pub fn new(models: impl IntoIterator<Item = ModelMetadata>) -> Result<Self> {
        let mut registry = BTreeMap::new();
        for model in models {
            model.validate()?;
            if registry.insert(model.id.clone(), model).is_some() {
                return Err(Error::InvalidMetadata);
            }
        }
        if registry.is_empty() {
            return Err(Error::InvalidMetadata);
        }
        Ok(Self(registry))
    }
    pub fn get(&self, model: &str) -> Option<&ModelMetadata> {
        self.0.get(model)
    }
    pub fn models(&self) -> impl Iterator<Item = &ModelMetadata> {
        self.0.values()
    }
}
