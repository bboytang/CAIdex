//! Model Studio native paginated Models and stateless foreground Responses.
//! Shared transport/Broker, no automatic region selection or tool execution.
mod catalog;
mod config;
mod request;

pub use caidex_provider_custom::{ClientOptions, Error, Limits};
pub use catalog::NativeModel;
pub use config::QwenConfig;

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
use std::{collections::HashSet, sync::Arc, time::Instant};

pub struct QwenProvider<S: SecretStore> {
    responses: CustomResponsesProvider<S>,
    models_endpoint: CustomResponses,
    limits: Limits,
}
impl<S: SecretStore + 'static> QwenProvider<S> {
    pub fn new(
        config: QwenConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::with_options(config, models, broker, limits, ClientOptions::default())
    }
    pub fn with_options(
        config: QwenConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
        options: ClientOptions,
    ) -> Result<Self, Error> {
        let models_endpoint = config.endpoint("api/v1/models")?;
        let routes = models
            .into_iter()
            .map(|model| {
                model.validate().map_err(|_| Error::InvalidRoute)?;
                if model.dialects != [ResponsesDialect::Classic] {
                    return Err(Error::InvalidRoute);
                }
                ConfiguredModel::new(
                    model.id.clone(),
                    model.native_model.clone(),
                    model.dialects.clone(),
                    config.endpoint("compatible-mode/v1/responses")?,
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
    pub async fn discover_models(
        &self,
        mut context: RequestContext,
    ) -> ProviderResult<Vec<NativeModel>> {
        validate_context(&context)?;
        // One absolute budget covers the whole scan, not a fresh timeout per page.
        context.deadline = Some(
            context
                .deadline
                .unwrap_or(Instant::now() + self.limits.total_timeout)
                .min(Instant::now() + self.limits.total_timeout),
        );
        let mut total = None;
        let mut bytes = 0usize;
        let mut models = Vec::new();
        let mut ids = HashSet::new();
        for page in 1..=catalog::MAX_PAGES {
            let page_no = page.to_string();
            let wire = self
                .responses
                .get_json_with_query(
                    &self.models_endpoint,
                    &[("page_no", &page_no), ("page_size", "20")],
                    RequestContext {
                        headers: ContextHeaders::default(),
                        cancellation: context.cancellation.clone(),
                        deadline: context.deadline,
                    },
                )
                .await?;
            bytes = bytes
                .checked_add(wire.to_string().len())
                .ok_or_else(|| ProviderError::new(413, "qwen_catalog_limit"))?;
            if bytes > self.limits.response_bytes {
                return Err(ProviderError::new(413, "qwen_catalog_limit"));
            }
            let (count, batch) = catalog::parse(wire, page)?;
            if total.is_some_and(|old| old != count) {
                return Err(catalog::invalid());
            }
            total = Some(count);
            for model in batch {
                if !ids.insert(model.id().to_owned()) {
                    return Err(catalog::invalid());
                }
                models.push(model);
            }
            if models.len() as u64 == count {
                models.sort_by(|a, b| a.id().cmp(b.id()));
                return Ok(models);
            }
        }
        Err(ProviderError::new(413, "qwen_catalog_limit"))
    }
    fn prepare(&self, request: CanonicalRequest) -> ProviderResult<CanonicalRequest> {
        if self.responses.metadata(request.model())?.capabilities.text
            == CapabilitySupport::Unsupported
        {
            return Err(ProviderError::new(400, "unsupported_text"));
        }
        request::compile(request, self.limits.request_bytes)
    }
}
fn validate_context(context: &RequestContext) -> ProviderResult<()> {
    if context.headers.iter().next().is_some() {
        return Err(ProviderError::new(400, "qwen_unsupported_context"));
    }
    Ok(())
}
fn response_headers(headers: &ContextHeaders) -> ProviderResult<()> {
    if headers.get("x-codex-turn-state").is_some() {
        return Err(ProviderError::new(502, "qwen_unsupported_turn_state"));
    }
    Ok(())
}
fn output(wire: &Value) -> ProviderResult<()> {
    let tool = |v: &Value| {
        v["type"].as_str().is_some_and(|kind| {
            kind.ends_with("_call")
                || kind.ends_with("_call_output")
                || kind == "mcp_approval_request"
        })
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
            kind.starts_with("response.")
                && (kind.contains("_call") || kind.starts_with("response.mcp_"))
        })
    {
        return Err(ProviderError::new(502, "qwen_unexpected_tool"));
    }
    if wire["store"] == true || wire["response"]["store"] == true {
        return Err(ProviderError::new(502, "qwen_unexpected_storage"));
    }
    Ok(())
}
impl<S: SecretStore + 'static> ModelProvider for QwenProvider<S> {
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
            validate_context(&context)?;
            let request = self.prepare(request)?;
            let response = self.responses.create_response(request, context).await?;
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
            validate_context(&context)?;
            let request = self.prepare(request)?;
            let mut response = self.responses.stream_response(request, context).await?;
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
