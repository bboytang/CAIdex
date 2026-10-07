//! The HTTP boundary must constrain an injected adapter independently of its
//! own transport implementation. This test adapter deliberately ignores context.
use caidex_credentials::Redactor;
use caidex_model_core::{
    CanonicalRequest, CanonicalResponse, CapabilitySupport, ContextHeaders, CredentialRequirement,
    ModelCapabilities, ModelMetadata, ModelProvider, ProviderError, ProviderFuture,
    ProviderResponse, ProviderResult, REQUEST_HEADERS, RequestContext, ResponsesDialect,
    StreamingResponse,
};
use caidex_model_gateway::{Limits, RunningGateway, start_with_provider};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

struct Adapter {
    metadata: ModelMetadata,
    calls: AtomicUsize,
    drops: Arc<AtomicUsize>,
    stall: bool,
}
struct OnDrop(Arc<AtomicUsize>);
impl Drop for OnDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl ModelProvider for Adapter {
    fn list_models(&self) -> ProviderFuture<'_, Vec<ModelMetadata>> {
        Box::pin(async { Ok(vec![self.metadata.clone()]) })
    }
    fn metadata(&self, model: &str) -> ProviderResult<ModelMetadata> {
        if model != "fixture" {
            return Err(ProviderError::new(404, "unknown_model"));
        }
        Ok(self.metadata.clone())
    }
    fn capabilities(&self, model: &str) -> ProviderResult<ModelCapabilities> {
        Ok(self.metadata(model)?.capabilities)
    }
    fn credential_requirements(&self, _: &str) -> ProviderResult<CredentialRequirement> {
        Ok(CredentialRequirement::None)
    }
    fn create_response(
        &self,
        _: CanonicalRequest,
        _: RequestContext,
    ) -> ProviderFuture<'_, ProviderResponse> {
        Box::pin(async {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let _drop = OnDrop(self.drops.clone());
            if self.stall {
                std::future::pending::<()>().await;
            }
            let mut headers = ContextHeaders::default();
            // Valid as REQUEST context, invalid as RESPONSE context.
            headers
                .insert("session_id", "must-not-leak".into(), REQUEST_HEADERS)
                .unwrap();
            Ok(ProviderResponse {
                response: CanonicalResponse::new(
                    json!({"id":"fixture","status":"completed","output":[]}),
                )
                .unwrap(),
                headers,
            })
        })
    }
    fn stream_response(
        &self,
        _: CanonicalRequest,
        _: RequestContext,
    ) -> ProviderFuture<'_, StreamingResponse> {
        Box::pin(async {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let _drop = OnDrop(self.drops.clone());
            std::future::pending::<StreamingResponse>().await;
            unreachable!()
        })
    }
}
async fn start(
    metadata: ModelMetadata,
    stall: bool,
    limits: Limits,
) -> (RunningGateway, Arc<Adapter>) {
    let adapter = Arc::new(Adapter {
        metadata,
        calls: AtomicUsize::new(0),
        drops: Arc::new(AtomicUsize::new(0)),
        stall,
    });
    let gateway = start_with_provider(adapter.clone(), &Redactor::default(), limits)
        .await
        .unwrap();
    (gateway, adapter)
}
fn model() -> ModelMetadata {
    ModelMetadata::configured(
        "fixture".into(),
        "native-fixture".into(),
        vec![ResponsesDialect::Classic],
    )
}
async fn post(gateway: &RunningGateway, wire: Value, lite: bool) -> reqwest::Response {
    let mut request = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .body(wire.to_string());
    if lite {
        request = request.header("x-openai-internal-codex-responses-lite", "true");
    }
    tokio::time::timeout(Duration::from_secs(10), request.send())
        .await
        .unwrap()
        .unwrap()
}
async fn error(response: reqwest::Response, status: u16, code: &str) {
    assert_eq!(response.status(), status);
    assert!(response.headers().get("session_id").is_none());
    let body: Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert_eq!(body["error"]["code"], code);
    assert!(!body.to_string().contains("must-not-leak"));
}

#[tokio::test]
async fn injection_cannot_bypass_fixed_model_dialect_streaming_or_metadata_guards() {
    let mut metadata = model();
    metadata.capabilities.streaming = CapabilitySupport::Unsupported;
    let (gateway, adapter) = start(metadata, false, Limits::default()).await;
    for (model, stream, lite, status, code) in [
        ("unknown", false, false, 404, "unknown_model"),
        ("fixture", false, true, 400, "unsupported_dialect"),
        ("fixture", true, false, 400, "unsupported_streaming"),
    ] {
        error(
            post(
                &gateway,
                json!({"model":model,"input":[],"stream":stream}),
                lite,
            )
            .await,
            status,
            code,
        )
        .await;
    }
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 0);
    gateway.shutdown().await.unwrap();
    let mut metadata = model();
    metadata.id = "wrong-route".into();
    let (gateway, adapter) = start(metadata, false, Limits::default()).await;
    error(
        post(&gateway, json!({"model":"fixture","input":[]}), false).await,
        502,
        "provider_invalid_metadata",
    )
    .await;
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 0);
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn injection_rejects_wrong_direction_context_headers() {
    let (gateway, adapter) = start(model(), false, Limits::default()).await;
    error(
        post(&gateway, json!({"model":"fixture","input":[]}), false).await,
        502,
        "provider_invalid_context_header",
    )
    .await;
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 1);
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn gateway_owns_stream_header_and_nonstream_deadlines_even_if_adapter_ignores_context() {
    for stream in [false, true] {
        let limits = Limits {
            in_flight: 1,
            header_timeout: Duration::from_millis(250),
            total_timeout: Duration::from_millis(500),
            ..Limits::default()
        };
        let (gateway, adapter) = start(model(), true, limits).await;
        for _ in 0..2 {
            error(
                post(
                    &gateway,
                    json!({"model":"fixture","input":[],"stream":stream}),
                    false,
                )
                .await,
                504,
                "provider_timeout",
            )
            .await;
        }
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 2);
        assert_eq!(adapter.drops.load(Ordering::SeqCst), 2);
        gateway.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn shutdown_drops_uncooperative_adapter_future() {
    let (gateway, adapter) = start(model(), true, Limits::default()).await;
    let request = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .body(json!({"model":"fixture","input":[]}).to_string());
    let pending = tokio::spawn(async { request.send().await.unwrap() });
    tokio::time::timeout(Duration::from_secs(10), async {
        while adapter.calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    gateway.shutdown().await.unwrap();
    error(pending.await.unwrap(), 503, "gateway_stopped").await;
    assert_eq!(adapter.drops.load(Ordering::SeqCst), 1);
}
