use crate::{ClientOptions, Error, Limits, ModelCatalog, ModelsPage, NativeModel};
use caidex_credentials::{Broker, CredentialRef, SecretKind, SecretStore};
use caidex_model_core::{ProviderError, ProviderResult, RequestContext};
use caidex_provider_custom::{CustomResponses, retry_after};
use reqwest::{Url, header::HeaderValue};
use serde_json::Value;
use std::{fmt, future::Future, sync::Arc, time::SystemTime};
use tokio::{sync::Semaphore, time::Instant};

/// Executor-owned native Gemini API profile. URLs and authentication cannot be
/// selected by a model body, paging token or incoming HTTP header.
pub struct GeminiConfig {
    base: Url,
    credential: CredentialRef,
}
impl GeminiConfig {
    pub fn new(credential: CredentialRef) -> Result<Self, Error> {
        if credential.provider.as_str() != "google" || credential.kind != SecretKind::ApiKey {
            return Err(Error::InvalidRoute);
        }
        Ok(Self {
            base: Url::parse("https://generativelanguage.googleapis.com/v1beta/")
                .expect("fixed URL"),
            credential,
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
}
impl fmt::Debug for GeminiConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GeminiConfig([PROFILE OMITTED])")
    }
}

pub struct GeminiClient<S: SecretStore> {
    config: GeminiConfig,
    broker: Arc<Broker<S>>,
    http: reqwest::Client,
    limits: Limits,
    permits: Semaphore,
}
impl<S: SecretStore + 'static> GeminiClient<S> {
    pub fn new(
        config: GeminiConfig,
        broker: Arc<Broker<S>>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::with_options(config, broker, limits, ClientOptions::default())
    }
    pub fn with_options(
        config: GeminiConfig,
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
            permits: Semaphore::new(limits.in_flight),
            limits,
        })
    }
    /// Every page shares one permit, absolute deadline and total byte budget.
    /// Tokens are encoded as query data; secrets only enter a sensitive header.
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
            url.query_pairs_mut().append_pair("pageSize", "1000");
            if let Some(cursor) = &cursor {
                url.query_pairs_mut().append_pair("pageToken", cursor);
            }
            if url.as_str().len() > self.limits.request_bytes {
                return Err(ProviderError::new(413, "invalid_or_oversized_body"));
            }
            let wire = self.json(url, &context, deadline, &mut remaining).await?;
            cursor = catalog.append(ModelsPage::parse(wire)?)?;
            if cursor.is_none() {
                return catalog.finish();
            }
        }
    }
    fn deadline(&self, context: &RequestContext) -> ProviderResult<Instant> {
        if context.cancellation.is_cancelled() {
            return Err(ProviderError::new(503, "provider_cancelled"));
        }
        let deadline = context
            .deadline
            .map(Instant::from_std)
            .unwrap_or(Instant::now() + self.limits.total_timeout)
            .min(Instant::now() + self.limits.total_timeout);
        if deadline <= Instant::now() {
            return Err(ProviderError::new(504, "provider_timeout"));
        }
        if context.headers.iter().next().is_some() {
            return Err(ProviderError::new(400, "unsupported_native_context_header"));
        }
        Ok(deadline)
    }
    async fn json(
        &self,
        url: Url,
        context: &RequestContext,
        deadline: Instant,
        remaining: &mut usize,
    ) -> ProviderResult<Value> {
        let header_deadline = deadline.min(Instant::now() + self.limits.header_timeout);
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
        let mut response = guard(
            self.http
                .get(url)
                .header("x-goog-api-key", key)
                .header("accept", "application/json")
                .send(),
            context,
            header_deadline,
        )
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
                    .eq_ignore_ascii_case("application/json")
            })
        {
            return Err(ProviderError::new(502, "provider_invalid_content_type"));
        }
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
async fn guard<T>(
    operation: impl Future<Output = T>,
    context: &RequestContext,
    deadline: Instant,
) -> ProviderResult<T> {
    tokio::select! {
        biased;
        _=context.cancellation.cancelled()=>Err(ProviderError::new(503,"provider_cancelled")),
        _=tokio::time::sleep_until(deadline)=>Err(ProviderError::new(504,"provider_timeout")),
        result=operation=>Ok(result),
    }
}
fn transport(error: reqwest::Error) -> ProviderError {
    ProviderError::new(
        if error.is_timeout() { 504 } else { 502 },
        if error.is_timeout() {
            "provider_timeout"
        } else {
            "provider_transport_error"
        },
    )
}
