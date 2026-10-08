//! Stateless Ollama Responses. Inference only, with the existing bounded client.
mod config;
mod models;
mod request;

pub use caidex_provider_custom::{ClientOptions, Error, Limits, NativeModel};
pub use config::OllamaConfig;
pub use models::ModelDetails;

use caidex_credentials::{Broker, SecretStore};
use caidex_model_core::{
    CanonicalRequest, CapabilitySupport, ContextHeaders, CredentialRequirement, EvidenceSource,
    ModelCapabilities, ModelMetadata, ModelProvider, ProviderError, ProviderFuture,
    ProviderResponse, ProviderResult, RequestContext, ResponsesDialect, StreamingResponse,
};
use caidex_provider_custom::{ConfiguredModel, CustomResponses, CustomResponsesProvider};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub struct OllamaProvider<S: SecretStore> {
    responses: CustomResponsesProvider<S>,
    models_endpoint: CustomResponses,
    show_endpoint: Option<CustomResponses>,
    model_details: HashMap<String, ModelDetails>,
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
        mut config: OllamaConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
        options: ClientOptions,
    ) -> Result<Self, Error> {
        let models_endpoint = config.endpoint("models")?;
        let show_endpoint = config.take_show_endpoint();
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
            show_endpoint,
            model_details: HashMap::new(),
        })
    }
    /// Install a fixed executor-owned snapshot; does not fetch automatically or
    /// turn catalog/fixture success into a live compatibility report.
    pub fn with_model_details(
        mut self,
        models: Vec<(String, ModelDetails)>,
    ) -> ProviderResult<Self> {
        for (alias, details) in models {
            let metadata = self.responses.metadata(&alias)?;
            if metadata.native_model != details.native_model()
                || self.model_details.contains_key(&alias)
            {
                return Err(ProviderError::new(400, "ollama_invalid_model_binding"));
            }
            self.model_details.insert(alias, details);
        }
        Ok(self)
    }
    pub async fn show_model(
        &self,
        model: &str,
        context: RequestContext,
    ) -> ProviderResult<ModelDetails> {
        context_guard(&context)?;
        let metadata = self.responses.metadata(model)?;
        let endpoint = self
            .show_endpoint
            .as_ref()
            .ok_or_else(|| ProviderError::new(400, "ollama_show_endpoint_not_configured"))?;
        ModelDetails::parse(
            metadata.native_model.clone(),
            self.responses
                .post_json(
                    endpoint,
                    serde_json::json!({"model":metadata.native_model}),
                    context,
                )
                .await?,
        )
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
        let details = self.model_details.get(request.model());
        request::compile(request, details, metadata.capabilities.reasoning)
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
            let mut models = Vec::new();
            for model in self.responses.list_models().await? {
                if available.contains(&model.native_model) {
                    let mut model = self.metadata(&model.id)?;
                    model.source = EvidenceSource::ProviderCatalog;
                    models.push(model);
                }
            }
            Ok(models)
        })
    }
    fn metadata(&self, model: &str) -> ProviderResult<ModelMetadata> {
        let mut metadata = self.responses.metadata(model)?;
        if let Some(details) = self.model_details.get(model) {
            let declared = details.declared_capabilities();
            for (configured, native) in [
                (&mut metadata.capabilities.text, declared.text),
                (&mut metadata.capabilities.vision, declared.vision),
                (&mut metadata.capabilities.reasoning, declared.reasoning),
                (
                    &mut metadata.capabilities.native_tools,
                    declared.native_tools,
                ),
            ] {
                // An explicit executor restriction cannot be loosened by discovery.
                if *configured != CapabilitySupport::Unsupported
                    && native != CapabilitySupport::Unknown
                {
                    *configured = native;
                }
            }
            metadata.source = EvidenceSource::ProviderCatalog;
        }
        Ok(metadata)
    }
    fn capabilities(&self, model: &str) -> ProviderResult<ModelCapabilities> {
        Ok(self.metadata(model)?.capabilities)
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
