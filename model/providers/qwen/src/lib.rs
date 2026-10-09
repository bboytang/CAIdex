//! Model Studio native paginated Models and stateless foreground Responses.
//! Shared transport/Broker, no automatic region selection or tool execution.
mod catalog;
mod config;
mod history;
mod history_stream;
mod request;
mod tools;

pub use caidex_provider_custom::{ClientOptions, Error, Limits};
pub use catalog::NativeModel;
pub use config::QwenConfig;
pub use history::NativeHistory;

use caidex_credentials::{Broker, SecretStore};
use caidex_model_core::{
    CanonicalRequest, CapabilitySupport, ContextHeaders, CredentialRequirement, EvidenceSource,
    ModelCapabilities, ModelMetadata, ModelProvider, ProviderError, ProviderFuture,
    ProviderResponse, ProviderResult, ProviderStreamEvent, RequestContext, ResponsesDialect,
    StreamState, StreamingResponse,
};
use caidex_provider_custom::{ConfiguredModel, CustomResponses, CustomResponsesProvider};
use futures_util::{StreamExt, stream};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Instant,
};

pub struct QwenProvider<S: SecretStore> {
    config: QwenConfig,
    responses: CustomResponsesProvider<S>,
    models_endpoint: CustomResponses,
    limits: Limits,
    runtime_context: bool,
    native_history: bool,
    native_tools: bool,
    verbosity_instructions: HashMap<String, String>,
    reasoning_efforts: HashMap<String, HashMap<String, String>>,
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
            config,
            responses: CustomResponsesProvider::with_options(
                routes,
                broker,
                limits.clone(),
                options,
            )?,
            models_endpoint,
            limits,
            runtime_context: false,
            native_history: false,
            native_tools: false,
            verbosity_instructions: HashMap::new(),
            reasoning_efforts: HashMap::new(),
        })
    }
    /// Consume executor-local attribution and neutral text controls, not caching.
    pub fn with_runtime_context(mut self) -> Self {
        self.runtime_context = true;
        self
    }
    /// Preserve stateless native summary history, bound to this executor.
    pub fn with_native_history(mut self) -> Self {
        self.native_history = true;
        self
    }
    /// Callable tools require bound native history.
    pub fn with_native_tools(mut self) -> Self {
        self.native_tools = true;
        self.with_native_history()
    }
    /// Executor guidance; does not promise a provider-native verbosity scale.
    pub fn with_verbosity_instruction(
        mut self,
        verbosity: String,
        instruction: String,
    ) -> ProviderResult<Self> {
        if !matches!(verbosity.as_str(), "low" | "medium" | "high")
            || instruction.trim().is_empty()
            || self.verbosity_instructions.contains_key(&verbosity)
        {
            return Err(ProviderError::new(400, "qwen_invalid_verbosity_mapping"));
        }
        self.verbosity_instructions.insert(verbosity, instruction);
        Ok(self)
    }
    /// Per-route, executor-owned mapping; do not infer support from model names.
    pub fn with_reasoning_effort_mapping(
        mut self,
        model: String,
        effort: String,
        native: String,
    ) -> ProviderResult<Self> {
        self.responses.metadata(&model)?;
        if !request::valid_effort(&effort)
            || !request::valid_effort(&native)
            || self
                .reasoning_efforts
                .get(&model)
                .is_some_and(|m| m.contains_key(&effort))
        {
            return Err(ProviderError::new(400, "qwen_invalid_effort_mapping"));
        }
        self.reasoning_efforts
            .entry(model)
            .or_default()
            .insert(effort, native);
        Ok(self)
    }
    pub async fn discover_models(
        &self,
        context: RequestContext,
    ) -> ProviderResult<Vec<NativeModel>> {
        let mut context = self.native_context(context)?;
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
    fn prepare(
        &self,
        mut request: CanonicalRequest,
    ) -> ProviderResult<(CanonicalRequest, Option<tools::ToolMap>)> {
        let model = self.responses.metadata(request.model())?;
        let capabilities = model.capabilities;
        if capabilities.text == CapabilitySupport::Unsupported {
            return Err(ProviderError::new(400, "unsupported_text"));
        }
        if request.wire().to_string().len() > self.limits.request_bytes {
            return Err(ProviderError::new(413, "invalid_or_oversized_body"));
        }
        let tools = if self.native_tools {
            Some(tools::ToolMap::from_request(&request)?)
        } else {
            None
        };
        if tools.is_some() {
            if request.wire()["tools"]
                .as_array()
                .is_some_and(|t| !t.is_empty())
                && capabilities.native_tools == CapabilitySupport::Unsupported
            {
                return Err(ProviderError::new(400, "unsupported_tools"));
            }
            let mut wire = request.wire().clone();
            for key in ["tools", "tool_choice", "parallel_tool_calls"] {
                wire.as_object_mut().unwrap().remove(key);
            }
            request =
                CanonicalRequest::new(wire, request.dialect()).map_err(|_| history::invalid())?;
        }
        let efforts = self.reasoning_efforts.get(request.model());
        let route = request.model().to_owned();
        let request = request::compile(
            request,
            self.limits.request_bytes,
            self.runtime_context,
            &self.verbosity_instructions,
            efforts,
            capabilities.reasoning,
            self.native_history,
        )?;
        if !self.native_history {
            return Ok((request, None));
        }
        let request = if let Some(tools) = &tools {
            tools.compile(request)?
        } else {
            request
        };
        let mut wire = request.wire().clone();
        wire["model"] = model.native_model.into();
        let request =
            CanonicalRequest::new(wire, request.dialect()).map_err(|_| history::invalid())?;
        let request = history::expand(
            request,
            &self.config,
            tools.as_ref(),
            self.limits.request_bytes.min(self.limits.response_bytes),
        )?;
        let mut wire = request.wire().clone();
        wire["model"] = route.into();
        CanonicalRequest::new(wire, request.dialect())
            .map(|r| (r, tools))
            .map_err(|_| history::invalid())
    }
    fn native_context(&self, mut context: RequestContext) -> ProviderResult<RequestContext> {
        if context.headers.iter().any(|(name, _)| {
            !self.runtime_context
                || !matches!(
                    name,
                    "session_id" | "x-client-request-id" | "x-codex-turn-metadata"
                )
        }) {
            return Err(ProviderError::new(400, "qwen_unsupported_context"));
        }
        context.headers = ContextHeaders::default();
        Ok(context)
    }
    fn native_request(&self, request: &CanonicalRequest) -> ProviderResult<CanonicalRequest> {
        let mut wire = request.wire().clone();
        wire["model"] = self
            .responses
            .metadata(request.model())?
            .native_model
            .into();
        CanonicalRequest::new(wire, request.dialect()).map_err(|_| history::invalid())
    }
}
fn response_headers(headers: &ContextHeaders) -> ProviderResult<()> {
    if headers.get("x-codex-turn-state").is_some() {
        return Err(ProviderError::new(502, "qwen_unsupported_turn_state"));
    }
    Ok(())
}
fn output(wire: &Value, native_tools: bool) -> ProviderResult<()> {
    let tool = |v: &Value| {
        v["type"].as_str().is_some_and(|kind| {
            !(native_tools && kind == "function_call")
                && (kind.ends_with("_call")
                    || kind.ends_with("_call_output")
                    || kind.starts_with("mcp_")
                    || kind.starts_with("tool_search_"))
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
                && !(native_tools
                    && matches!(
                        kind,
                        "response.function_call_arguments.delta"
                            | "response.function_call_arguments.done"
                    ))
                && (kind.contains("_call")
                    || kind.starts_with("response.mcp_")
                    || kind.starts_with("response.tool_search_"))
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
            let context = self.native_context(context)?;
            let (request, tools) = self.prepare(request)?;
            let native = self.native_request(&request)?;
            let mut response = self.responses.create_response(request, context).await?;
            response_headers(&response.headers)?;
            output(response.response.wire(), tools.is_some())?;
            if let Some(tools) = &tools {
                tools.validate_response(&native, &response.response)?;
            }
            if self.native_history && response.response.state() == StreamState::Completed {
                let budget = self.limits.request_bytes.min(self.limits.response_bytes);
                response.response = NativeHistory::record(
                    &self.config.replay_scope(),
                    &native,
                    &response.response,
                    tools.as_ref(),
                    None,
                    budget,
                )
                .and_then(|h| h.to_responses(budget))
                .map_err(history::native_error)?;
            }
            Ok(response)
        })
    }
    fn stream_response(
        &self,
        request: CanonicalRequest,
        context: RequestContext,
    ) -> ProviderFuture<'_, StreamingResponse> {
        Box::pin(async move {
            let mut context = self.native_context(context)?;
            if self.native_history {
                let deadline = Instant::now() + self.limits.total_timeout;
                context.deadline = Some(context.deadline.unwrap_or(deadline).min(deadline));
            }
            let (request, tools) = self.prepare(request)?;
            let native = self.native_request(&request)?;
            let stream_context = RequestContext {
                headers: ContextHeaders::default(),
                cancellation: context.cancellation.clone(),
                deadline: context.deadline,
            };
            let mut response = self
                .responses
                .stream_response(request, stream_context)
                .await?;
            response_headers(&response.headers)?;
            if self.native_history {
                response.events = Box::pin(history_stream::HistoryStream::new(
                    response.events,
                    self.config.replay_scope(),
                    native,
                    tools,
                    context,
                    self.limits.clone(),
                ));
                return Ok(response);
            }
            response.events = Box::pin(stream::unfold(Some(response.events), |state| async move {
                let mut events = state?;
                match events.next().await? {
                    Ok(event) => {
                        if let ProviderStreamEvent::Model(model) = &event
                            && let Err(error) = output(model.response.wire(), false)
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
