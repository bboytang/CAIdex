use crate::{CanonicalRequest, CanonicalResponse, ModelCapabilities, ModelMetadata, StreamEvent};
use caidex_credentials::CredentialRef;
use futures_core::Stream;
use serde::Serialize;
use std::{collections::BTreeMap, fmt, future::Future, pin::Pin, time::Instant};

pub type ProviderResult<T> = std::result::Result<T, ProviderError>;
pub use tokio_util::sync::CancellationToken;
pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = ProviderResult<T>> + Send + 'a>>;
pub type ProviderStream = Pin<Box<dyn Stream<Item = ProviderResult<ProviderStreamEvent>> + Send>>;

/// Errors are static classifications plus safe transport metadata. Never attach
/// reqwest errors, URLs, raw third-party bodies, prompts or keys here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderError {
    pub http_status: u16,
    pub code: &'static str,
    pub retry_after_seconds: Option<u64>,
}
impl ProviderError {
    pub fn new(http_status: u16, code: &'static str) -> Self {
        Self {
            http_status,
            code,
            retry_after_seconds: None,
        }
    }
    pub fn wire(&self) -> serde_json::Value {
        serde_json::json!({"type":"caidex_gateway_error", "code":self.code, "message":self.code})
    }
}
impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code)
    }
}
impl std::error::Error for ProviderError {}

pub const REQUEST_HEADERS: &[&str] = &[
    "session_id",
    "x-client-request-id",
    "x-codex-turn-state",
    "x-codex-turn-metadata",
];
pub const RESPONSE_HEADERS: &[&str] = &["x-request-id", "x-codex-turn-state"];

/// Explicit non-authentication context headers. Opaque turn-state values do not
/// implement Serialize/Display or appear in Debug; adapters expose borrowed wire.
#[derive(Clone, Default)]
pub struct ContextHeaders(BTreeMap<String, String>);
impl ContextHeaders {
    pub fn insert(&mut self, name: &str, value: String, allowed: &[&str]) -> ProviderResult<()> {
        let name = name.to_ascii_lowercase();
        if !allowed.contains(&name.as_str())
            || !REQUEST_HEADERS.contains(&name.as_str())
                && !RESPONSE_HEADERS.contains(&name.as_str())
            || value.len() > 8192
            || !value.bytes().all(|byte| (32..=126).contains(&byte))
        {
            return Err(ProviderError::new(400, "invalid_context_header"));
        }
        if self.0.contains_key(&name) {
            return Err(ProviderError::new(400, "duplicate_context_header"));
        }
        self.0.insert(name, value);
        Ok(())
    }
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }
}
impl fmt::Debug for ContextHeaders {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ContextHeaders([VALUES OMITTED])")
    }
}

#[derive(Default, Debug)]
pub struct RequestContext {
    pub headers: ContextHeaders,
    pub cancellation: CancellationToken,
    /// A caller's overall deadline also covers the adapter's request/stream.
    pub deadline: Option<Instant>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "authentication", rename_all = "camelCase")]
pub enum CredentialRequirement {
    None,
    Bearer { reference: CredentialRef },
}

pub struct ProviderResponse {
    pub response: CanonicalResponse,
    pub headers: ContextHeaders,
}
pub struct StreamingResponse {
    pub events: ProviderStream,
    pub headers: ContextHeaders,
}
#[derive(Debug)]
pub enum ProviderStreamEvent {
    Model(StreamEvent),
    Heartbeat,
}

/// Shared by Codex Gateway and ordinary Chat. Implementations only perform
/// inference; tools remain data. Drop response futures/streams to cancel I/O.
pub trait ModelProvider: Send + Sync {
    fn list_models(&self) -> ProviderFuture<'_, Vec<ModelMetadata>>;
    fn create_response(
        &self,
        request: CanonicalRequest,
        context: RequestContext,
    ) -> ProviderFuture<'_, ProviderResponse>;
    fn stream_response(
        &self,
        request: CanonicalRequest,
        context: RequestContext,
    ) -> ProviderFuture<'_, StreamingResponse>;
    fn capabilities(&self, model: &str) -> ProviderResult<ModelCapabilities>;
    fn metadata(&self, model: &str) -> ProviderResult<ModelMetadata>;
    fn credential_requirements(&self, model: &str) -> ProviderResult<CredentialRequirement>;
}
