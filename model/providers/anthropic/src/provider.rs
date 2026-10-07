use crate::{
    AnthropicClient, MessagesRequest, NativeModel, ProjectedStreamingResponse, ReasoningMapping,
    RequestOptions, ResponsesProjection, ServiceTierMapping, SummaryMapping, ThinkingContext,
};
use caidex_credentials::SecretStore;
use caidex_model_core::{
    CanonicalRequest, CapabilitySupport, ContextHeaders, CredentialRequirement, EvidenceSource,
    ModelCapabilities, ModelMetadata, ModelProvider, ProviderError, ProviderFuture,
    ProviderResponse, ProviderResult, RequestContext, StreamingResponse,
};
use std::collections::{BTreeMap, BTreeSet};

/// Executor-owned, explicit model contract. Metadata is a declaration, not live
/// compatibility evidence. Native output budgets never come from caller JSON.
pub struct AnthropicModel {
    pub metadata: ModelMetadata,
    pub max_tokens: u64,
    pub max_tools: usize,
    pub retain_runtime_metadata: bool,
    pub service_tier_mappings: Vec<ServiceTierMapping>,
    pub supports_system_messages: bool,
    pub supports_structured_outputs: bool,
    pub summary_mappings: Vec<SummaryMapping>,
    pub thinking_context: Option<ThinkingContext>,
    pub reasoning_mappings: Vec<ReasoningMapping>,
}
impl AnthropicModel {
    pub fn new(metadata: ModelMetadata, max_tokens: u64, max_tools: usize) -> Self {
        Self {
            metadata,
            max_tokens,
            max_tools,
            retain_runtime_metadata: false,
            service_tier_mappings: Vec::new(),
            supports_system_messages: false,
            supports_structured_outputs: false,
            summary_mappings: Vec::new(),
            thinking_context: None,
            reasoning_mappings: Vec::new(),
        }
    }
    fn compile(
        &self,
        request: &CanonicalRequest,
        max_bytes: usize,
    ) -> ProviderResult<MessagesRequest> {
        if !self.metadata.dialects.contains(&request.dialect()) {
            return Err(ProviderError::new(400, "unsupported_dialect"));
        }
        if request.is_streaming()
            && self.metadata.capabilities.streaming == CapabilitySupport::Unsupported
        {
            return Err(ProviderError::new(400, "unsupported_streaming"));
        }
        MessagesRequest::from_responses_with_options(
            request,
            &self.metadata.native_model,
            self.max_tokens,
            max_bytes,
            self.max_tools,
            &RequestOptions {
                retain_runtime_metadata: self.retain_runtime_metadata,
                service_tier_mappings: &self.service_tier_mappings,
                supports_system_messages: self.supports_system_messages,
                supports_structured_outputs: self.supports_structured_outputs,
                summary_mappings: &self.summary_mappings,
                thinking_context: self.thinking_context,
                reasoning_mappings: &self.reasoning_mappings,
            },
        )
    }
}

pub struct AnthropicProvider<S: SecretStore> {
    client: AnthropicClient<S>,
    models: BTreeMap<String, AnthropicModel>,
    max_models: usize,
}
impl<S: SecretStore + 'static> AnthropicProvider<S> {
    /// Reuse the native client's exact credential, limits, TLS and concurrency
    /// policy; no separate inference transport or mutable caller routing.
    pub fn new(
        client: AnthropicClient<S>,
        models: Vec<AnthropicModel>,
        max_models: usize,
    ) -> ProviderResult<Self> {
        if models.is_empty() || max_models == 0 {
            return Err(ProviderError::new(400, "invalid_anthropic_profile"));
        }
        let mut profiles = BTreeMap::new();
        for model in models {
            if model.metadata.validate().is_err()
                || model.max_tokens == 0
                || model
                    .metadata
                    .capabilities
                    .output_limit
                    .is_some_and(|limit| model.max_tokens > limit)
            {
                return Err(ProviderError::new(400, "invalid_anthropic_profile"));
            }
            // Existing compiler validates duplicate mappings even when no
            // reasoning/tier is requested; reuse it instead of a second validator.
            let request = CanonicalRequest::new(
                serde_json::json!({"model":model.metadata.id,"input":"profile validation"}),
                model.metadata.dialects[0],
            )
            .map_err(|_| ProviderError::new(400, "invalid_anthropic_profile"))?;
            model.compile(&request, client.limits().request_bytes)?;
            if profiles.insert(model.metadata.id.clone(), model).is_some() {
                return Err(ProviderError::new(400, "invalid_anthropic_profile"));
            }
        }
        Ok(Self {
            client,
            models: profiles,
            max_models,
        })
    }
    pub async fn discover_models(
        &self,
        context: RequestContext,
    ) -> ProviderResult<Vec<NativeModel>> {
        self.client.discover_models(self.max_models, context).await
    }
    fn model(&self, model: &str) -> ProviderResult<&AnthropicModel> {
        self.models
            .get(model)
            .ok_or_else(|| ProviderError::new(404, "unknown_model"))
    }
}
impl<S: SecretStore + 'static> ModelProvider for AnthropicProvider<S> {
    fn list_models(&self) -> ProviderFuture<'_, Vec<ModelMetadata>> {
        Box::pin(async {
            let available: BTreeSet<_> = self
                .discover_models(RequestContext::default())
                .await?
                .into_iter()
                .map(|model| model.id().to_owned())
                .collect();
            Ok(self
                .models
                .values()
                .filter(|model| available.contains(&model.metadata.native_model))
                .map(|model| {
                    let mut metadata = model.metadata.clone();
                    metadata.source = EvidenceSource::ProviderCatalog;
                    metadata
                })
                .collect())
        })
    }
    fn metadata(&self, model: &str) -> ProviderResult<ModelMetadata> {
        Ok(self.model(model)?.metadata.clone())
    }
    fn capabilities(&self, model: &str) -> ProviderResult<ModelCapabilities> {
        Ok(self.model(model)?.metadata.capabilities.clone())
    }
    fn credential_requirements(&self, model: &str) -> ProviderResult<CredentialRequirement> {
        self.model(model)?;
        Ok(CredentialRequirement::ApiKey {
            reference: self.client.credential().clone(),
        })
    }
    fn create_response(
        &self,
        request: CanonicalRequest,
        context: RequestContext,
    ) -> ProviderFuture<'_, ProviderResponse> {
        Box::pin(async move {
            if request.is_streaming() {
                return Err(ProviderError::new(400, "invalid_model_request"));
            }
            let model = self.model(request.model())?;
            let compiled = model.compile(&request, self.client.limits().request_bytes)?;
            let (wire, tools) = compiled.into_parts();
            let native = self
                .client
                .create_message(&model.metadata.native_model, wire, context)
                .await?;
            if native.model() != model.metadata.native_model {
                return Err(ProviderError::new(502, "anthropic_response_model_mismatch"));
            }
            let response =
                native.to_responses_with_tools(&tools, self.client.limits().response_bytes)?;
            if response.wire().to_string().len() > self.client.limits().response_bytes {
                return Err(ProviderError::new(502, "anthropic_projection_too_large"));
            }
            Ok(ProviderResponse {
                response,
                headers: ContextHeaders::default(),
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
                return Err(ProviderError::new(400, "invalid_model_request"));
            }
            let model = self.model(request.model())?;
            let compiled = model.compile(&request, self.client.limits().request_bytes)?;
            let (wire, tools) = compiled.into_parts();
            let projection = ResponsesProjection::new(
                model.metadata.native_model.clone(),
                tools,
                self.client.limits().response_bytes,
            )?;
            let native = self
                .client
                .stream_message(&model.metadata.native_model, wire, context)
                .await?;
            Ok(StreamingResponse {
                events: Box::pin(ProjectedStreamingResponse::new(native, projection)),
                headers: ContextHeaders::default(),
            })
        })
    }
}
