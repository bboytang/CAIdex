//! OpenRouter Models and stateless Classic Responses, using shared transport.
//! No hosted executor, model-name capability guesses or inference retries.
mod catalog;
mod config;
mod request;

pub use caidex_provider_custom::{ClientOptions, Error, Limits};
pub use catalog::NativeModel;
pub use config::OpenRouterConfig;

use caidex_credentials::{Broker, SecretStore};
use caidex_model_core::{
    CanonicalRequest, CredentialRequirement, EvidenceSource, ModelCapabilities, ModelMetadata,
    ModelProvider, ProviderError, ProviderFuture, ProviderResponse, ProviderResult,
    ProviderStreamEvent, RequestContext, StreamingResponse,
};
use caidex_provider_custom::{ConfiguredModel, CustomResponses, CustomResponsesProvider};
use futures_util::{StreamExt, stream};
use std::{collections::HashSet, sync::Arc};

pub struct OpenRouterProvider<S: SecretStore> {
    responses: CustomResponsesProvider<S>,
    models_endpoint: CustomResponses,
    limits: Limits,
}
impl<S: SecretStore + 'static> OpenRouterProvider<S> {
    pub fn new(
        config: OpenRouterConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::with_options(config, models, broker, limits, ClientOptions::default())
    }
    pub fn with_options(
        config: OpenRouterConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
        options: ClientOptions,
    ) -> Result<Self, Error> {
        if models
            .iter()
            .any(|m| m.dialects != [caidex_model_core::ResponsesDialect::Classic])
        {
            return Err(Error::InvalidRoute);
        }
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
            responses: CustomResponsesProvider::with_options(
                routes,
                broker,
                limits.clone(),
                options,
            )?,
            models_endpoint,
            limits,
        })
    }
    /// Complete native model inventory, including non-Responses models. It is
    /// availability metadata, never a Codex compatibility or capability test.
    pub async fn discover_models(
        &self,
        context: RequestContext,
    ) -> ProviderResult<Vec<NativeModel>> {
        if context.headers.iter().next().is_some() {
            return Err(ProviderError::new(400, "openrouter_unsupported_context"));
        }
        catalog::parse(
            self.responses
                .get_json(&self.models_endpoint, context)
                .await?,
        )
    }
    fn prepare(
        &self,
        request: CanonicalRequest,
        context: &RequestContext,
    ) -> ProviderResult<CanonicalRequest> {
        self.responses.metadata(request.model())?;
        if context.headers.iter().next().is_some() {
            return Err(ProviderError::new(400, "openrouter_unsupported_context"));
        }
        request::compile(request, self.limits.request_bytes)
    }
}
impl<S: SecretStore + 'static> ModelProvider for OpenRouterProvider<S> {
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
            let response = self
                .responses
                .create_response(self.prepare(request, &context)?, context)
                .await?;
            request::output(response.response.wire())?;
            request::headers(&response.headers)?;
            Ok(response)
        })
    }
    fn stream_response(
        &self,
        request: CanonicalRequest,
        context: RequestContext,
    ) -> ProviderFuture<'_, StreamingResponse> {
        Box::pin(async move {
            let mut response = self
                .responses
                .stream_response(self.prepare(request, &context)?, context)
                .await?;
            request::headers(&response.headers)?;
            response.events = Box::pin(stream::unfold(Some(response.events), |state| async move {
                let mut events = state?;
                match events.next().await? {
                    Ok(event) => {
                        if let ProviderStreamEvent::Model(model) = &event
                            && let Err(error) = request::output(model.response.wire())
                        {
                            return Some((Err(error), None));
                        }
                        Some((Ok(event), Some(events)))
                    }
                    Err(error) => Some((Err(error), None)),
                }
            }));
            Ok(response)
        })
    }
}
