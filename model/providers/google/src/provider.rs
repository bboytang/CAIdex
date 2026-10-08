use crate::{
    GeminiClient, GenerateContentRequest, NativeHistory, NativeModel, ProjectedStreamingResponse,
    RequestOptions, ResponsesProjection,
};
use caidex_credentials::SecretStore;
use caidex_model_core::{
    CanonicalRequest, CapabilitySupport, CredentialRequirement, EvidenceSource, ModelCapabilities,
    ModelMetadata, ModelProvider, ProviderError, ProviderFuture, ProviderResponse, ProviderResult,
    RequestContext, StreamingResponse,
};
use std::collections::{BTreeMap, BTreeSet};

/// Fixed executor-owned profile; declarations are not live compatibility.
pub struct GeminiModel {
    pub metadata: ModelMetadata,
    pub max_tokens: u64,
    pub max_tools: usize,
    pub retain_runtime_metadata: bool,
    /// Local output cardinality guard; not a native generation guarantee.
    pub enforce_single_tool_call: bool,
    pub service_tier_mappings: Vec<crate::ServiceTierMapping>,
    pub verbosity_mappings: Vec<crate::VerbosityMapping>,
    pub image_mime_types: Vec<String>,
    pub tool_result_image_mime_types: Vec<String>,
    pub image_detail_mappings: Vec<crate::ImageDetailMapping>,
    pub reasoning_mappings: Vec<crate::ReasoningMapping>,
    pub summary_mappings: Vec<crate::SummaryMapping>,
    pub thinking_context: Option<crate::ThinkingContext>,
    pub supports_structured_outputs: bool,
    pub supports_structured_outputs_with_tools: bool,
}
impl GeminiModel {
    pub fn new(metadata: ModelMetadata, max_tokens: u64, max_tools: usize) -> Self {
        Self {
            metadata,
            max_tokens,
            max_tools,
            retain_runtime_metadata: false,
            enforce_single_tool_call: false,
            service_tier_mappings: Vec::new(),
            verbosity_mappings: Vec::new(),
            image_mime_types: Vec::new(),
            tool_result_image_mime_types: Vec::new(),
            image_detail_mappings: Vec::new(),
            reasoning_mappings: Vec::new(),
            summary_mappings: Vec::new(),
            thinking_context: None,
            supports_structured_outputs: false,
            supports_structured_outputs_with_tools: false,
        }
    }
}
impl GeminiModel {
    fn compile(
        &self,
        request: &CanonicalRequest,
        max_bytes: usize,
    ) -> ProviderResult<GenerateContentRequest> {
        if !self.metadata.dialects.contains(&request.dialect()) {
            return Err(ProviderError::new(400, "unsupported_dialect"));
        }
        if request.is_streaming()
            && self.metadata.capabilities.streaming == CapabilitySupport::Unsupported
        {
            return Err(ProviderError::new(400, "unsupported_streaming"));
        }
        let images: Vec<_> = self.image_mime_types.iter().map(String::as_str).collect();
        let tool_images: Vec<_> = self
            .tool_result_image_mime_types
            .iter()
            .map(String::as_str)
            .collect();
        GenerateContentRequest::from_responses_with_options(
            request,
            &self.metadata.native_model,
            self.max_tokens,
            max_bytes,
            self.max_tools,
            &RequestOptions {
                retain_runtime_metadata: self.retain_runtime_metadata,
                enforce_single_tool_call: self.enforce_single_tool_call,
                service_tier_mappings: &self.service_tier_mappings,
                verbosity_mappings: &self.verbosity_mappings,
                image_mime_types: &images,
                tool_result_image_mime_types: &tool_images,
                image_detail_mappings: &self.image_detail_mappings,
                reasoning_mappings: &self.reasoning_mappings,
                summary_mappings: &self.summary_mappings,
                thinking_context: self.thinking_context,
                supports_structured_outputs: self.supports_structured_outputs,
                supports_structured_outputs_with_tools: self.supports_structured_outputs_with_tools,
            },
        )
    }
}
pub struct GeminiProvider<S: SecretStore> {
    client: GeminiClient<S>,
    models: BTreeMap<String, GeminiModel>,
    max_models: usize,
}
impl<S: SecretStore + 'static> GeminiProvider<S> {
    /// Keep the native client's credential, transport, deadlines and shared slots.
    pub fn new(
        client: GeminiClient<S>,
        models: Vec<GeminiModel>,
        max_models: usize,
    ) -> ProviderResult<Self> {
        let invalid = || ProviderError::new(400, "invalid_google_profile");
        if models.is_empty() || max_models == 0 {
            return Err(invalid());
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
                return Err(invalid());
            }
            // Validate unused mappings and native routes through the existing compiler.
            let request = CanonicalRequest::new(serde_json::json!({"model":model.metadata.id,"input":[{"role":"user","content":"profile validation"}]}), model.metadata.dialects[0]).map_err(|_| invalid())?;
            model.compile(&request, client.limits().request_bytes)?;
            if profiles.insert(model.metadata.id.clone(), model).is_some() {
                return Err(invalid());
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
    fn model(&self, model: &str) -> ProviderResult<&GeminiModel> {
        self.models
            .get(model)
            .ok_or_else(|| ProviderError::new(404, "unknown_model"))
    }
}
impl<S: SecretStore + 'static> ModelProvider for GeminiProvider<S> {
    fn list_models(&self) -> ProviderFuture<'_, Vec<ModelMetadata>> {
        Box::pin(async {
            let available: BTreeSet<_> = self
                .discover_models(RequestContext::default())
                .await?
                .into_iter()
                .map(|m| m.name().to_owned())
                .collect();
            Ok(self
                .models
                .values()
                .filter(|m| available.contains(&m.metadata.native_model))
                .map(|m| {
                    let mut metadata = m.metadata.clone();
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
            let id = generation_id()?;
            let (native, headers) = self
                .client
                .generate_content_with_headers(
                    &model.metadata.native_model,
                    compiled.wire().clone(),
                    context,
                )
                .await?;
            validate_tool_call_limit(&native, compiled.tool_call_limit())?;
            let selected = if native.blocked_prompt().is_some() {
                None
            } else {
                Some(0)
            };
            let max_bytes = self.client.limits().response_bytes;
            let response = NativeHistory::from_response(
                &native,
                &model.metadata.native_model,
                compiled.wire(),
                selected,
                &id,
                max_bytes,
            )?
            .with_tools(compiled.tools(), max_bytes)?
            .to_responses(max_bytes)?;
            if response.wire().to_string().len() > max_bytes {
                return Err(ProviderError::new(502, "google_projection_too_large"));
            }
            Ok(ProviderResponse { response, headers })
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
            let projection = ResponsesProjection::new(
                model.metadata.native_model.clone(),
                compiled.wire().clone(),
                compiled.tools().clone(),
                generation_id()?,
                self.client.limits().response_bytes,
            )?
            .with_tool_call_limit(compiled.tool_call_limit());
            let native = self
                .client
                .stream_content(
                    &model.metadata.native_model,
                    compiled.wire().clone(),
                    context,
                )
                .await?;
            let headers = native.headers().clone();
            Ok(StreamingResponse {
                events: Box::pin(ProjectedStreamingResponse::new(native, projection)),
                headers,
            })
        })
    }
}
fn generation_id() -> ProviderResult<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| ProviderError::new(503, "google_generation_id_unavailable"))?;
    let mut id = String::from("resp_");
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut id, "{byte:02x}").expect("write to String");
    }
    Ok(id)
}

pub(crate) fn validate_tool_call_limit(
    response: &crate::NativeResponse,
    limit: Option<usize>,
) -> ProviderResult<()> {
    let calls = response
        .candidates()
        .iter()
        .filter(|c| c["finishReason"] == "STOP")
        .filter_map(|c| c["content"]["parts"].as_array())
        .flatten()
        .filter(|p| p["thought"] != true && crate::content::present(p, "functionCall").is_some())
        .count();
    if limit.is_some_and(|limit| calls > limit) {
        return Err(ProviderError::new(502, "google_tool_call_limit_exceeded"));
    }
    Ok(())
}
