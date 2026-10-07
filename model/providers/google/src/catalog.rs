use caidex_model_core::{
    CapabilitySupport, EvidenceSource, ModelCapabilities, ModelMetadata, ProviderError,
    ProviderResult, ResponsesDialect,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
};

fn invalid() -> ProviderError {
    ProviderError::new(502, "google_invalid_model_catalog")
}
fn nonempty(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
}
pub(crate) fn resource_name(name: &str) -> bool {
    name.strip_prefix("models/").is_some_and(|id| {
        !id.is_empty()
            && !matches!(id, "." | "..")
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    })
}

#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct NativeModel(Value);
impl NativeModel {
    pub fn name(&self) -> &str {
        self.0["name"].as_str().unwrap()
    }
    pub fn wire(&self) -> &Value {
        &self.0
    }
    /// Method availability is catalog evidence, not inference or tool support.
    pub fn supports_generation_method(&self, method: &str) -> Option<bool> {
        self.0["supportedGenerationMethods"]
            .as_array()
            .map(|methods| methods.iter().any(|value| value == method))
    }
    pub fn metadata(
        &self,
        alias: &str,
        dialects: Vec<ResponsesDialect>,
    ) -> ProviderResult<ModelMetadata> {
        let mut metadata = ModelMetadata::configured(alias.into(), self.name().into(), dialects);
        metadata.display_name = nonempty(&self.0["displayName"])
            .unwrap_or(self.name())
            .into();
        metadata.source = EvidenceSource::ProviderCatalog;
        metadata.capabilities = ModelCapabilities {
            reasoning: match self.0["thinking"].as_bool() {
                Some(true) => CapabilitySupport::Supported,
                Some(false) => CapabilitySupport::Unsupported,
                None => CapabilitySupport::Unknown,
            },
            context_window: self.0["inputTokenLimit"].as_u64().filter(|v| *v > 0),
            output_limit: self.0["outputTokenLimit"].as_u64().filter(|v| *v > 0),
            ..Default::default()
        };
        metadata.validate().map_err(|_| invalid())?;
        Ok(metadata)
    }
}
impl fmt::Debug for NativeModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeModel([WIRE OMITTED])")
    }
}

pub struct ModelsPage {
    models: Vec<NativeModel>,
    next: Option<String>,
}
impl ModelsPage {
    pub fn parse(wire: Value) -> ProviderResult<Self> {
        if !wire.is_object() {
            return Err(invalid());
        }
        let data = match wire.get("models") {
            None | Some(Value::Null) => &[][..],
            Some(Value::Array(models)) => models.as_slice(),
            _ => return Err(invalid()),
        };
        let next = match wire.get("nextPageToken") {
            None | Some(Value::Null) => None,
            Some(Value::String(token)) if token.is_empty() => None,
            Some(Value::String(token)) => Some(token.clone()),
            _ => return Err(invalid()),
        };
        let mut names = HashSet::new();
        let mut models = Vec::new();
        for model in data {
            let name = nonempty(&model["name"])
                .filter(|name| resource_name(name))
                .ok_or_else(invalid)?;
            if !names.insert(name)
                || nonempty(&model["baseModelId"]).is_none()
                || nonempty(&model["version"]).is_none()
                || model
                    .get("displayName")
                    .is_some_and(|v| !v.is_null() && !v.is_string())
                || model
                    .get("thinking")
                    .is_some_and(|v| !v.is_null() && !v.is_boolean())
                || ["inputTokenLimit", "outputTokenLimit"].iter().any(|key| {
                    model
                        .get(*key)
                        .is_some_and(|v| !v.is_null() && v.as_u64().is_none())
                })
                || model.get("supportedGenerationMethods").is_some_and(|v| {
                    !v.is_null()
                        && v.as_array()
                            .is_none_or(|methods| methods.iter().any(|m| nonempty(m).is_none()))
                })
            {
                return Err(invalid());
            }
            models.push(NativeModel(model.clone()));
        }
        Ok(Self { models, next })
    }
    pub fn models(&self) -> &[NativeModel] {
        &self.models
    }
    pub fn next_page_token(&self) -> Option<&str> {
        self.next.as_deref()
    }
}

/// One bounded, complete catalog; failed pages never become a partial success.
pub struct ModelCatalog {
    models: BTreeMap<String, NativeModel>,
    cursors: HashSet<String>,
    limit: usize,
    complete: bool,
}
impl ModelCatalog {
    pub fn new(max_models: usize) -> ProviderResult<Self> {
        if max_models == 0 {
            return Err(ProviderError::new(400, "invalid_catalog_limit"));
        }
        Ok(Self {
            models: BTreeMap::new(),
            cursors: HashSet::new(),
            limit: max_models,
            complete: false,
        })
    }
    pub fn append(&mut self, page: ModelsPage) -> ProviderResult<Option<String>> {
        if self.complete
            || page.models.len() > self.limit.saturating_sub(self.models.len())
            || page
                .models
                .iter()
                .any(|model| self.models.contains_key(model.name()))
            || page
                .next
                .as_ref()
                .is_some_and(|cursor| self.cursors.contains(cursor))
        {
            return Err(invalid());
        }
        if let Some(cursor) = &page.next {
            self.cursors.insert(cursor.clone());
        } else {
            self.complete = true;
        }
        self.models
            .extend(page.models.into_iter().map(|m| (m.name().to_owned(), m)));
        Ok(page.next)
    }
    pub fn finish(self) -> ProviderResult<Vec<NativeModel>> {
        if !self.complete {
            return Err(invalid());
        }
        Ok(self.models.into_values().collect())
    }
}
