//! Stateless Ollama Responses. Inference only, with the existing bounded client.
mod config;
mod request;

pub use caidex_provider_custom::{ClientOptions, Error, Limits, NativeModel};
pub use config::OllamaConfig;

use caidex_credentials::{Broker, SecretStore};
use caidex_model_core::{
    CanonicalRequest, CapabilitySupport, ContextHeaders, CredentialRequirement, EvidenceSource,
    ModelCapabilities, ModelMetadata, ModelProvider, ProviderError, ProviderFuture,
    ProviderResponse, ProviderResult, RequestContext, ResponsesDialect, StreamingResponse,
};
use caidex_provider_custom::{ConfiguredModel, CustomResponses, CustomResponsesProvider};
use std::{collections::HashSet, sync::Arc};

pub struct OllamaProvider<S: SecretStore> {
    responses: CustomResponsesProvider<S>,
    models_endpoint: CustomResponses,
}
impl<S: SecretStore + 'static> OllamaProvider<S> {
    pub fn new(
        config: OllamaConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::with_options(config, models, broker, limits, ClientOptions::default())
    }
    pub fn with_options(
        config: OllamaConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
        options: ClientOptions,
    ) -> Result<Self, Error> {
        let models_endpoint = config.endpoint("models")?;
        let routes = models
            .into_iter()
            .map(|model| {
                if model.dialects != [ResponsesDialect::Classic] {
                    return Err(Error::InvalidRoute);
                }
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
    /// Inventory is availability evidence, never a model compatibility report.
    pub async fn discover_models(
        &self,
        context: RequestContext,
    ) -> ProviderResult<Vec<NativeModel>> {
        context_guard(&context)?;
        caidex_provider_custom::parse_model_catalog(
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
        context_guard(context)?;
        let metadata = self.metadata(request.model())?;
        if request.dialect() != ResponsesDialect::Classic {
            return Err(ProviderError::new(400, "unsupported_dialect"));
        }
        if metadata.capabilities.text == CapabilitySupport::Unsupported
            || metadata.capabilities.native_tools == CapabilitySupport::Unsupported
                && request.wire()["tools"]
                    .as_array()
                    .is_some_and(|tools| !tools.is_empty())
        {
            return Err(ProviderError::new(400, "ollama_unsupported_capability"));
        }
        request::compile(request)
    }
}
impl<S: SecretStore + 'static> ModelProvider for OllamaProvider<S> {
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
            response_headers_guard(&response.headers)?;
            Ok(response)
        })
    }
    fn stream_response(
        &self,
        request: CanonicalRequest,
        context: RequestContext,
    ) -> ProviderFuture<'_, StreamingResponse> {
        Box::pin(async move {
            let response = self
                .responses
                .stream_response(self.prepare(request, &context)?, context)
                .await?;
            response_headers_guard(&response.headers)?;
            Ok(response)
        })
    }
}

fn context_guard(context: &RequestContext) -> ProviderResult<()> {
    if context.cancellation.is_cancelled() {
        return Err(ProviderError::new(503, "provider_cancelled"));
    }
    if context
        .deadline
        .is_some_and(|deadline| deadline <= std::time::Instant::now())
    {
        return Err(ProviderError::new(504, "provider_timeout"));
    }
    if context.headers.iter().next().is_some() {
        return Err(ProviderError::new(
            400,
            "ollama_unsupported_context_headers",
        ));
    }
    Ok(())
}
fn response_headers_guard(headers: &ContextHeaders) -> ProviderResult<()> {
    if headers.get("x-codex-turn-state").is_some() {
        return Err(ProviderError::new(502, "ollama_unsupported_turn_state"));
    }
    Ok(())
}
