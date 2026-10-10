use caidex_model_core::*;
use futures_core::Stream;
use serde_json::{Value, json};
use std::{
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

fn ready<T>(mut future: ProviderFuture<'_, T>) -> ProviderResult<T> {
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("fixture unexpectedly pending"),
    }
}

fn model(id: &str) -> ModelMetadata {
    ModelMetadata::configured(
        id.into(),
        format!("native-{id}"),
        vec![ResponsesDialect::Classic],
    )
}

#[derive(Default)]
struct Calls {
    catalog: usize,
    requests: Vec<(Value, RequestContext)>,
}

struct Adapter {
    models: Mutex<Vec<ModelMetadata>>,
    catalog: Mutex<Vec<ModelMetadata>>,
    failure: Option<ProviderError>,
    calls: Mutex<Calls>,
    drops: Arc<AtomicUsize>,
}
impl Adapter {
    fn new(models: Vec<ModelMetadata>, failure: Option<ProviderError>) -> Arc<Self> {
        Arc::new(Self {
            catalog: Mutex::new(models.clone()),
            models: Mutex::new(models),
            failure,
            calls: Mutex::new(Calls::default()),
            drops: Arc::new(AtomicUsize::new(0)),
        })
    }
}
struct Events(Arc<AtomicUsize>);
impl Stream for Events {
    type Item = ProviderResult<ProviderStreamEvent>;
    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(Some(Ok(ProviderStreamEvent::Heartbeat)))
    }
}
impl Drop for Events {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl ModelProvider for Adapter {
    fn list_models(&self) -> ProviderFuture<'_, Vec<ModelMetadata>> {
        Box::pin(async move {
            self.calls.lock().unwrap().catalog += 1;
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            Ok(self.catalog.lock().unwrap().clone())
        })
    }
    fn metadata(&self, id: &str) -> ProviderResult<ModelMetadata> {
        self.models
            .lock()
            .unwrap()
            .iter()
            .find(|m| m.id == id)
            .cloned()
            .ok_or_else(|| ProviderError::new(404, "unknown_model"))
    }
    fn capabilities(&self, id: &str) -> ProviderResult<ModelCapabilities> {
        Ok(self.metadata(id)?.capabilities)
    }
    fn credential_requirements(&self, id: &str) -> ProviderResult<CredentialRequirement> {
        self.metadata(id)?;
        Ok(CredentialRequirement::None)
    }
    fn create_response(
        &self,
        request: CanonicalRequest,
        context: RequestContext,
    ) -> ProviderFuture<'_, ProviderResponse> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .requests
                .push((request.wire().clone(), context));
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            Ok(ProviderResponse {
                response: CanonicalResponse::new(json!({"id":"fixture","status":"completed","output":[],"future":18446744073709551616_u128})).unwrap(),
                headers: ContextHeaders::default(),
            })
        })
    }
    fn stream_response(
        &self,
        request: CanonicalRequest,
        context: RequestContext,
    ) -> ProviderFuture<'_, StreamingResponse> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .requests
                .push((request.wire().clone(), context));
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            Ok(StreamingResponse {
                events: Box::pin(Events(self.drops.clone())),
                headers: ContextHeaders::default(),
            })
        })
    }
}
fn request(id: &str, dialect: ResponsesDialect) -> CanonicalRequest {
    CanonicalRequest::new(json!({"model":id,"input":[],"future":{"signature":"opaque+/==","number":18446744073709551616_u128}}), dialect).unwrap()
}

#[test]
fn router_registration_reuses_registry_validation_and_requires_explicit_ids() {
    let a = Adapter::new(vec![model("a")], None);
    assert!(ModelRouter::new(vec![]).is_err());
    assert!(ModelRouter::new(vec![("a".into(), a.clone()), ("a".into(), a.clone())]).is_err());
    assert!(ModelRouter::new(vec![("unknown".into(), a.clone())]).is_err());
    a.models.lock().unwrap()[0].capabilities.context_window = Some(0);
    assert!(ModelRouter::new(vec![("a".into(), a.clone())]).is_err());
    assert_eq!(a.calls.lock().unwrap().catalog, 0);
    assert!(a.calls.lock().unwrap().requests.is_empty());
}

#[test]
fn router_keeps_versioned_reports_and_rejects_fixture_full_claims() {
    let mut metadata = model("a");
    metadata.codex_compatibility = Some(CompatibilityReport {
        schema_version: 1,
        level: CompatibilityLevel::Full,
        source: EvidenceSource::ProtocolFixture,
        reference: "tests/router-v1".into(),
        tested_model_version: "fixture-v1".into(),
        limitations: vec!["offline fixture only".into()],
    });
    let a = Adapter::new(vec![metadata], None);
    assert!(ModelRouter::new(vec![("a".into(), a.clone())]).is_err());
    a.models.lock().unwrap()[0]
        .codex_compatibility
        .as_mut()
        .unwrap()
        .level = CompatibilityLevel::Experimental;
    *a.catalog.lock().unwrap() = a.models.lock().unwrap().clone();
    let router = ModelRouter::new(vec![("a".into(), a.clone())]).unwrap();
    assert_eq!(
        ready(router.list_models()).unwrap()[0],
        router.metadata("a").unwrap()
    );
    a.catalog.lock().unwrap()[0]
        .codex_compatibility
        .as_mut()
        .unwrap()
        .tested_model_version = "fixture-v2".into();
    assert_eq!(
        ready(router.list_models()).unwrap_err().code,
        "provider_invalid_metadata"
    );
}

#[test]
fn router_catalog_is_sorted_explicit_and_calls_each_adapter_once() {
    let a = Adapter::new(vec![model("z"), model("a"), model("hidden")], None);
    let b = Adapter::new(vec![model("b"), model("a")], None);
    for m in a.catalog.lock().unwrap().iter_mut() {
        m.source = EvidenceSource::ProviderCatalog;
    }
    let router = ModelRouter::new(vec![
        ("z".into(), a.clone()),
        ("a".into(), a.clone()),
        ("b".into(), b.clone()),
    ])
    .unwrap();
    let models = ready(router.list_models()).unwrap();
    assert_eq!(
        models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        ["a", "b", "z"]
    );
    assert_eq!(models[0].source, EvidenceSource::ProviderCatalog);
    assert_eq!(models[0].capabilities.vision, CapabilitySupport::Unknown);
    assert!(models[0].codex_compatibility.is_none());
    assert_eq!(a.calls.lock().unwrap().catalog, 1);
    assert_eq!(b.calls.lock().unwrap().catalog, 1);
    assert_eq!(router.metadata("a").unwrap(), model("a"));
    assert_eq!(router.capabilities("a").unwrap(), model("a").capabilities);
    assert!(matches!(
        router.credential_requirements("a").unwrap(),
        CredentialRequirement::None
    ));
}

#[test]
fn router_rejects_catalog_binding_drift_duplicates_and_metadata_changes() {
    let a = Adapter::new(vec![model("a")], None);
    let router = ModelRouter::new(vec![("a".into(), a.clone())]).unwrap();
    for changed in [
        {
            let mut m = model("a");
            m.native_model = "other".into();
            m
        },
        {
            let mut m = model("a");
            m.capabilities.vision = CapabilitySupport::Supported;
            m
        },
        {
            let mut m = model("a");
            m.source = EvidenceSource::LiveRuntime;
            m
        },
    ] {
        *a.catalog.lock().unwrap() = vec![changed];
        assert_eq!(
            ready(router.list_models()).unwrap_err().code,
            "provider_invalid_metadata"
        );
    }
    *a.catalog.lock().unwrap() = vec![model("a"), model("a")];
    assert_eq!(
        ready(router.list_models()).unwrap_err().code,
        "provider_invalid_metadata"
    );
    a.models.lock().unwrap()[0].native_model = "other".into();
    assert_eq!(
        router.metadata("a").unwrap_err().code,
        "provider_invalid_metadata"
    );
    assert_eq!(
        ready(router.create_response(
            request("a", ResponsesDialect::Classic),
            RequestContext::default()
        ))
        .err()
        .unwrap()
        .code,
        "provider_invalid_metadata"
    );
    assert!(a.calls.lock().unwrap().requests.is_empty());
}

#[test]
fn router_forwards_wire_context_cancel_and_stream_drop_to_selected_adapter() {
    let a = Adapter::new(vec![model("a")], None);
    let b = Adapter::new(vec![model("b")], None);
    let router = ModelRouter::new(vec![("a".into(), a.clone()), ("b".into(), b.clone())]).unwrap();
    let mut context = RequestContext::default();
    context
        .headers
        .insert("session_id", "opaque-context".into(), REQUEST_HEADERS)
        .unwrap();
    let cancellation = context.cancellation.clone();
    let deadline = Instant::now() + Duration::from_secs(10);
    context.deadline = Some(deadline);
    let wire = request("b", ResponsesDialect::Classic).wire().clone();
    let response =
        ready(router.create_response(request("b", ResponsesDialect::Classic), context)).unwrap();
    assert_eq!(
        response.response.wire()["future"],
        json!(18446744073709551616_u128)
    );
    cancellation.cancel();
    let calls = b.calls.lock().unwrap();
    assert_eq!(calls.requests[0].0, wire);
    assert_eq!(
        calls.requests[0].1.headers.get("session_id"),
        Some("opaque-context")
    );
    assert_eq!(calls.requests[0].1.deadline, Some(deadline));
    assert!(calls.requests[0].1.cancellation.is_cancelled());
    drop(calls);
    let mut stream = ready(router.stream_response(
        request("a", ResponsesDialect::Classic),
        RequestContext::default(),
    ))
    .unwrap();
    assert!(matches!(
        stream
            .events
            .as_mut()
            .poll_next(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Some(Ok(ProviderStreamEvent::Heartbeat)))
    ));
    drop(stream);
    assert_eq!(a.drops.load(Ordering::SeqCst), 1);
    assert_eq!(a.calls.lock().unwrap().requests.len(), 1);
    assert_eq!(b.calls.lock().unwrap().requests.len(), 1);
}

#[test]
fn router_errors_keep_retry_metadata_without_fallback_or_cached_catalog() {
    let failure = ProviderError {
        http_status: 429,
        code: "provider_rate_limited",
        retry_after_seconds: Some(7),
    };
    let a = Adapter::new(vec![model("a")], Some(failure.clone()));
    let b = Adapter::new(vec![model("b")], None);
    let router = ModelRouter::new(vec![("a".into(), a.clone()), ("b".into(), b.clone())]).unwrap();
    assert_eq!(
        ready(router.create_response(
            request("a", ResponsesDialect::Classic),
            RequestContext::default()
        ))
        .err()
        .unwrap(),
        failure
    );
    assert_eq!(
        ready(router.stream_response(
            request("a", ResponsesDialect::Classic),
            RequestContext::default()
        ))
        .err()
        .unwrap(),
        failure
    );
    assert_eq!(ready(router.list_models()).unwrap_err(), failure);
    assert_eq!(a.calls.lock().unwrap().requests.len(), 2);
    assert!(b.calls.lock().unwrap().requests.is_empty());
    assert_eq!(b.calls.lock().unwrap().catalog, 0);
}

#[test]
fn router_unknown_dialect_and_unsupported_streaming_fail_before_adapter_calls() {
    let mut metadata = model("a");
    metadata.capabilities.streaming = CapabilitySupport::Unsupported;
    let a = Adapter::new(vec![metadata], None);
    let router = ModelRouter::new(vec![("a".into(), a.clone())]).unwrap();
    for (id, dialect, stream, code) in [
        ("unknown", ResponsesDialect::Classic, false, "unknown_model"),
        ("a", ResponsesDialect::Lite, false, "unsupported_dialect"),
        (
            "a",
            ResponsesDialect::Classic,
            true,
            "unsupported_streaming",
        ),
    ] {
        let result = if stream {
            ready(router.stream_response(request(id, dialect), RequestContext::default())).err()
        } else {
            ready(router.create_response(request(id, dialect), RequestContext::default())).err()
        };
        assert_eq!(result.unwrap().code, code);
    }
    assert!(a.calls.lock().unwrap().requests.is_empty());
}
