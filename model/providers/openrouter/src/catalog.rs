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
    /// Native declarations and unknown extensions, not live compatibility.
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
    let invalid = || ProviderError::new(502, "openrouter_invalid_model_catalog");
    let valid = |v: &Value| {
        v.as_str()
            .is_some_and(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
    };
    let mut ids = HashSet::new();
    let mut models = Vec::new();
    for model in wire["data"].as_array().ok_or_else(invalid)? {
        if !model.is_object()
            || !valid(&model["id"])
            || !ids.insert(model["id"].as_str().expect("validated ID"))
        {
            return Err(invalid());
        }
        models.push(NativeModel(model.clone()));
    }
    models.sort_by(|a, b| a.id().cmp(b.id()));
    Ok(models)
}
