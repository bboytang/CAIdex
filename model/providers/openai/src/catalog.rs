use caidex_model_core::{ProviderError, ProviderResult};
use serde::Serialize;
use serde_json::Value;
use std::{collections::HashSet, fmt};

#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct NativeModel(Value);
impl NativeModel {
    pub fn id(&self) -> &str {
        self.0["id"].as_str().expect("validated ID")
    }
    pub fn owned_by(&self) -> &str {
        self.0["owned_by"].as_str().expect("validated owner")
    }
    pub fn created(&self) -> u64 {
        self.0["created"].as_u64().expect("validated time")
    }
    pub fn shutdown_date(&self) -> Option<&str> {
        self.0["shutdown_date"].as_str()
    }
    /// Unknown model metadata stays available without inventing capabilities.
    pub fn wire(&self) -> &Value {
        &self.0
    }
}
impl fmt::Debug for NativeModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeModel([WIRE OMITTED])")
    }
}
pub(crate) fn parse(wire: Value) -> ProviderResult<Vec<NativeModel>> {
    let invalid = || ProviderError::new(502, "provider_invalid_model_catalog");
    if wire["object"] != "list" {
        return Err(invalid());
    }
    let mut ids = HashSet::new();
    let mut models = Vec::new();
    for model in wire["data"].as_array().ok_or_else(invalid)? {
        let id = model["id"]
            .as_str()
            .filter(|id| !id.trim().is_empty() && !id.chars().any(char::is_control))
            .ok_or_else(invalid)?;
        if model["object"] != "model"
            || !ids.insert(id.to_owned())
            || model["created"].as_u64().is_none()
            || !model["owned_by"].is_string()
            || model
                .get("shutdown_date")
                .is_some_and(|date| !date.is_null() && !date.is_string())
        {
            return Err(invalid());
        }
        models.push(NativeModel(model.clone()));
    }
    models.sort_by(|a, b| a.id().cmp(b.id()));
    Ok(models)
}
