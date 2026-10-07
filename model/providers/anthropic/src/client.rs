use crate::transfer::guard;
use crate::{ModelCatalog, ModelsPage, NativeMessage, NativeModel, NativeStreamingResponse};
use caidex_credentials::{Broker, CredentialRef, SecretKind, SecretStore};
use caidex_model_core::{ProviderError, ProviderResult, RequestContext};
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
            let wire = self
                .json(self.http.get(url), &context, deadline, &mut remaining)
                .await?;
            cursor = catalog.append(ModelsPage::parse(wire)?)?;
            if cursor.is_none() {
                return catalog.finish();
            }
        }
    }
    /// Native Messages only. No Responses translation or tool execution here.
    /// The configured model overrides the body's model before transmission.
    pub async fn create_message(
        &self,
        model: &str,
        mut wire: Value,
        context: RequestContext,
    ) -> ProviderResult<NativeMessage> {
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
        NativeMessage::parse(
            self.json(outgoing, &context, deadline, &mut remaining)
                .await?,
        )
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
        Ok(crate::transfer::stream(
            response,
            context,
            deadline,
            self.limits.clone(),
            permit,
        ))
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
        // Native context/header conversion is explicit subsequent Adapter work.
        if context.headers.iter().next().is_some() {
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
        mut outgoing: reqwest::RequestBuilder,
        context: &RequestContext,
        deadline: Instant,
        media_type: &str,
    ) -> ProviderResult<reqwest::Response> {
        self.deadline(context)?;
        let broker = self.broker.clone();
        let reference = self.config.credential.clone();
        let header_deadline = deadline.min(Instant::now() + self.limits.header_timeout);
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
        outgoing = outgoing
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01")
            .header("accept", media_type);
        if let Some(workspace) = &self.config.workspace {
            outgoing = outgoing.header("anthropic-workspace-id", workspace.clone());
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
    ) -> ProviderResult<Value> {
        let mut response = self
            .execute(outgoing, context, deadline, "application/json")
            .await?;
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
        serde_json::from_slice(&bytes).map_err(|_| ProviderError::new(502, "provider_invalid_json"))
    }
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
