//! DeepSeek native Models and foreground Responses, using the shared transport.
//! No implicit keys, model-name capability inference or tool executor.
mod catalog;
mod config;
mod request;

pub use caidex_provider_custom::{ClientOptions, Error, Limits};
pub use catalog::NativeModel;
pub use config::DeepSeekConfig;

use caidex_credentials::{Broker, SecretStore};
use caidex_model_core::{
    CanonicalRequest, CapabilitySupport, ContextHeaders, CredentialRequirement, EvidenceSource,
    ModelCapabilities, ModelMetadata, ModelProvider, ProviderError, ProviderFuture,
    ProviderResponse, ProviderResult, ProviderStreamEvent, RequestContext, ResponsesDialect,
    StreamingResponse,
};
use caidex_provider_custom::{ConfiguredModel, CustomResponses, CustomResponsesProvider};
use futures_util::{StreamExt, stream};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub struct DeepSeekProvider<S: SecretStore> {
    responses: CustomResponsesProvider<S>,
    models_endpoint: CustomResponses,
    request_bytes: usize,
    runtime_context: bool,
    verbosity_instructions: HashMap<String, String>,
}
impl<S: SecretStore + 'static> DeepSeekProvider<S> {
    pub fn new(
        config: DeepSeekConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::with_options(config, models, broker, limits, ClientOptions::default())
    }
    pub fn with_options(
        config: DeepSeekConfig,
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
        let request_bytes = limits.request_bytes;
        Ok(Self {
            responses: CustomResponsesProvider::with_options(routes, broker, limits, options)?,
            models_endpoint,
            request_bytes,
            runtime_context: false,
            verbosity_instructions: HashMap::new(),
        })
    }
    /// Explicit local attribution and leading developer-to-system policy.
    /// Does not implement native caching, persistence or reasoning replay.
    pub fn with_runtime_context(mut self) -> Self {
        self.runtime_context = true;
        self
    }
    /// Executor-owned guidance, not a native verbosity scale guarantee.
    pub fn with_verbosity_instruction(
        mut self,
        verbosity: String,
        instruction: String,
    ) -> ProviderResult<Self> {
        if !matches!(verbosity.as_str(), "low" | "medium" | "high")
            || instruction.trim().is_empty()
            || self.verbosity_instructions.contains_key(&verbosity)
        {
            return Err(ProviderError::new(
                400,
                "deepseek_invalid_verbosity_mapping",
            ));
        }
        self.verbosity_instructions.insert(verbosity, instruction);
        Ok(self)
    }
    fn native_context(&self, mut context: RequestContext) -> ProviderResult<RequestContext> {
        if context.headers.iter().any(|(name, _)| {
            !self.runtime_context
                || !matches!(
                    name,
                    "session_id" | "x-client-request-id" | "x-codex-turn-metadata"
                )
        }) {
            return Err(ProviderError::new(400, "deepseek_unsupported_context"));
        }
        context.headers = ContextHeaders::default();
        Ok(context)
    }
    pub async fn discover_models(
        &self,
        context: RequestContext,
    ) -> ProviderResult<Vec<NativeModel>> {
        let context = self.native_context(context)?;
        catalog::parse(
            self.responses
                .get_json(&self.models_endpoint, context)
                .await?,
        )
    }
    fn prepare(&self, request: CanonicalRequest) -> ProviderResult<CanonicalRequest> {
        let model = self.responses.metadata(request.model())?;
        if !model.dialects.contains(&request.dialect()) {
            return Err(ProviderError::new(400, "unsupported_dialect"));
        }
        if model.capabilities.text == CapabilitySupport::Unsupported {
            return Err(ProviderError::new(400, "unsupported_text"));
        }
        request::compile(
            request,
            self.request_bytes,
            self.runtime_context,
            &self.verbosity_instructions,
        )
    }
}
fn response_headers(headers: &ContextHeaders) -> ProviderResult<()> {
    if headers.get("x-codex-turn-state").is_some() {
        return Err(ProviderError::new(502, "deepseek_unsupported_turn_state"));
    }
    Ok(())
}
fn output(wire: &Value) -> ProviderResult<()> {
    let tool = |v: &Value| {
        matches!(
            v["type"].as_str(),
            Some("function_call" | "custom_tool_call" | "tool_search_call")
        )
    };
    if tool(wire)
        || tool(&wire["item"])
        || wire["output"]
            .as_array()
            .is_some_and(|items| items.iter().any(tool))
        || wire["response"]["output"]
            .as_array()
            .is_some_and(|items| items.iter().any(tool))
        || wire["type"].as_str().is_some_and(|kind| {
            kind.starts_with("response.function_call_")
                || kind.starts_with("response.custom_tool_call_")
                || kind.starts_with("response.tool_search_call")
        })
    {
        return Err(ProviderError::new(502, "deepseek_unexpected_tool"));
    }
    Ok(())
}
impl<S: SecretStore + 'static> ModelProvider for DeepSeekProvider<S> {
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
            let context = self.native_context(context)?;
            let response = self
                .responses
                .create_response(self.prepare(request)?, context)
                .await?;
            response_headers(&response.headers)?;
            output(response.response.wire())?;
            Ok(response)
        })
    }
    fn stream_response(
        &self,
        request: CanonicalRequest,
        context: RequestContext,
    ) -> ProviderFuture<'_, StreamingResponse> {
        Box::pin(async move {
            let context = self.native_context(context)?;
            let mut response = self
                .responses
                .stream_response(self.prepare(request)?, context)
                .await?;
            response_headers(&response.headers)?;
            response.events = Box::pin(stream::unfold(Some(response.events), |state| async move {
                let mut events = state?;
                match events.next().await? {
                    Ok(event) => {
                        if let ProviderStreamEvent::Model(model) = &event
                            && let Err(error) = output(model.response.wire())
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
