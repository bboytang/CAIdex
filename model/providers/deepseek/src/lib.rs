//! DeepSeek native Models and foreground Responses, using the shared transport.
//! No implicit keys, model-name capability inference or tool executor.
mod catalog;
mod config;
mod history;
mod history_stream;
mod request;
mod tools;

pub use caidex_provider_custom::{ClientOptions, Error, Limits};
pub use catalog::NativeModel;
pub use config::DeepSeekConfig;
pub use history::NativeHistory;

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
    config: DeepSeekConfig,
    responses: CustomResponsesProvider<S>,
    models_endpoint: CustomResponses,
    request_bytes: usize,
    runtime_context: bool,
    verbosity_instructions: HashMap<String, String>,
    reasoning_efforts: HashMap<String, String>,
    limits: Limits,
    native_history: bool,
    native_tools: bool,
    native_apply_patch: bool,
    lite: bool,
    model_dialects: HashMap<String, Vec<ResponsesDialect>>,
}
impl<S: SecretStore + 'static> DeepSeekProvider<S> {
    /// Explicit Lite custom-to-function compilation and local single-call
    /// delivery. Native transport remains Classic; history binds this policy.
    pub fn with_lite_options(
        config: DeepSeekConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
        options: ClientOptions,
    ) -> Result<Self, Error> {
        Ok(Self::build(config, models, broker, limits, options, true)?.with_native_tools())
    }

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
        Self::build(config, models, broker, limits, options, false)
    }
    fn build(
        config: DeepSeekConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
        options: ClientOptions,
        lite: bool,
    ) -> Result<Self, Error> {
        let models_endpoint = config.endpoint("models")?;
        let mut model_dialects = HashMap::new();
        let routes = models
            .into_iter()
            .map(|mut model| {
                model.validate().map_err(|_| Error::InvalidRoute)?;
                if !lite && model.dialects != [ResponsesDialect::Classic] {
                    return Err(Error::InvalidRoute);
                }
                model_dialects.insert(model.id.clone(), model.dialects.clone());
                model.dialects = vec![ResponsesDialect::Classic];
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
            responses: CustomResponsesProvider::with_options(
                routes,
                broker,
                limits.clone(),
                options,
            )?,
            config,
            models_endpoint,
            request_bytes,
            runtime_context: false,
            verbosity_instructions: HashMap::new(),
            reasoning_efforts: HashMap::new(),
            limits,
            native_history: false,
            native_tools: false,
            native_apply_patch: false,
            lite,
            model_dialects,
        })
    }
    /// Explicit local attribution and leading developer-to-system policy.
    /// Combined with native history, consumes auto whole-reasoning display,
    /// all_turns replay and the local carrier include; no encryption or caching.
    pub fn with_runtime_context(mut self) -> Self {
        self.runtime_context = true;
        self
    }
    /// Bind complete native reasoning/output to the execution scope and prefix.
    /// Sensitive JSON carrier; does not implement provider-side persistence.
    pub fn with_native_history(mut self) -> Self {
        self.native_history = true;
        self
    }
    /// Classic function/namespace compilation plus terminal-only tool delivery.
    /// Custom tools, deferred search and Lite are not enabled by this policy.
    pub fn with_native_tools(mut self) -> Self {
        self.native_tools = true;
        self.with_native_history()
    }
    /// Native apply_patch with source grammar retained as guidance, not a
    /// native constrained-decoding guarantee. Codex validates execution.
    pub fn with_native_apply_patch(mut self) -> Self {
        self.native_apply_patch = true;
        self.with_native_tools()
    }
    /// Executor-owned effort selection; aliases are never inferred from model names.
    /// Mapping a source level does not claim identical reasoning strength.
    pub fn with_reasoning_effort_mapping(
        mut self,
        effort: String,
        native: String,
    ) -> ProviderResult<Self> {
        if !matches!(
            effort.as_str(),
            "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
        ) || !matches!(native.as_str(), "none" | "low" | "high" | "max")
            || self.reasoning_efforts.contains_key(&effort)
        {
            return Err(ProviderError::new(400, "deepseek_invalid_effort_mapping"));
        }
        self.reasoning_efforts.insert(effort, native);
        Ok(self)
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
    fn prepare(
        &self,
        request: CanonicalRequest,
    ) -> ProviderResult<(CanonicalRequest, Option<tools::ToolMap>)> {
        let model = self.metadata(request.model())?;
        if !model.dialects.contains(&request.dialect()) {
            return Err(ProviderError::new(400, "unsupported_dialect"));
        }
        if model.capabilities.text == CapabilitySupport::Unsupported {
            return Err(ProviderError::new(400, "unsupported_text"));
        }
        let (request, lite_single) = request::classic(request, self.lite, self.request_bytes)?;
        if self.native_tools
            && request.wire()["tools"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
            && model.capabilities.native_tools == CapabilitySupport::Unsupported
        {
            return Err(ProviderError::new(400, "unsupported_tools"));
        }
        let request = request::compile_history_controls(
            request,
            self.runtime_context && self.native_history,
            model.capabilities.reasoning,
            self.request_bytes,
        )?;
        let request =
            request::compile_effort(request, &self.reasoning_efforts, self.request_bytes)?;
        if request.wire().get("reasoning").is_some()
            && request.wire()["reasoning"]["effort"] != "none"
            && model.capabilities.reasoning == CapabilitySupport::Unsupported
        {
            return Err(ProviderError::new(400, "unsupported_reasoning"));
        }
        let route = request.model().to_owned();
        let request = request::compile(
            request,
            self.request_bytes,
            self.runtime_context,
            &self.verbosity_instructions,
            self.native_history,
            self.native_tools,
            !self.reasoning_efforts.is_empty(),
        )?;
        if !self.native_history {
            return Ok((request, None));
        }
        let tools = tools::ToolMap::from_request(&request, self.native_apply_patch, lite_single)?;
        let mut request = tools.compile(request)?;
        let mut wire = request.wire().clone();
        wire["model"] = model.native_model.into();
        request = CanonicalRequest::new(wire, request.dialect())
            .map_err(|_| ProviderError::new(400, "deepseek_invalid_request"))?;
        request = history::expand(
            request,
            &self.config,
            &tools,
            self.limits.request_bytes.min(self.limits.response_bytes),
        )?;
        request = request::compile(
            request,
            self.request_bytes,
            self.runtime_context,
            &self.verbosity_instructions,
            self.native_history,
            self.native_tools,
            !self.reasoning_efforts.is_empty(),
        )?;
        tools::validate_input(&request)?;
        let mut wire = request.wire().clone();
        wire["model"] = route.into();
        request = CanonicalRequest::new(wire, request.dialect())
            .map_err(|_| ProviderError::new(400, "deepseek_invalid_request"))?;
        Ok((request, Some(tools)))
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
                    model.dialects = self.model_dialects[&model.id].clone();
                    Some(model)
                })
                .collect())
        })
    }
    fn metadata(&self, model: &str) -> ProviderResult<ModelMetadata> {
        let mut metadata = self.responses.metadata(model)?;
        metadata.dialects = self.model_dialects[model].clone();
        Ok(metadata)
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
            let native_request = history::native_request(
                &request,
                &self.responses.metadata(request.model())?.native_model,
            )?;
            let mut response = self
                .responses
                .create_response(request.clone(), context)
                .await?;
            response_headers(&response.headers)?;
            if let Some(tools) = tools {
                response.response = NativeHistory::record(
                    &self.config.replay_scope(),
                    &native_request,
                    &response.response,
                    &tools,
                    None,
                    self.limits.request_bytes.min(self.limits.response_bytes),
                )
                .map_err(history::native_error)?
                .to_responses(self.limits.request_bytes.min(self.limits.response_bytes))
                .map_err(history::native_error)?;
            } else {
                output(response.response.wire())?;
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
            let context = self.native_context(context)?;
            let history_context = RequestContext {
                cancellation: context.cancellation.clone(),
                deadline: context.deadline,
                headers: ContextHeaders::default(),
            };
            let (request, tools) = self.prepare(request)?;
            let native_request = history::native_request(
                &request,
                &self.responses.metadata(request.model())?.native_model,
            )?;
            let mut response = self
                .responses
                .stream_response(request.clone(), context)
                .await?;
            response_headers(&response.headers)?;
            if let Some(tools) = tools {
                response.events = Box::pin(history_stream::HistoryStream::new(
                    response.events,
                    self.config.replay_scope(),
                    native_request,
                    history_context,
                    self.limits.clone(),
                    tools,
                ));
                return Ok(response);
            }
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
