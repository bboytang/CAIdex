//! OpenRouter Models and stateless Classic Responses, using shared transport.
//! No hosted executor, model-name capability guesses or inference retries.
mod catalog;
mod config;
mod history;
mod history_stream;
mod request;
mod tools;

pub use caidex_provider_custom::{ClientOptions, Error, Limits};
pub use catalog::NativeModel;
pub use config::OpenRouterConfig;
pub use history::NativeHistory;

use caidex_credentials::{Broker, SecretStore};
use caidex_model_core::{
    CanonicalRequest, ContextHeaders, CredentialRequirement, EvidenceSource, ModelCapabilities,
    ModelMetadata, ModelProvider, ProviderError, ProviderFuture, ProviderResponse, ProviderResult,
    ProviderStreamEvent, RequestContext, StreamingResponse,
};
use caidex_provider_custom::{ConfiguredModel, CustomResponses, CustomResponsesProvider};
use futures_util::{StreamExt, stream};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::Arc,
};

struct Prepared {
    request: CanonicalRequest,
    tools: Option<tools::ToolPolicy>,
    history: Option<history::HistoryContext>,
}

pub struct OpenRouterProvider<S: SecretStore> {
    responses: CustomResponsesProvider<S>,
    models_endpoint: CustomResponses,
    limits: Limits,
    runtime_context: bool,
    model_dialects: HashMap<String, Vec<caidex_model_core::ResponsesDialect>>,
    reasoning_efforts: HashMap<String, HashMap<String, String>>,
    verbosity_instructions: HashMap<String, HashMap<String, String>>,
    service_tiers: HashMap<String, HashMap<String, String>>,
    backends: HashMap<String, String>,
    native_tools: HashSet<String>,
    advanced_tools: HashSet<String>,
    native_history: HashSet<String>,
    reasoning_summaries: HashMap<String, BTreeSet<String>>,
    reasoning_contexts: HashMap<String, BTreeSet<String>>,
    replay_scope: serde_json::Value,
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
        Self::build(config, models, broker, limits, options, false)
    }
    /// Explicit Lite compilation; native transport remains stateless Classic.
    pub fn with_lite_options(
        config: OpenRouterConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
        options: ClientOptions,
    ) -> Result<Self, Error> {
        Self::build(config, models, broker, limits, options, true)
    }
    fn build(
        config: OpenRouterConfig,
        models: Vec<ModelMetadata>,
        broker: Arc<Broker<S>>,
        limits: Limits,
        options: ClientOptions,
        lite: bool,
    ) -> Result<Self, Error> {
        let mut model_dialects = HashMap::new();
        let models_endpoint = config.endpoint("models")?;
        let routes = models
            .into_iter()
            .map(|mut model| {
                model.validate().map_err(|_| Error::InvalidRoute)?;
                if !lite && model.dialects != [caidex_model_core::ResponsesDialect::Classic] {
                    return Err(Error::InvalidRoute);
                }
                model_dialects.insert(model.id.clone(), model.dialects.clone());
                model.dialects = vec![caidex_model_core::ResponsesDialect::Classic];
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
            runtime_context: false,
            model_dialects,
            reasoning_efforts: HashMap::new(),
            verbosity_instructions: HashMap::new(),
            service_tiers: HashMap::new(),
            backends: HashMap::new(),
            native_tools: HashSet::new(),
            advanced_tools: HashSet::new(),
            native_history: HashSet::new(),
            reasoning_summaries: HashMap::new(),
            reasoning_contexts: HashMap::new(),
            replay_scope: config.replay_scope(),
        })
    }
    /// Consume executor-local attribution and neutral text; do not forward identity or enable caching.
    pub fn with_runtime_context(mut self) -> Self {
        self.runtime_context = true;
        self
    }
    /// Executor-owned, per-route effort translation, never inferred from model names or catalog.
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
            return Err(ProviderError::new(400, "openrouter_invalid_effort_mapping"));
        }
        self.reasoning_efforts
            .entry(model)
            .or_default()
            .insert(effort, native);
        Ok(self)
    }
    /// Per-route executor guidance, not a native verbosity scale or structured output.
    pub fn with_verbosity_instruction(
        mut self,
        model: String,
        verbosity: String,
        instruction: String,
    ) -> ProviderResult<Self> {
        self.responses.metadata(&model)?;
        if !matches!(verbosity.as_str(), "low" | "medium" | "high")
            || instruction.trim().is_empty()
            || self
                .verbosity_instructions
                .get(&model)
                .is_some_and(|m| m.contains_key(&verbosity))
        {
            return Err(ProviderError::new(
                400,
                "openrouter_invalid_verbosity_mapping",
            ));
        }
        self.verbosity_instructions
            .entry(model)
            .or_default()
            .insert(verbosity, instruction);
        Ok(self)
    }
    /// Explicit native tier request policy, not a guarantee of capacity, price or backend identity.
    pub fn with_service_tier_mapping(
        mut self,
        model: String,
        source: String,
        native: String,
    ) -> ProviderResult<Self> {
        self.responses.metadata(&model)?;
        if !matches!(
            (source.as_str(), native.as_str()),
            ("auto" | "default", "auto" | "default")
                | ("flex", "flex")
                | ("priority" | "fast", "priority" | "fast")
                | ("ultrafast", "ultrafast")
        ) || self
            .service_tiers
            .get(&model)
            .is_some_and(|m| m.contains_key(&source))
        {
            return Err(ProviderError::new(400, "openrouter_invalid_tier_mapping"));
        }
        self.service_tiers
            .entry(model)
            .or_default()
            .insert(source, native);
        Ok(self)
    }
    /// Executor-owned singleton provider.only policy, not proof of the serving endpoint.
    /// A base slug may match multiple variants; use the official full endpoint slug when needed.
    pub fn with_backend_selection(
        mut self,
        model: String,
        backend: String,
    ) -> ProviderResult<Self> {
        self.responses.metadata(&model)?;
        if !backend.split('/').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        }) || self.backends.contains_key(&model)
        {
            return Err(ProviderError::new(
                400,
                "openrouter_invalid_backend_selection",
            ));
        }
        self.backends.insert(model, backend);
        Ok(self)
    }
    /// Enable flat Classic function tools for an explicitly configured backend route.
    pub fn with_native_tools(mut self, model: String) -> ProviderResult<Self> {
        self.responses.metadata(&model)?;
        if !self.backends.contains_key(&model) || !self.native_tools.insert(model) {
            return Err(ProviderError::new(400, "openrouter_invalid_tool_route"));
        }
        Ok(self)
    }
    /// Enable native namespace/custom tools and explicit subset compilation on a native-tools route.
    pub fn with_advanced_tools(mut self, model: String) -> ProviderResult<Self> {
        self.responses.metadata(&model)?;
        if !self.native_tools.contains(&model) || !self.advanced_tools.insert(model) {
            return Err(ProviderError::new(400, "openrouter_invalid_tool_route"));
        }
        Ok(self)
    }
    /// Bind stateless native history to this executor's route, scope and complete compiled policy.
    pub fn with_native_history(mut self, model: String) -> ProviderResult<Self> {
        self.responses.metadata(&model)?;
        if !self.backends.contains_key(&model) || !self.native_history.insert(model) {
            return Err(ProviderError::new(400, "openrouter_invalid_history_route"));
        }
        Ok(self)
    }
    /// Explicit native summary support on a route with local Runtime context and bound history.
    pub fn with_reasoning_summary(
        mut self,
        model: String,
        summary: String,
    ) -> ProviderResult<Self> {
        self.responses.metadata(&model)?;
        if !self.runtime_context
            || !self.native_history.contains(&model)
            || !request::valid_summary(&summary)
            || !self
                .reasoning_summaries
                .entry(model)
                .or_default()
                .insert(summary)
        {
            return Err(ProviderError::new(400, "openrouter_invalid_summary_policy"));
        }
        Ok(self)
    }
    /// Declare exact native context support; the executor must verify model/backend support.
    pub fn with_reasoning_context(
        mut self,
        model: String,
        context: String,
    ) -> ProviderResult<Self> {
        self.responses.metadata(&model)?;
        if !self.runtime_context
            || !self.native_history.contains(&model)
            || !request::valid_context(&context)
            || !self
                .reasoning_contexts
                .entry(model)
                .or_default()
                .insert(context)
        {
            return Err(ProviderError::new(400, "openrouter_invalid_context_policy"));
        }
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
            return Err(ProviderError::new(400, "openrouter_unsupported_context"));
        }
        context.headers = ContextHeaders::default();
        Ok(context)
    }
    /// Complete native model inventory, including non-Responses models. It is
    /// availability metadata, never a Codex compatibility or capability test.
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
    fn prepare(&self, request: CanonicalRequest) -> ProviderResult<Prepared> {
        let metadata = self.metadata(request.model())?;
        if !metadata.dialects.contains(&request.dialect()) {
            return Err(ProviderError::new(400, "unsupported_dialect"));
        }
        if request.wire().to_string().len() > self.limits.request_bytes {
            return Err(ProviderError::new(413, "invalid_or_oversized_body"));
        }
        let (request, lite) = request::classic(request)?;
        if self.native_history.contains(request.model()) {
            let mut source = request.wire().clone();
            request::consume_context(&mut source, self.runtime_context)?;
            let mut controls = source.clone();
            for k in ["model", "input", "stream"] {
                controls.as_object_mut().unwrap().remove(k);
            }
            let mut policy = serde_json::json!({"route":request.model(),"native_model":metadata.native_model,"capabilities":metadata.capabilities,"runtime_context":self.runtime_context,"native_tools":self.native_tools.contains(request.model()),"advanced_tools":self.advanced_tools.contains(request.model()),"backend":self.backends[request.model()],"efforts":self.reasoning_efforts.get(request.model()),"verbosity":self.verbosity_instructions.get(request.model()),"tiers":self.service_tiers.get(request.model()),"controls":controls});
            if let Some(lite) = lite {
                policy["lite"] = lite;
            }
            // Absent policies keep existing v1 carriers byte-compatible.
            if let Some(choices) = self.reasoning_summaries.get(request.model()) {
                policy["summaries"] = serde_json::json!(choices);
            }
            if let Some(choices) = self.reasoning_contexts.get(request.model()) {
                policy["contexts"] = serde_json::json!(choices);
            }
            let source =
                CanonicalRequest::new(source, request.dialect()).map_err(|_| history::invalid())?;
            let (request, tools, history) =
                history::prepare(source, self.replay_scope.clone(), policy, &self.limits)?;
            return Ok(Prepared {
                request,
                tools,
                history: Some(history),
            });
        }
        let policy = if self.native_tools.contains(request.model()) {
            let policy = tools::ToolPolicy::new(
                &request,
                metadata.capabilities.parallel_tools,
                self.advanced_tools.contains(request.model()),
            )?;
            if policy.has_tools()
                && metadata.capabilities.native_tools
                    == caidex_model_core::CapabilitySupport::Unsupported
            {
                return Err(ProviderError::new(400, "unsupported_tools"));
            }
            Some(policy)
        } else {
            None
        };
        let original = request.wire().clone();
        let mut wire = original.clone();
        if policy.is_some() {
            for key in ["tools", "tool_choice", "parallel_tool_calls"] {
                wire.as_object_mut().unwrap().remove(key);
            }
            if let Some(items) = wire["input"].as_array_mut() {
                items.retain(|item| !tools::input_item(item));
            }
        }
        let request = CanonicalRequest::new(wire, request.dialect())
            .map_err(|_| ProviderError::new(400, "openrouter_invalid_request"))?;
        let efforts = self.reasoning_efforts.get(request.model());
        let verbosity = self.verbosity_instructions.get(request.model());
        let tiers = self.service_tiers.get(request.model());
        let backend = self.backends.get(request.model());
        let compiled = request::compile(
            request,
            self.limits.request_bytes,
            self.runtime_context,
            efforts,
            metadata.capabilities.reasoning,
            verbosity,
            tiers,
        )?;
        let mut wire = compiled.wire().clone();
        if let Some(backend) = backend {
            wire["provider"]["only"] = serde_json::json!([backend]);
        }
        if policy.is_some() {
            wire["input"] = original["input"].clone();
            for key in ["tools", "tool_choice", "parallel_tool_calls"] {
                if let Some(value) = original.get(key) {
                    wire[key] = value.clone();
                }
            }
        }
        if let Some(policy) = &policy {
            policy.compile_selection(&mut wire);
        }
        CanonicalRequest::new(wire, compiled.dialect())
            .map(|request| Prepared {
                request,
                tools: policy,
                history: None,
            })
            .map_err(|_| ProviderError::new(400, "openrouter_invalid_request"))
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
                    model.dialects = self.model_dialects[&model.id].clone();
                    model.source = EvidenceSource::ProviderCatalog;
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
            let Prepared {
                request,
                tools,
                history,
            } = self.prepare(request)?;
            let mut response = self.responses.create_response(request, context).await?;
            request::output(response.response.wire(), tools.is_some())?;
            if let Some(tools) = tools {
                tools.response(response.response.wire())?;
            }
            request::headers(&response.headers)?;
            if let Some(history) = history
                && response.response.state() == caidex_model_core::StreamState::Completed
            {
                response.response = NativeHistory::record(
                    &history,
                    &response.response,
                    None,
                    self.limits.request_bytes.min(self.limits.response_bytes),
                )?
                .to_responses(self.limits.request_bytes.min(self.limits.response_bytes))
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
            let Prepared {
                request,
                tools,
                history,
            } = self.prepare(request)?;
            if tools.is_some() || history.is_some() {
                let deadline = std::time::Instant::now() + self.limits.total_timeout;
                context.deadline = Some(context.deadline.unwrap_or(deadline).min(deadline));
            }
            let stream_context = RequestContext {
                headers: ContextHeaders::default(),
                cancellation: context.cancellation.clone(),
                deadline: context.deadline,
            };
            let mut response = self
                .responses
                .stream_response(request, stream_context)
                .await?;
            request::headers(&response.headers)?;
            if let Some(history) = history {
                response.events = history_stream::buffered_stream(
                    response.events,
                    history,
                    context,
                    self.limits.clone(),
                );
                return Ok(response);
            }
            if let Some(tools) = tools {
                response.events = tools::buffered_stream(
                    response.events,
                    tools,
                    context,
                    self.limits.response_bytes,
                );
                return Ok(response);
            }
            response.events = Box::pin(stream::unfold(Some(response.events), |state| async move {
                let mut events = state?;
                match events.next().await? {
                    Ok(event) => {
                        if let ProviderStreamEvent::Model(model) = &event
                            && let Err(error) = request::output(model.response.wire(), false)
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
