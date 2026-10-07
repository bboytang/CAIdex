use crate::transfer::guard;
use crate::{ModelCatalog, ModelsPage, NativeMessage, NativeModel, NativeStreamingResponse};
use caidex_credentials::{Broker, CredentialRef, SecretKind, SecretStore};
use caidex_model_core::{
    ContextHeaders, ProviderError, ProviderResult, RESPONSE_HEADERS, RequestContext,
};
use caidex_provider_custom::{ClientOptions, CustomResponses, Error, Limits, retry_after};
use reqwest::{Url, header::HeaderValue};
use serde_json::Value;
use std::{fmt, sync::Arc, time::SystemTime};
use tokio::{sync::Semaphore, time::Instant};

/// Executor-owned profile. Caller JSON cannot choose a URL, credential or scope.
pub struct AnthropicConfig {
    base: Url,
    credential: CredentialRef,
    workspace: Option<HeaderValue>,
    local_runtime_context: bool,
    thinking_binding_controls: bool,
    inline_tools: bool,
    expected_organization: Option<String>,
}
impl AnthropicConfig {
    pub fn new(credential: CredentialRef) -> Result<Self, Error> {
        if credential.provider.as_str() != "anthropic" || credential.kind != SecretKind::ApiKey {
            return Err(Error::InvalidRoute);
        }
        Ok(Self {
            base: Url::parse("https://api.anthropic.com/v1/").expect("fixed URL"),
            credential,
            workspace: None,
            local_runtime_context: false,
            thinking_binding_controls: false,
            inline_tools: false,
            expected_organization: None,
        })
    }
    pub fn with_base_url(mut self, base: &str) -> Result<Self, Error> {
        CustomResponses::new(base, None)?;
        self.base = Url::parse(base).map_err(|_| Error::InvalidEndpoint)?;
        if !self.base.path().ends_with('/') {
            self.base.set_path(&format!("{}/", self.base.path()));
        }
        Ok(self)
    }
    pub fn with_workspace(mut self, workspace: &str) -> Result<Self, Error> {
        if workspace.is_empty()
            || workspace.len() > 1024
            || !workspace.bytes().all(|byte| (33..=126).contains(&byte))
        {
            return Err(Error::InvalidScope);
        }
        let mut value = HeaderValue::from_str(workspace).map_err(|_| Error::InvalidScope)?;
        value.set_sensitive(true);
        self.workspace = Some(value);
        Ok(self)
    }
    /// Accept session/request/turn metadata as local RequestContext only. No
    /// native cache, account or sticky-routing equivalence is implied.
    pub fn with_local_runtime_context(mut self) -> Self {
        self.local_runtime_context = true;
        self
    }
    /// Opt into the fixed thinking-binding beta and require its reports.
    /// Prefix enforcement still requires an explicit reasoning mapping with
    /// block_binding.prefix_mismatch_behavior = "error"; no drop/retry is added.
    pub fn with_thinking_binding_controls(mut self) -> Self {
        self.thinking_binding_controls = true;
        self
    }
    /// Opt into native inline definitions; model capability is a separate gate.
    pub fn with_inline_tools(mut self) -> Self {
        self.inline_tools = true;
        self
    }
    /// Require authenticated organization identity before sending model input.
    /// This is an expectation, not an organization-selection request header.
    pub fn with_expected_organization(mut self, organization: &str) -> Result<Self, Error> {
        if !valid_organization_id(organization) {
            return Err(Error::InvalidScope);
        }
        self.expected_organization = Some(organization.to_owned());
        Ok(self)
    }
}
impl fmt::Debug for AnthropicConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AnthropicConfig([PROFILE OMITTED])")
    }
}

pub struct AnthropicClient<S: SecretStore> {
    config: AnthropicConfig,
    broker: Arc<Broker<S>>,
    http: reqwest::Client,
    limits: Limits,
    permits: Arc<Semaphore>,
}
impl<S: SecretStore + 'static> AnthropicClient<S> {
    pub(crate) fn credential(&self) -> &CredentialRef {
        &self.config.credential
    }
    pub(crate) fn limits(&self) -> &Limits {
        &self.limits
    }
    pub(crate) fn expected_organization(&self) -> Option<&str> {
        self.config.expected_organization.as_deref()
    }
    pub(crate) fn inline_tools(&self) -> bool {
        self.config.inline_tools
    }
    pub(crate) fn thinking_binding_controls(&self) -> bool {
        self.config.thinking_binding_controls
    }
    pub fn new(
        config: AnthropicConfig,
        broker: Arc<Broker<S>>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::with_options(config, broker, limits, ClientOptions::default())
    }
    pub fn with_options(
        config: AnthropicConfig,
        broker: Arc<Broker<S>>,
        limits: Limits,
        options: ClientOptions,
    ) -> Result<Self, Error> {
        limits.validate()?;
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(limits.connect_timeout)
            .pool_max_idle_per_host(0);
        for certificate in options.root_certificates {
            builder = builder.add_root_certificate(certificate);
        }
        let http = builder.build().map_err(|_| Error::Initialization)?;
        Ok(Self {
            config,
            broker,
            http,
            permits: Arc::new(Semaphore::new(limits.in_flight)),
            limits,
        })
    }
    /// One deadline, concurrency permit and aggregate byte budget cover all
    /// pages. Cursors are encoded query parameters, never interpreted as URLs.
    pub async fn discover_models(
        &self,
        max_models: usize,
        context: RequestContext,
    ) -> ProviderResult<Vec<NativeModel>> {
        let mut catalog = ModelCatalog::new(max_models)?;
        let deadline = self.deadline(&context)?;
        let _permit = self
            .permits
            .try_acquire()
            .map_err(|_| ProviderError::new(503, "provider_busy"))?;
        let mut remaining = self.limits.response_bytes;
        let mut cursor: Option<String> = None;
        loop {
            let mut url = self
                .config
                .base
                .join("models")
                .expect("fixed relative path");
            url.query_pairs_mut().append_pair("limit", "1000");
            if let Some(cursor) = &cursor {
                url.query_pairs_mut().append_pair("after_id", cursor);
            }
            let (wire, _) = self
                .json(self.http.get(url), &context, deadline, &mut remaining)
                .await?;
            cursor = catalog.append(ModelsPage::parse(wire)?)?;
            if cursor.is_none() {
                return catalog.finish();
            }
        }
    }
    /// Read the actual organization of the currently resolved API key. No
    /// inference, key fingerprint or credential-reference identity cache.
    pub async fn current_organization(&self, context: RequestContext) -> ProviderResult<String> {
        let deadline = self.deadline(&context)?;
        let _permit = self
            .permits
            .try_acquire()
            .map_err(|_| ProviderError::new(503, "provider_busy"))?;
        let key = self
            .authentication_key(
                &context,
                deadline.min(Instant::now() + self.limits.header_timeout),
            )
            .await?;
        self.authenticated_organization(&key, &context, deadline)
            .await
    }
    /// Native Messages only. No Responses translation or tool execution here.
    /// The configured model overrides the body's model before transmission.
    pub async fn create_message(
        &self,
        model: &str,
        wire: Value,
        context: RequestContext,
    ) -> ProviderResult<NativeMessage> {
        Ok(self
            .create_message_with_headers(model, wire, context)
            .await?
            .0)
    }
    /// Preserve the native request-id as the shared response correlation header.
    pub async fn create_message_with_headers(
        &self,
        model: &str,
        mut wire: Value,
        context: RequestContext,
    ) -> ProviderResult<(NativeMessage, ContextHeaders)> {
        let deadline = self.deadline(&context)?;
        let body = self.message_body(model, &mut wire, false)?;
        let _permit = self
            .permits
            .try_acquire()
            .map_err(|_| ProviderError::new(503, "provider_busy"))?;
        let url = self
            .config
            .base
            .join("messages")
            .expect("fixed relative path");
        let outgoing = self
            .http
            .post(url)
            .header("content-type", "application/json")
            .body(body);
        let mut remaining = self.limits.response_bytes;
        let (wire, headers) = self
            .json(outgoing, &context, deadline, &mut remaining)
            .await?;
        if self.config.thinking_binding_controls {
            crate::message::require_binding_report(&wire)?;
        }
        Ok((NativeMessage::parse(wire)?, headers))
    }
    /// Foreground native SSE. Dropping the delivery cancels its I/O worker.
    pub async fn stream_message(
        &self,
        model: &str,
        mut wire: Value,
        context: RequestContext,
    ) -> ProviderResult<NativeStreamingResponse> {
        let deadline = self.deadline(&context)?;
        let body = self.message_body(model, &mut wire, true)?;
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| ProviderError::new(503, "provider_busy"))?;
        let url = self
            .config
            .base
            .join("messages")
            .expect("fixed relative path");
        let outgoing = self
            .http
            .post(url)
            .header("content-type", "application/json")
            .body(body);
        let response = self
            .execute(outgoing, &context, deadline, "text/event-stream")
            .await?;
        crate::transfer::stream(
            response,
            context,
            deadline,
            self.limits.clone(),
            permit,
            self.config.thinking_binding_controls,
        )
    }
    fn message_body(&self, model: &str, wire: &mut Value, stream: bool) -> ProviderResult<Vec<u8>> {
        if model.trim().is_empty()
            || model.chars().any(char::is_control)
            || !wire.is_object()
            || wire["max_tokens"].as_u64().is_none()
            || !wire["messages"].is_array()
            || wire
                .get("stream")
                .is_some_and(|v| !v.is_null() && v != stream)
        {
            return Err(ProviderError::new(400, "invalid_native_message_request"));
        }
        if wire["thinking"].get("block_binding").is_some() && !self.config.thinking_binding_controls
        {
            return Err(ProviderError::new(
                400,
                "anthropic_thinking_binding_beta_required",
            ));
        }
        if !self.config.inline_tools
            && wire["messages"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|message| message["content"].as_array().into_iter().flatten())
                .any(|block| {
                    matches!(
                        block["type"].as_str(),
                        Some("tool_addition" | "tool_removal")
                    )
                })
        {
            return Err(ProviderError::new(
                400,
                "anthropic_inline_tools_beta_required",
            ));
        }
        wire["model"] = model.into();
        wire["stream"] = stream.into();
        let body = serde_json::to_vec(wire).expect("JSON value");
        if body.len() > self.limits.request_bytes {
            return Err(ProviderError::new(413, "invalid_or_oversized_body"));
        }
        Ok(body)
    }
    fn deadline(&self, context: &RequestContext) -> ProviderResult<Instant> {
        if context.cancellation.is_cancelled() {
            return Err(ProviderError::new(503, "provider_cancelled"));
        }
        if context
            .deadline
            .is_some_and(|deadline| deadline <= std::time::Instant::now())
        {
            return Err(ProviderError::new(504, "provider_timeout"));
        }
        // Context remains owned by this request/stream worker and is never
        // forwarded to native HTTP or metadata.user_id. Turn-state is an
        // OpenAI server-issued routing token; no native equivalent is invented.
        if context.headers.iter().any(|(name, _)| {
            !self.config.local_runtime_context
                || !matches!(
                    name,
                    "session_id" | "x-client-request-id" | "x-codex-turn-metadata"
                )
        }) {
            return Err(ProviderError::new(400, "unsupported_native_context_header"));
        }
        Ok(context
            .deadline
            .map(Instant::from_std)
            .unwrap_or(Instant::now() + self.limits.total_timeout)
            .min(Instant::now() + self.limits.total_timeout))
    }
    async fn execute(
        &self,
        outgoing: reqwest::RequestBuilder,
        context: &RequestContext,
        deadline: Instant,
        media_type: &str,
    ) -> ProviderResult<reqwest::Response> {
        self.deadline(context)?;
        let header_deadline = deadline.min(Instant::now() + self.limits.header_timeout);
        let key = self.authentication_key(context, header_deadline).await?;
        if self.config.expected_organization.is_some() {
            // Resolve once: a key replacement between GET and POST must not
            // authenticate one organization and send history to another.
            self.authenticated_organization(&key, context, header_deadline)
                .await?;
        }
        let response = self
            .send(outgoing, &key, context, header_deadline, media_type)
            .await?;
        if let Some(expected) = &self.config.expected_organization {
            let organization = organization_header(response.headers())?
                .ok_or_else(|| ProviderError::new(502, "anthropic_invalid_organization"))?;
            if &organization != expected {
                return Err(ProviderError::new(
                    502,
                    "anthropic_response_organization_mismatch",
                ));
            }
        }
        Ok(response)
    }
    async fn authentication_key(
        &self,
        context: &RequestContext,
        header_deadline: Instant,
    ) -> ProviderResult<HeaderValue> {
        let broker = self.broker.clone();
        let reference = self.config.credential.clone();
        let secret = guard(
            tokio::task::spawn_blocking(move || broker.resolve(&reference)),
            context,
            header_deadline,
        )
        .await?
        .map_err(|_| ProviderError::new(503, "credential_unavailable"))?
        .map_err(|_| ProviderError::new(503, "credential_unavailable"))?
        .ok_or_else(|| ProviderError::new(503, "credential_missing"))?;
        let mut key = HeaderValue::from_str(secret.expose())
            .map_err(|_| ProviderError::new(503, "credential_invalid_header"))?;
        key.set_sensitive(true);
        Ok(key)
    }
    async fn authenticated_organization(
        &self,
        key: &HeaderValue,
        context: &RequestContext,
        deadline: Instant,
    ) -> ProviderResult<String> {
        let url = self
            .config
            .base
            .join("organizations/me")
            .expect("fixed relative path");
        let response = self
            .send(
                self.http.get(url),
                key,
                context,
                deadline,
                "application/json",
            )
            .await?;
        let header = organization_header(response.headers())?;
        let mut remaining = self.limits.response_bytes;
        let wire = self
            .read_json(response, context, deadline, &mut remaining)
            .await?;
        let id = wire["id"]
            .as_str()
            .filter(|id| valid_organization_id(id))
            .ok_or_else(|| ProviderError::new(502, "anthropic_invalid_organization"))?;
        if wire["type"] != "organization"
            || !wire["name"].is_string()
            || header.as_deref().is_some_and(|header| header != id)
        {
            return Err(ProviderError::new(502, "anthropic_invalid_organization"));
        }
        if self
            .config
            .expected_organization
            .as_deref()
            .is_some_and(|expected| expected != id)
        {
            return Err(ProviderError::new(400, "anthropic_organization_mismatch"));
        }
        Ok(id.to_owned())
    }
    async fn send(
        &self,
        mut outgoing: reqwest::RequestBuilder,
        key: &HeaderValue,
        context: &RequestContext,
        header_deadline: Instant,
        media_type: &str,
    ) -> ProviderResult<reqwest::Response> {
        let header_deadline = header_deadline.min(Instant::now() + self.limits.header_timeout);
        outgoing = outgoing
            .header("x-api-key", key.clone())
            .header("anthropic-version", "2023-06-01")
            .header("accept", media_type);
        if let Some(workspace) = &self.config.workspace {
            outgoing = outgoing.header("anthropic-workspace-id", workspace.clone());
        }
        let mut betas = Vec::new();
        if self.config.thinking_binding_controls {
            betas.push("thinking-binding-controls-2026-08-01");
        }
        if self.config.inline_tools {
            betas.push("inline-tools-2026-09-15");
        }
        if !betas.is_empty() {
            outgoing = outgoing.header("anthropic-beta", betas.join(","));
        }
        let response = guard(outgoing.send(), context, header_deadline)
            .await?
            .map_err(transport)?;
        let status = response.status();
        if !status.is_success() {
            let code = match status.as_u16() {
                401 | 403 => "provider_authentication_failed",
                429 => "provider_rate_limited",
                400 | 422 => "provider_request_rejected",
                300..=399 => "provider_redirect_blocked",
                500..=599 => "provider_unavailable",
                _ => "provider_http_error",
            };
            let mut error = ProviderError::new(
                if status.is_redirection() {
                    502
                } else {
                    status.as_u16()
                },
                code,
            );
            if status.as_u16() == 429 {
                error.retry_after_seconds = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| retry_after(v, SystemTime::now()));
            }
            return Err(error);
        }
        if !response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case(media_type)
            })
        {
            return Err(ProviderError::new(502, "provider_invalid_content_type"));
        }
        Ok(response)
    }
    async fn json(
        &self,
        outgoing: reqwest::RequestBuilder,
        context: &RequestContext,
        deadline: Instant,
        remaining: &mut usize,
    ) -> ProviderResult<(Value, ContextHeaders)> {
        let response = self
            .execute(outgoing, context, deadline, "application/json")
            .await?;
        let headers = response_headers(response.headers())?;
        let wire = self
            .read_json(response, context, deadline, remaining)
            .await?;
        Ok((wire, headers))
    }
    async fn read_json(
        &self,
        mut response: reqwest::Response,
        context: &RequestContext,
        deadline: Instant,
        remaining: &mut usize,
    ) -> ProviderResult<Value> {
        let mut bytes = Vec::new();
        while let Some(chunk) = guard(
            response.chunk(),
            context,
            deadline.min(Instant::now() + self.limits.idle_timeout),
        )
        .await?
        .map_err(transport)?
        {
            *remaining = remaining
                .checked_sub(chunk.len())
                .ok_or_else(|| ProviderError::new(502, "provider_oversized_response"))?;
            bytes.extend_from_slice(&chunk);
        }
        let wire = serde_json::from_slice(&bytes)
            .map_err(|_| ProviderError::new(502, "provider_invalid_json"))?;
        Ok(wire)
    }
}
pub(crate) fn valid_organization_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 1024 && id.bytes().all(|byte| (33..=126).contains(&byte))
}
fn organization_header(headers: &reqwest::header::HeaderMap) -> ProviderResult<Option<String>> {
    let invalid = || ProviderError::new(502, "anthropic_invalid_organization");
    let mut values = headers.get_all("anthropic-organization-id").iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    let id = value.to_str().map_err(|_| invalid())?;
    if values.next().is_some() || !valid_organization_id(id) {
        return Err(invalid());
    }
    Ok(Some(id.to_owned()))
}
pub(crate) fn response_headers(
    headers: &reqwest::header::HeaderMap,
) -> ProviderResult<ContextHeaders> {
    let invalid = || ProviderError::new(502, "provider_invalid_context_header");
    let mut context = ContextHeaders::default();
    for value in headers.get_all("request-id") {
        let value = value.to_str().map_err(|_| invalid())?;
        if value.trim().is_empty() {
            return Err(invalid());
        }
        context
            .insert("x-request-id", value.to_owned(), RESPONSE_HEADERS)
            .map_err(|_| invalid())?;
    }
    Ok(context)
}
pub(crate) fn transport(error: reqwest::Error) -> ProviderError {
    ProviderError::new(
        if error.is_timeout() { 504 } else { 502 },
        if error.is_timeout() {
            "provider_timeout"
        } else {
            "provider_transport_error"
        },
    )
}
