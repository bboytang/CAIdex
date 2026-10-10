//! Executor-configured standard Chat Completions, projected into Responses.
//! Stateless text/function-tool protocol; no execution or approval authority.
mod request;
mod response;
mod tools;

use caidex_credentials::{Broker, CredentialRef, SecretStore};
use caidex_model_core::{
    CanonicalRequest, CredentialRequirement, ModelCapabilities, ModelMetadata, ModelProvider,
    ProviderError, ProviderFuture, ProviderResponse, ProviderResult, RequestContext,
    ResponsesDialect, StreamingResponse,
};
pub use caidex_provider_custom::{ClientOptions, Error, Limits};
use caidex_provider_custom::{ConfiguredModel, CustomResponses, CustomResponsesProvider};
use std::sync::Arc;

/// Exact /chat/completions endpoint and credential are owned by the executor.
/// HTTPS or literal-loopback HTTP only, no redirects or implicit key discovery.
pub struct ChatCompletionsConfig {
    endpoint: String,
    credential: Option<CredentialRef>,
    runtime_context: bool,
    lite: bool,
    no_reasoning_runtime: bool,
    grammar_prompt_mapping: bool,
}
impl ChatCompletionsConfig {
    pub fn new(endpoint: &str, credential: Option<CredentialRef>) -> Result<Self, Error> {
        CustomResponses::new(endpoint, credential.clone())?;
        if !endpoint.ends_with("/chat/completions") {
            return Err(Error::InvalidEndpoint);
        }
        Ok(Self {
            endpoint: endpoint.into(),
            credential,
            runtime_context: false,
            lite: false,
            no_reasoning_runtime: false,
            grammar_prompt_mapping: false,
        })
    }
    /// Explicitly consume fixed Runtime context locally; it never selects native
    /// authentication, URL, model routing or cache semantics.
    pub fn with_local_runtime_context(mut self) -> Self {
        self.runtime_context = true;
        self
    }
    /// Fixed Runtime always asks to include encrypted reasoning. In this
    /// explicit no-reasoning profile the optional include has an empty result;
    /// non-none effort, summaries and native reasoning outputs still fail closed.
    pub fn with_no_reasoning_runtime(mut self) -> Self {
        self.runtime_context = true;
        self.no_reasoning_runtime = true;
        self
    }
    /// Explicitly translate a custom input grammar into function description
    /// guidance. Native grammar enforcement is unsupported; this opt-in is
    /// a prompt mapping only, matching the existing native adapter boundary.
    pub fn with_grammar_prompt_mapping(mut self) -> Self {
        self.grammar_prompt_mapping = true;
        self
    }
    /// Prompt-only Lite tools compile to function declarations. This does not
    /// imply native Responses Lite, grammar, discovery or reasoning support.
    pub fn with_lite(mut self) -> Self {
        self.lite = true;
        self
    }
}
impl std::fmt::Debug for ChatCompletionsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ChatCompletionsConfig([PROFILE OMITTED])")
    }
}

pub struct ChatCompletionsProvider<S: SecretStore> {
    transport: CustomResponsesProvider<S>,
    endpoint: CustomResponses,
    config: ChatCompletionsConfig,
    limits: Limits,
}
impl<S: SecretStore + 'static> ChatCompletionsProvider<S> {
    pub fn new(
        config: ChatCompletionsConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::with_options(config, models, broker, limits, ClientOptions::default())
    }
    pub fn with_options(
        config: ChatCompletionsConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
        options: ClientOptions,
    ) -> Result<Self, Error> {
        if !config.lite
            && models
                .iter()
                .any(|m| m.dialects.contains(&ResponsesDialect::Lite))
        {
            return Err(Error::InvalidRoute);
        }
        let routes = models
            .into_iter()
            .map(|metadata| {
                ConfiguredModel::new(
                    metadata.id.clone(),
                    metadata.native_model.clone(),
                    metadata.dialects.clone(),
                    CustomResponses::new(&config.endpoint, config.credential.clone())?,
                )?
                .with_metadata(metadata)
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let transport =
            CustomResponsesProvider::with_options(routes, broker, limits.clone(), options)?;
        let endpoint = CustomResponses::new(&config.endpoint, config.credential.clone())?;
        Ok(Self {
            transport,
            endpoint,
            config,
            limits,
        })
    }
    fn prepare(
        &self,
        request: &CanonicalRequest,
        context: &RequestContext,
    ) -> ProviderResult<request::Prepared> {
        let metadata = self.transport.metadata(request.model())?;
        if !metadata.dialects.contains(&request.dialect()) {
            return Err(ProviderError::new(400, "unsupported_dialect"));
        }
        if serde_json::to_vec(request.wire()).expect("JSON").len() > self.limits.request_bytes {
            return Err(ProviderError::new(413, "invalid_or_oversized_body"));
        }
        request::prepare(request, &metadata, context, &self.config)
    }
}
impl<S: SecretStore + 'static> ModelProvider for ChatCompletionsProvider<S> {
    fn list_models(&self) -> ProviderFuture<'_, Vec<ModelMetadata>> {
        self.transport.list_models()
    }
    fn metadata(&self, model: &str) -> ProviderResult<ModelMetadata> {
        self.transport.metadata(model)
    }
    fn capabilities(&self, model: &str) -> ProviderResult<ModelCapabilities> {
        self.transport.capabilities(model)
    }
    fn credential_requirements(&self, model: &str) -> ProviderResult<CredentialRequirement> {
        self.transport.credential_requirements(model)
    }
    fn create_response(
        &self,
        request: CanonicalRequest,
        context: RequestContext,
    ) -> ProviderFuture<'_, ProviderResponse> {
        Box::pin(async move {
            if request.is_streaming() {
                return Err(ProviderError::new(400, "stream_endpoint_mismatch"));
            }
            let prepared = self.prepare(&request, &context)?;
            let native = self
                .transport
                .post_json(
                    &self.endpoint,
                    prepared.wire.clone(),
                    request::local_context(context),
                )
                .await?;
            let response = response::project(native, &prepared)?;
            if serde_json::to_vec(response.wire()).expect("JSON").len() > self.limits.response_bytes
            {
                return Err(ProviderError::new(502, "provider_response_too_large"));
            }
            Ok(ProviderResponse {
                response,
                headers: Default::default(),
            })
        })
    }
    fn stream_response(
        &self,
        request: CanonicalRequest,
        context: RequestContext,
    ) -> ProviderFuture<'_, StreamingResponse> {
        Box::pin(async move {
            if !request.is_streaming() {
                return Err(ProviderError::new(400, "stream_endpoint_mismatch"));
            }
            let prepared = self.prepare(&request, &context)?;
            let cancellation = context.cancellation.clone();
            let deadline = context
                .deadline
                .unwrap_or(std::time::Instant::now() + self.limits.total_timeout)
                .min(std::time::Instant::now() + self.limits.total_timeout);
            let native = self
                .transport
                .post_sse(
                    &self.endpoint,
                    prepared.wire.clone(),
                    request::local_context(context),
                )
                .await?;
            Ok(StreamingResponse {
                headers: native.headers,
                events: response::project_stream(
                    native.events,
                    prepared,
                    cancellation,
                    deadline,
                    self.limits.response_bytes,
                ),
            })
        })
    }
}
