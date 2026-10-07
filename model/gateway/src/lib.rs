//! Executor-local Responses HTTP boundary. No tool execution, implicit route,
//! retries, redirects, credential export, or provider history reconstruction.

mod custom;
mod transfer;

pub use custom::{CustomResponses, ModelRoute};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
    routing::post,
};
use caidex_credentials::{Broker, Secret, SecretStore};
use caidex_model_core::{CanonicalRequest, ResponsesDialect};
use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};
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

#[derive(Clone, Debug)]
pub struct Limits {
    pub request_bytes: usize,
    pub frame_bytes: usize,
    pub response_bytes: usize,
    pub in_flight: usize,
    pub connect_timeout: Duration,
    pub header_timeout: Duration,
    pub idle_timeout: Duration,
    pub total_timeout: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            request_bytes: 8 * 1024 * 1024,
            frame_bytes: 2 * 1024 * 1024,
            response_bytes: 16 * 1024 * 1024,
            in_flight: 16,
            connect_timeout: Duration::from_secs(10),
            header_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(90),
            total_timeout: Duration::from_secs(600),
        }
    }
}
impl Limits {
    fn validate(&self) -> Result<()> {
        if self.request_bytes == 0
            || self.frame_bytes == 0
            || self.response_bytes == 0
            || self.in_flight == 0
            || self.in_flight > Semaphore::MAX_PERMITS
            || [
                self.connect_timeout,
                self.header_timeout,
                self.idle_timeout,
                self.total_timeout,
            ]
            .iter()
            .any(|duration| duration.is_zero() || Instant::now().checked_add(*duration).is_none())
        {
            return Err(Error::InvalidLimits);
        }
        Ok(())
    }
}

struct GatewayState<S: SecretStore> {
    routes: HashMap<String, ModelRoute>,
    broker: Arc<Broker<S>>,
    token: Arc<Secret>,
    client: reqwest::Client,
    limits: Limits,
    permits: Arc<Semaphore>,
    shutdown: tokio::sync::watch::Receiver<bool>,
}

/// Give the token only to a local Runtime through its isolated environment. It
/// authenticates this listener and is unrelated to any provider credential.
pub struct RunningGateway {
    address: SocketAddr,
    token: Arc<Secret>,
    shutdown: tokio::sync::watch::Sender<bool>,
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
        self.shutdown.send_replace(true);
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
        self.shutdown.send_replace(true);
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
    limits.validate()?;
    let mut configured = HashMap::new();
    for route in routes {
        if configured.insert(route.model.clone(), route).is_some() {
            return Err(Error::InvalidRoute);
        }
    }
    if configured.is_empty() {
        return Err(Error::InvalidRoute);
    }
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).map_err(|_| Error::Initialization)?;
    let token = Arc::new(
        Secret::new(random.iter().map(|byte| format!("{byte:02x}")).collect())
            .map_err(|_| Error::Initialization)?,
    );
    broker
        .redactor()
        .register(&token)
        .map_err(|_| Error::Initialization)?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(limits.connect_timeout)
        .pool_max_idle_per_host(0)
        .build()
        .map_err(|_| Error::Initialization)?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| Error::Initialization)?;
    let address = listener.local_addr().map_err(|_| Error::Initialization)?;
    let (shutdown, rx) = tokio::sync::watch::channel(false);
    let state = Arc::new(GatewayState {
        routes: configured,
        broker,
        token: token.clone(),
        client,
        permits: Arc::new(Semaphore::new(limits.in_flight)),
        limits,
        shutdown: rx.clone(),
    });
    let app = Router::new()
        .route("/v1/responses", post(responses::<S>))
        .with_state(state);
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(transfer::cancelled(rx))
            .await
    });
    Ok(RunningGateway {
        address,
        token,
        shutdown,
        task: Some(task),
    })
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Failure {
    status: StatusCode,
    code: &'static str,
}
impl Failure {
    fn new(status: StatusCode, code: &'static str) -> Self {
        Self { status, code }
    }
    fn wire(self) -> serde_json::Value {
        serde_json::json!({"type":"caidex_gateway_error", "code":self.code, "message":self.code})
    }
    fn response(self) -> Response {
        Response::builder()
            .status(self.status)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(
                serde_json::json!({"error":self.wire()}).to_string(),
            ))
            .expect("static response headers")
    }
    fn transport(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::new(StatusCode::GATEWAY_TIMEOUT, "provider_timeout")
        } else {
            Self::new(StatusCode::BAD_GATEWAY, "provider_transport_error")
        }
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

async fn responses<S: SecretStore + 'static>(
    State(state): State<Arc<GatewayState<S>>>,
    request: Request,
) -> Response {
    match handle(state, request).await {
        Ok(response) => response,
        Err(failure) => failure.response(),
    }
}

async fn handle<S: SecretStore + 'static>(
    state: Arc<GatewayState<S>>,
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
    let permit = state
        .permits
        .clone()
        .try_acquire_owned()
        .map_err(|_| Failure::new(StatusCode::SERVICE_UNAVAILABLE, "gateway_busy"))?;
    let deadline = Instant::now() + state.limits.total_timeout;
    let bytes = transfer::guard(
        to_bytes(request.into_body(), state.limits.request_bytes),
        state.shutdown.clone(),
        deadline.min(Instant::now() + state.limits.header_timeout),
    )
    .await?
    .map_err(|_| Failure::new(StatusCode::PAYLOAD_TOO_LARGE, "invalid_or_oversized_body"))?;
    let wire = serde_json::from_slice(&bytes)
        .map_err(|_| Failure::new(StatusCode::BAD_REQUEST, "invalid_json"))?;
    let request = CanonicalRequest::new(wire, dialect)
        .map_err(|_| Failure::new(StatusCode::BAD_REQUEST, "invalid_model_request"))?;
    let route = state
        .routes
        .get(request.model())
        .ok_or_else(|| Failure::new(StatusCode::NOT_FOUND, "unknown_model"))?;
    if !route.dialects.contains(&dialect) {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "unsupported_dialect"));
    }
    let mut wire = request.wire().clone();
    wire["model"] = route.upstream_model.clone().into();
    let mut outgoing = state
        .client
        .post(route.adapter.endpoint.clone())
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::ACCEPT,
            if request.is_streaming() {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .body(serde_json::to_vec(&wire).expect("validated JSON"));
    if let Some((name, value)) = dialect.lite_header() {
        outgoing = outgoing.header(name, value);
    }
    if let Some(reference) = route.adapter.credential.clone() {
        let broker = state.broker.clone();
        let secret = transfer::guard(
            tokio::task::spawn_blocking(move || broker.resolve(&reference)),
            state.shutdown.clone(),
            deadline.min(Instant::now() + state.limits.header_timeout),
        )
        .await?
        .map_err(|_| Failure::new(StatusCode::SERVICE_UNAVAILABLE, "credential_unavailable"))?
        .map_err(|_| Failure::new(StatusCode::SERVICE_UNAVAILABLE, "credential_unavailable"))?
        .ok_or_else(|| Failure::new(StatusCode::SERVICE_UNAVAILABLE, "credential_missing"))?;
        let mut value =
            HeaderValue::from_str(&format!("Bearer {}", secret.expose())).map_err(|_| {
                Failure::new(StatusCode::SERVICE_UNAVAILABLE, "credential_invalid_header")
            })?;
        value.set_sensitive(true);
        outgoing = outgoing.header(header::AUTHORIZATION, value);
    }
    let upstream = transfer::guard(
        outgoing.send(),
        state.shutdown.clone(),
        deadline.min(Instant::now() + state.limits.header_timeout),
    )
    .await?
    .map_err(Failure::transport)?;
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
        let mut response = Failure::new(
            if status.is_redirection() {
                StatusCode::BAD_GATEWAY
            } else {
                status
            },
            code,
        )
        .response();
        if status == StatusCode::TOO_MANY_REQUESTS
            && let Some(value) = upstream.headers().get(header::RETRY_AFTER)
            && value.to_str().ok().is_some_and(|value| {
                !value.is_empty()
                    && value.bytes().all(|byte| byte.is_ascii_digit())
                    && value.parse::<u64>().is_ok_and(|seconds| seconds <= 86400)
            })
        {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, value.clone());
        }
        // Do not read/reflect arbitrary upstream error bodies, cookies or URLs.
        return Ok(response);
    }
    if request.is_streaming() {
        if !is_media_type(upstream.headers(), "text/event-stream") {
            return Err(Failure::new(
                StatusCode::BAD_GATEWAY,
                "provider_invalid_content_type",
            ));
        }
        let body = transfer::stream(upstream, state, deadline, permit);
        Ok(Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-store")
            .header("x-accel-buffering", "no")
            .body(body)
            .expect("static response headers"))
    } else {
        if !is_media_type(upstream.headers(), "application/json") {
            return Err(Failure::new(
                StatusCode::BAD_GATEWAY,
                "provider_invalid_content_type",
            ));
        }
        let bytes = transfer::json(upstream, &state, deadline).await?;
        Ok(Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(bytes))
            .expect("static response headers"))
    }
}
