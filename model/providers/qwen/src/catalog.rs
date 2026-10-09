use caidex_model_core::{ProviderError, ProviderResult};
use serde::Serialize;
use serde_json::Value;
use std::fmt;

pub(crate) const PAGE_SIZE: u64 = 20;
pub(crate) const MAX_PAGES: u64 = 256;

#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct NativeModel(Value);
impl NativeModel {
    pub fn id(&self) -> &str {
        self.0["model"].as_str().expect("validated model ID")
    }
    /// Raw optional declarations, nulls and extensions, never live compatibility.
    pub fn wire(&self) -> &Value {
        &self.0
    }
}
impl fmt::Debug for NativeModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeModel([WIRE OMITTED])")
    }
}
pub(crate) fn invalid() -> ProviderError {
    ProviderError::new(502, "qwen_invalid_model_catalog")
}
pub(crate) fn parse(wire: Value, page: u64) -> ProviderResult<(u64, Vec<NativeModel>)> {
    if wire["success"] != true
        || wire["output"]["page_no"] != page
        || wire["output"]["page_size"] != PAGE_SIZE
    {
        return Err(invalid());
    }
    let total = wire["output"]["total"].as_u64().ok_or_else(invalid)?;
    if total > MAX_PAGES * PAGE_SIZE {
        return Err(ProviderError::new(413, "qwen_catalog_limit"));
    }
    let items = wire["output"]["models"].as_array().ok_or_else(invalid)?;
    if items.len() as u64 != total.saturating_sub((page - 1) * PAGE_SIZE).min(PAGE_SIZE) {
        return Err(invalid());
    }
    let mut models = Vec::new();
    for model in items {
        if model["model"]
            .as_str()
            .is_none_or(|id| id.trim().is_empty() || id.chars().any(char::is_control))
        {
            return Err(invalid());
        }
        // The official sample omits provider/capabilities and permits nulls.
        // Do not require OpenAI's id/object/created/owned_by or guess support.
        models.push(NativeModel(model.clone()));
    }
    Ok((total, models))
}
