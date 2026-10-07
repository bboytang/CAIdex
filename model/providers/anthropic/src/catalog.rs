use crate::string;
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
    ProviderError::new(502, "anthropic_invalid_model_catalog")
}
#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct NativeModel(Value);
impl NativeModel {
    pub fn id(&self) -> &str {
        self.0["id"].as_str().unwrap()
    }
    pub fn wire(&self) -> &Value {
        &self.0
    }
    /// Only documented provider fields become capability declarations. They
    /// remain catalog evidence, not Codex or commercial inference acceptance.
    pub fn metadata(
        &self,
        alias: &str,
        dialects: Vec<ResponsesDialect>,
    ) -> ProviderResult<ModelMetadata> {
        let support = |key| match self.0["capabilities"][key]["supported"].as_bool() {
            Some(true) => CapabilitySupport::Supported,
            Some(false) => CapabilitySupport::Unsupported,
            None => CapabilitySupport::Unknown,
        };
        let mut metadata = ModelMetadata::configured(alias.into(), self.id().into(), dialects);
        metadata.display_name = self.0["display_name"].as_str().unwrap().into();
        metadata.source = EvidenceSource::ProviderCatalog;
        metadata.capabilities = ModelCapabilities {
            vision: support("image_input"),
            reasoning: support("thinking"),
            structured_output: support("structured_outputs"),
            context_window: self.0["max_input_tokens"]
                .as_u64()
                .filter(|limit| *limit > 0),
            output_limit: self.0["max_tokens"].as_u64().filter(|limit| *limit > 0),
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
        let data = wire["data"].as_array().ok_or_else(invalid)?;
        let more = wire["has_more"].as_bool().ok_or_else(invalid)?;
        let mut ids = HashSet::new();
        let mut models = Vec::new();
        for model in data {
            let id = string(model, "id")
                .filter(|id| !id.chars().any(char::is_control))
                .ok_or_else(invalid)?;
            if model["type"] != "model"
                || !ids.insert(id)
                || string(model, "created_at").is_none()
                || string(model, "display_name").is_none()
                || ["max_input_tokens", "max_tokens"].iter().any(|name| {
                    model
                        .get(*name)
                        .is_some_and(|value| !value.is_null() && value.as_u64().is_none())
                })
                || model
                    .get("capabilities")
                    .is_some_and(|value| !value.is_null() && !value.is_object())
            {
                return Err(invalid());
            }
            for key in ["image_input", "thinking", "structured_outputs"] {
                if model["capabilities"].get(key).is_some_and(|capability| {
                    !capability.is_null()
                        && (!capability.is_object() || !capability["supported"].is_boolean())
                }) {
                    return Err(invalid());
                }
            }
            models.push(NativeModel(model.clone()));
        }
        if models.is_empty() {
            if more
                || wire.get("first_id").is_none_or(|id| !id.is_null())
                || wire.get("last_id").is_none_or(|id| !id.is_null())
            {
                return Err(invalid());
            }
        } else if wire["first_id"] != models[0].id()
            || wire["last_id"] != models.last().unwrap().id()
        {
            return Err(invalid());
        }
        let next = more.then(|| models.last().unwrap().id().to_owned());
        Ok(Self { models, next })
    }
    pub fn models(&self) -> &[NativeModel] {
        &self.models
    }
    pub fn next_after_id(&self) -> Option<&str> {
        self.next.as_deref()
    }
}
/// Bounded paging accumulator; the HTTP adapter must use after_id only in its
/// configured Models endpoint, never interpret a cursor as a provider URL.
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
                .any(|model| self.models.contains_key(model.id()))
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
        self.models.extend(
            page.models
                .into_iter()
                .map(|model| (model.id().to_owned(), model)),
        );
        Ok(page.next)
    }
    pub fn finish(self) -> ProviderResult<Vec<NativeModel>> {
        if !self.complete {
            return Err(invalid());
        }
        Ok(self.models.into_values().collect())
    }
}
