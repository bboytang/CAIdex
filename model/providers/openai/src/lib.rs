//! OpenAI Models and Responses adapter. No implicit environment credentials,
//! hosted agent executor, model-name capability guesses or inference retries.
mod catalog;
mod config;

pub use caidex_provider_custom::{ClientOptions, Error, Limits};
pub use catalog::NativeModel;
pub use config::OpenAiConfig;

use caidex_credentials::{Broker, SecretStore};
use caidex_model_core::{
    CanonicalRequest, CredentialRequirement, EvidenceSource, ModelCapabilities, ModelMetadata,
    ModelProvider, ProviderError, ProviderFuture, ProviderResponse, ProviderResult, RequestContext,
    StreamingResponse,
};
use caidex_provider_custom::{ConfiguredModel, CustomResponses, CustomResponsesProvider};
use std::{collections::HashSet, sync::Arc};

pub struct OpenAiProvider<S: SecretStore> {
    responses: CustomResponsesProvider<S>,
    models_endpoint: CustomResponses,
}
impl<S: SecretStore + 'static> OpenAiProvider<S> {
    pub fn new(
        config: OpenAiConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::with_options(config, models, broker, limits, ClientOptions::default())
    }
    pub fn with_options(
        config: OpenAiConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
        options: ClientOptions,
    ) -> Result<Self, Error> {
        let models_endpoint = config.endpoint("models")?;
        let routes = models
            .into_iter()
            .map(|model| {
                ConfiguredModel::new(
                    model.id.clone(),
                    model.native_model.clone(),
                    model.dialects.clone(),
                    config.endpoint("responses")?,
                )?
                .with_metadata(model)
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Self {
            responses: CustomResponsesProvider::with_options(routes, broker, limits, options)?,
            models_endpoint,
        })
    }
    /// Complete native model inventory, including non-Responses models. It is
    /// availability metadata, never a Codex compatibility or capability test.
    pub async fn discover_models(
        &self,
        context: RequestContext,
    ) -> ProviderResult<Vec<NativeModel>> {
        catalog::parse(
            self.responses
                .get_json(&self.models_endpoint, context)
                .await?,
        )
    }
}
impl<S: SecretStore + 'static> ModelProvider for OpenAiProvider<S> {
    fn list_models(&self) -> ProviderFuture<'_, Vec<ModelMetadata>> {
        Box::pin(async {
            let available: HashSet<_> = self
                .discover_models(RequestContext::default())
                .await?
                .into_iter()
                .map(|model| model.id().to_owned())
                .collect();
            Ok(self
                .responses
                .list_models()
                .await?
                .into_iter()
                .filter_map(|mut model| {
                    if !available.contains(&model.native_model) {
                        return None;
                    }
                    model.source = EvidenceSource::ProviderCatalog;
                    Some(model)
                })
                .collect())
        })
    }
    fn metadata(&self, model: &str) -> ProviderResult<ModelMetadata> {
        self.responses.metadata(model)
    }
    fn capabilities(&self, model: &str) -> ProviderResult<ModelCapabilities> {
        self.responses.capabilities(model)
    }
    fn credential_requirements(&self, model: &str) -> ProviderResult<CredentialRequirement> {
        self.responses.credential_requirements(model)
    }
    fn create_response(
        &self,
        request: CanonicalRequest,
        context: RequestContext,
    ) -> ProviderFuture<'_, ProviderResponse> {
        Box::pin(async move {
            self.responses
                .create_response(stateless_request(request)?, context)
                .await
        })
    }
    fn stream_response(
        &self,
        request: CanonicalRequest,
        context: RequestContext,
    ) -> ProviderFuture<'_, StreamingResponse> {
        Box::pin(async move {
            self.responses
                .stream_response(stateless_request(request)?, context)
                .await
        })
    }
}

fn stateless_request(request: CanonicalRequest) -> ProviderResult<CanonicalRequest> {
    let mut wire = request.wire().clone();
    // Server background generation survives an HTTP disconnect and requires a
    // separate cancel API; it is outside this foreground inference contract.
    if wire["background"] == true {
        return Err(ProviderError::new(400, "unsupported_background_generation"));
    }
    if wire
        .get("background")
        .is_some_and(|value| !value.is_null() && !value.is_boolean())
        || wire
            .get("store")
            .is_some_and(|value| !value.is_null() && !value.is_boolean())
    {
        return Err(ProviderError::new(400, "invalid_model_request"));
    }
    // Explicit true remains an opt-in. Absent/null never silently enables the
    // API's stored-response default; opaque reasoning remains caller-owned.
    if wire.get("store").is_none_or(serde_json::Value::is_null) {
        wire["store"] = false.into();
    }
    CanonicalRequest::new(wire, request.dialect())
        .map_err(|_| ProviderError::new(400, "invalid_model_request"))
}
