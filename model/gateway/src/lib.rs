//! Executor-local Responses HTTP boundary. No tool execution, implicit route,
//! retries, redirects, credential export, or provider history reconstruction.

mod transfer;

use caidex_provider_custom::CustomResponsesProvider;
pub use caidex_provider_custom::{ConfiguredModel as ModelRoute, CustomResponses, Limits};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
    routing::post,
};
use caidex_credentials::{Broker, Redactor, Secret, SecretStore};
use caidex_model_core::{
    CancellationToken, CanonicalRequest, CapabilitySupport, ModelProvider, ProviderError,
    RequestContext, ResponsesDialect,
};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use subtle::ConstantTimeEq;
use tokio::{net::TcpListener, sync::Semaphore, task::JoinHandle, time::Instant};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid provider endpoint")]
    InvalidEndpoint,
    #[error("invalid or duplicate model route")]
    InvalidRoute,
    #[error("invalid gateway limits")]
    InvalidLimits,
    #[error("gateway could not initialize")]
    Initialization,
    #[error("gateway could not shut down cleanly")]
    Shutdown,
}
pub type Result<T> = std::result::Result<T, Error>;

struct GatewayState {
    provider: Arc<dyn ModelProvider>,
    token: Arc<Secret>,
    limits: Limits,
    permits: Arc<Semaphore>,
    cancellation: CancellationToken,
}

/// Give the token only to a local Runtime through its isolated environment. It
/// authenticates this listener and is unrelated to any provider credential.
pub struct RunningGateway {
    address: SocketAddr,
    token: Arc<Secret>,
    cancellation: CancellationToken,
    task: Option<JoinHandle<std::io::Result<()>>>,
}
impl RunningGateway {
    pub fn address(&self) -> SocketAddr {
        self.address
    }
    pub fn token(&self) -> &Secret {
        &self.token
    }
    pub async fn shutdown(mut self) -> Result<()> {
        self.cancellation.cancel();
        let mut task = self.task.take().expect("gateway task exists");
        match tokio::time::timeout(Duration::from_secs(5), &mut task).await {
            Ok(Ok(Ok(()))) => Ok(()),
            _ => {
                task.abort();
                Err(Error::Shutdown)
            }
        }
    }
}
impl Drop for RunningGateway {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

pub async fn start<S: SecretStore + 'static>(
    routes: Vec<ModelRoute>,
    broker: Arc<Broker<S>>,
    limits: Limits,
) -> Result<RunningGateway> {
    limits.validate().map_err(configuration_error)?;
    let provider = Arc::new(
        CustomResponsesProvider::new(routes, broker.clone(), limits.clone())
            .map_err(configuration_error)?,
    );
    start_with_provider(provider, broker.redactor(), limits).await
}

/// Inject a configured native adapter (or provider router) without changing the
/// Codex HTTP contract. Register the listener token in the executor's redactor.
/// This does not list models, read credentials, or start inference at startup.
pub async fn start_with_provider(
    provider: Arc<dyn ModelProvider>,
    redactor: &Redactor,
    limits: Limits,
) -> Result<RunningGateway> {
    limits.validate().map_err(configuration_error)?;
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).map_err(|_| Error::Initialization)?;
    let token = Arc::new(
        Secret::new(random.iter().map(|byte| format!("{byte:02x}")).collect())
            .map_err(|_| Error::Initialization)?,
    );
    redactor
        .register(&token)
        .map_err(|_| Error::Initialization)?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| Error::Initialization)?;
    let address = listener.local_addr().map_err(|_| Error::Initialization)?;
    let cancellation = CancellationToken::new();
    let state = Arc::new(GatewayState {
        provider,
        token: token.clone(),
        permits: Arc::new(Semaphore::new(limits.in_flight)),
        limits,
        cancellation: cancellation.clone(),
    });
    let app = Router::new()
        .route("/v1/responses", post(responses))
        .with_state(state);
    let server_cancel = cancellation.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(server_cancel.cancelled_owned())
            .await
    });
    Ok(RunningGateway {
        address,
        token,
        cancellation,
        task: Some(task),
    })
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Failure {
    status: StatusCode,
    code: &'static str,
    retry_after_seconds: Option<u64>,
}
impl Failure {
    fn new(status: StatusCode, code: &'static str) -> Self {
        Self {
            status,
            code,
            retry_after_seconds: None,
        }
    }
    fn wire(self) -> serde_json::Value {
        serde_json::json!({"type":"caidex_gateway_error", "code":self.code, "message":self.code})
    }
    fn response(self) -> Response {
        let mut response = Response::builder()
            .status(self.status)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(
                serde_json::json!({"error":self.wire()}).to_string(),
            ))
            .expect("static response headers");
        if let Some(seconds) = self.retry_after_seconds {
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from_str(&seconds.to_string()).expect("numeric hint"),
            );
        }
        response
    }
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

async fn responses(State(state): State<Arc<GatewayState>>, request: Request) -> Response {
    match handle(state, request).await {
        Ok(response) => response,
        Err(failure) => failure.response(),
    }
}

async fn handle(
    state: Arc<GatewayState>,
    request: Request,
) -> std::result::Result<Response, Failure> {
    let headers = request.headers();
    let authorization = headers.get_all(header::AUTHORIZATION);
    let token = authorization
        .iter()
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if authorization.iter().count() != 1
        || !token.is_some_and(|token| {
            bool::from(token.as_bytes().ct_eq(state.token.expose().as_bytes()))
        })
    {
        return Err(Failure::new(
            StatusCode::UNAUTHORIZED,
            "gateway_unauthorized",
        ));
    }
    if headers.contains_key(header::ORIGIN) || request.uri().query().is_some() {
        return Err(Failure::new(
            StatusCode::FORBIDDEN,
            "gateway_request_forbidden",
        ));
    }
    if !is_media_type(headers, "application/json")
        || headers
            .get(header::CONTENT_ENCODING)
            .is_some_and(|value| value != "identity")
    {
        return Err(Failure::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_content_type",
        ));
    }
    let lite = headers.get_all("x-openai-internal-codex-responses-lite");
    let dialect = match (lite.iter().count(), lite.iter().next()) {
        (0, _) => ResponsesDialect::Classic,
        (1, Some(value)) if value == "true" => ResponsesDialect::Lite,
        _ => return Err(Failure::new(StatusCode::BAD_REQUEST, "invalid_dialect")),
    };
    let context_headers =
        caidex_provider_custom::context_headers(headers, caidex_model_core::REQUEST_HEADERS)
            .map_err(model_error)?;
    let permit = state
        .permits
        .clone()
        .try_acquire_owned()
        .map_err(|_| Failure::new(StatusCode::SERVICE_UNAVAILABLE, "gateway_busy"))?;
    let deadline = Instant::now() + state.limits.total_timeout;
    let bytes = transfer::guard(
        to_bytes(request.into_body(), state.limits.request_bytes),
        state.cancellation.clone(),
        deadline.min(Instant::now() + state.limits.header_timeout),
    )
    .await?
    .map_err(|_| Failure::new(StatusCode::PAYLOAD_TOO_LARGE, "invalid_or_oversized_body"))?;
    let wire = serde_json::from_slice(&bytes)
        .map_err(|_| Failure::new(StatusCode::BAD_REQUEST, "invalid_json"))?;
    let request = CanonicalRequest::new(wire, dialect)
        .map_err(|_| Failure::new(StatusCode::BAD_REQUEST, "invalid_model_request"))?;
    let metadata = state
        .provider
        .metadata(request.model())
        .map_err(model_error)?;
    if metadata.id != request.model() || metadata.validate().is_err() {
        return Err(Failure::new(
            StatusCode::BAD_GATEWAY,
            "provider_invalid_metadata",
        ));
    }
    if !metadata.dialects.contains(&dialect) {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "unsupported_dialect"));
    }
    if request.is_streaming() && metadata.capabilities.streaming == CapabilitySupport::Unsupported {
        return Err(Failure::new(
            StatusCode::BAD_REQUEST,
            "unsupported_streaming",
        ));
    }
    let context = RequestContext {
        headers: context_headers,
        cancellation: state.cancellation.child_token(),
        deadline: Some(deadline.into_std()),
    };
    if request.is_streaming() {
        let result = transfer::guard(
            state.provider.stream_response(request, context),
            state.cancellation.clone(),
            deadline.min(Instant::now() + state.limits.header_timeout),
        )
        .await?
        .map_err(model_error)?;
        let mut response = Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-store")
            .header("x-accel-buffering", "no")
            .body(transfer::stream(
                result.events,
                state.cancellation.child_token(),
                deadline,
                permit,
            ))
            .expect("static headers");
        transfer::headers(&mut response, &result.headers)?;
        Ok(response)
    } else {
        let result = transfer::guard(
            state.provider.create_response(request, context),
            state.cancellation.clone(),
            deadline,
        )
        .await?
        .map_err(model_error)?;
        let mut response = Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(
                serde_json::to_vec(&result.response).expect("validated response"),
            ))
            .expect("static headers");
        transfer::headers(&mut response, &result.headers)?;
        Ok(response)
    }
}

fn model_error(error: ProviderError) -> Failure {
    Failure {
        status: StatusCode::from_u16(error.http_status).unwrap_or(StatusCode::BAD_GATEWAY),
        code: error.code,
        retry_after_seconds: error.retry_after_seconds,
    }
}
fn configuration_error(error: caidex_provider_custom::Error) -> Error {
    match error {
        caidex_provider_custom::Error::InvalidEndpoint => Error::InvalidEndpoint,
        caidex_provider_custom::Error::InvalidRoute => Error::InvalidRoute,
        caidex_provider_custom::Error::InvalidScope => Error::InvalidRoute,
        caidex_provider_custom::Error::InvalidLimits => Error::InvalidLimits,
        caidex_provider_custom::Error::Initialization => Error::Initialization,
    }
}
