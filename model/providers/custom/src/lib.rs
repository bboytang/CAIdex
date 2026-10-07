//! Custom Responses inference client shared by Gateway and ordinary Chat.
//! No server, shell, tool executor or implicit credential discovery.
mod config;
mod limits;
mod transfer;

pub use config::{ConfiguredModel, CustomResponses};
pub use limits::Limits;

use caidex_credentials::{Broker, SecretStore};
use caidex_model_core::{
    CanonicalRequest, CapabilitySupport, ContextHeaders, CredentialRequirement, ModelCapabilities,
    ModelMetadata, ModelProvider, ModelRegistry, ProviderError, ProviderFuture, ProviderResponse,
    ProviderResult, REQUEST_HEADERS, RESPONSE_HEADERS, RequestContext, StreamingResponse,
};
use reqwest::{
    StatusCode,
    header::{self, HeaderMap, HeaderValue},
};
use std::{collections::HashMap, sync::Arc, time::SystemTime};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    time::Instant,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid provider endpoint")]
    InvalidEndpoint,
    #[error("invalid or duplicate model route")]
    InvalidRoute,
    #[error("invalid gateway limits")]
    InvalidLimits,
    #[error("provider could not initialize")]
    Initialization,
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Default)]
pub struct ClientOptions {
    /// Explicit extra trust roots for this configured client; native validation
    /// still checks issuer, hostname and validity. Never disable verification.
    pub root_certificates: Vec<reqwest::Certificate>,
}

struct ProviderState<S: SecretStore> {
    models: HashMap<String, ConfiguredModel>,
    registry: ModelRegistry,
    broker: Arc<Broker<S>>,
    client: reqwest::Client,
    limits: Limits,
    permits: Arc<Semaphore>,
}
pub struct CustomResponsesProvider<S: SecretStore> {
    state: Arc<ProviderState<S>>,
}
impl<S: SecretStore + 'static> CustomResponsesProvider<S> {
    pub fn new(
        models: Vec<ConfiguredModel>,
        broker: Arc<Broker<S>>,
        limits: Limits,
    ) -> Result<Self> {
        Self::with_options(models, broker, limits, ClientOptions::default())
    }
    pub fn with_options(
        models: Vec<ConfiguredModel>,
        broker: Arc<Broker<S>>,
        limits: Limits,
        options: ClientOptions,
    ) -> Result<Self> {
        limits.validate()?;
        let registry = ModelRegistry::new(models.iter().map(|model| model.metadata.clone()))
            .map_err(|_| Error::InvalidRoute)?;
        let models = models
            .into_iter()
            .map(|model| (model.metadata.id.clone(), model))
            .collect();
        let mut client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(limits.connect_timeout)
            .pool_max_idle_per_host(0);
        for certificate in options.root_certificates {
            client = client.add_root_certificate(certificate);
        }
        let client = client.build().map_err(|_| Error::Initialization)?;
        Ok(Self {
            state: Arc::new(ProviderState {
                models,
                registry,
                broker,
                client,
                permits: Arc::new(Semaphore::new(limits.in_flight)),
                limits,
            }),
        })
    }
}

impl<S: SecretStore + 'static> ModelProvider for CustomResponsesProvider<S> {
    fn list_models(&self) -> ProviderFuture<'_, Vec<ModelMetadata>> {
        Box::pin(async { Ok(self.state.registry.models().cloned().collect()) })
    }
    fn metadata(&self, model: &str) -> ProviderResult<ModelMetadata> {
        self.state
            .registry
            .get(model)
            .cloned()
            .ok_or_else(|| ProviderError::new(404, "unknown_model"))
    }
    fn capabilities(&self, model: &str) -> ProviderResult<ModelCapabilities> {
        Ok(self.metadata(model)?.capabilities)
    }
    fn credential_requirements(&self, model: &str) -> ProviderResult<CredentialRequirement> {
        let model = self
            .state
            .models
            .get(model)
            .ok_or_else(|| ProviderError::new(404, "unknown_model"))?;
        Ok(match &model.adapter.credential {
            Some(reference) => CredentialRequirement::Bearer {
                reference: reference.clone(),
            },
            None => CredentialRequirement::None,
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
            let (upstream, _permit, deadline) = send(&self.state, &request, &context).await?;
            if !is_media_type(upstream.headers(), "application/json") {
                return Err(ProviderError::new(502, "provider_invalid_content_type"));
            }
            let headers = response_headers(upstream.headers())?;
            let response = transfer::json(upstream, &self.state, &context, deadline).await?;
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
            let (upstream, permit, deadline) = send(&self.state, &request, &context).await?;
            if !is_media_type(upstream.headers(), "text/event-stream") {
                return Err(ProviderError::new(502, "provider_invalid_content_type"));
            }
            let headers = response_headers(upstream.headers())?;
            Ok(StreamingResponse {
                headers,
                events: transfer::stream(upstream, self.state.clone(), context, deadline, permit),
            })
        })
    }
}

pub fn context_headers(headers: &HeaderMap, allowed: &[&str]) -> ProviderResult<ContextHeaders> {
    let mut context = ContextHeaders::default();
    for name in allowed {
        for value in headers.get_all(*name) {
            context.insert(
                name,
                value
                    .to_str()
                    .map_err(|_| ProviderError::new(400, "invalid_context_header"))?
                    .to_owned(),
                allowed,
            )?;
        }
    }
    Ok(context)
}
fn response_headers(headers: &HeaderMap) -> ProviderResult<ContextHeaders> {
    context_headers(headers, RESPONSE_HEADERS)
        .map_err(|_| ProviderError::new(502, "provider_invalid_context_header"))
}
fn is_media_type(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case(expected)
        })
}
fn transport(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::new(504, "provider_timeout")
    } else {
        ProviderError::new(502, "provider_transport_error")
    }
}

async fn send<S: SecretStore + 'static>(
    state: &ProviderState<S>,
    request: &CanonicalRequest,
    context: &RequestContext,
) -> ProviderResult<(reqwest::Response, OwnedSemaphorePermit, Instant)> {
    let deadline = context
        .deadline
        .map(Instant::from_std)
        .unwrap_or(Instant::now() + state.limits.total_timeout)
        .min(Instant::now() + state.limits.total_timeout);
    if context.cancellation.is_cancelled() {
        return Err(ProviderError::new(503, "provider_cancelled"));
    }
    if deadline <= Instant::now() {
        return Err(ProviderError::new(504, "provider_timeout"));
    }
    let model = state
        .models
        .get(request.model())
        .ok_or_else(|| ProviderError::new(404, "unknown_model"))?;
    if !model.metadata.dialects.contains(&request.dialect()) {
        return Err(ProviderError::new(400, "unsupported_dialect"));
    }
    if request.is_streaming()
        && model.metadata.capabilities.streaming == CapabilitySupport::Unsupported
    {
        return Err(ProviderError::new(400, "unsupported_streaming"));
    }
    let permit = state
        .permits
        .clone()
        .try_acquire_owned()
        .map_err(|_| ProviderError::new(503, "provider_busy"))?;
    let mut wire = request.wire().clone();
    wire["model"] = model.metadata.native_model.clone().into();
    let bytes = serde_json::to_vec(&wire).expect("validated JSON");
    if bytes.len() > state.limits.request_bytes {
        return Err(ProviderError::new(413, "invalid_or_oversized_body"));
    }
    let mut outgoing = state
        .client
        .post(model.adapter.endpoint.clone())
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::ACCEPT,
            if request.is_streaming() {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .body(bytes);
    for (name, value) in context.headers.iter() {
        if !REQUEST_HEADERS.contains(&name) {
            return Err(ProviderError::new(400, "invalid_context_header"));
        }
        let mut value = HeaderValue::from_str(value)
            .map_err(|_| ProviderError::new(400, "invalid_context_header"))?;
        value.set_sensitive(true);
        outgoing = outgoing.header(name, value);
    }
    if let Some((name, value)) = request.dialect().lite_header() {
        outgoing = outgoing.header(name, value);
    }
    if let Some(reference) = model.adapter.credential.clone() {
        let broker = state.broker.clone();
        let secret = transfer::guard(
            async move { tokio::task::spawn_blocking(move || broker.resolve(&reference)).await },
            &context.cancellation,
            deadline.min(Instant::now() + state.limits.header_timeout),
        )
        .await?
        .map_err(|_| ProviderError::new(503, "credential_unavailable"))?
        .map_err(|_| ProviderError::new(503, "credential_unavailable"))?
        .ok_or_else(|| ProviderError::new(503, "credential_missing"))?;
        let mut value = HeaderValue::from_str(&format!("Bearer {}", secret.expose()))
            .map_err(|_| ProviderError::new(503, "credential_invalid_header"))?;
        value.set_sensitive(true);
        outgoing = outgoing.header(header::AUTHORIZATION, value);
    }
    let upstream = transfer::guard(
        outgoing.send(),
        &context.cancellation,
        deadline.min(Instant::now() + state.limits.header_timeout),
    )
    .await?
    .map_err(transport)?;
    let status = upstream.status();
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
        if status == StatusCode::TOO_MANY_REQUESTS {
            error.retry_after_seconds = upstream
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| retry_after(value, SystemTime::now()));
        }
        return Err(error);
    }
    Ok((upstream, permit, deadline))
}

/// RFC Retry-After accepts delta seconds or an HTTP-date. Date hints are rounded
/// up to avoid resuming early; this returns metadata and never retries a POST.
pub fn retry_after(value: &str, now: SystemTime) -> Option<u64> {
    let seconds = if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        value.parse().ok()?
    } else {
        let delay = httpdate::parse_http_date(value)
            .ok()?
            .duration_since(now)
            .unwrap_or_default();
        delay
            .as_secs()
            .checked_add(u64::from(delay.subsec_nanos() > 0))?
    };
    (seconds <= 86400).then_some(seconds)
}
