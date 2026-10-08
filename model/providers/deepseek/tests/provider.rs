//! Offline native HTTP/SSE; synthetic executor key and isolated loopback only.
use caidex_credentials::{Broker, CredentialRef, Id, Secret, SecretKind, SecretStore};
use caidex_model_core::{
    CancellationToken, CanonicalRequest, CapabilitySupport, ContextHeaders, CredentialRequirement,
    EvidenceSource, ModelMetadata, ModelProvider, ProviderStreamEvent, REQUEST_HEADERS,
    RequestContext, ResponsesDialect,
};
use caidex_provider_deepseek::{DeepSeekConfig, DeepSeekProvider, Limits};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};

const KEY: &str = "CAIDEX_SYNTHETIC_DEEPSEEK_KEY";
const WAIT: Duration = Duration::from_secs(10);
fn reference() -> CredentialRef {
    CredentialRef {
        owner: Id::new("executor").unwrap(),
        provider: Id::new("deepseek").unwrap(),
        profile: Id::new("fixture").unwrap(),
        kind: SecretKind::ApiKey,
    }
}
struct Store {
    reads: Arc<AtomicUsize>,
    key: Option<&'static str>,
}
impl SecretStore for Store {
    fn get(&self, r: &CredentialRef) -> caidex_credentials::Result<Option<Secret>> {
        assert_eq!(r.provider.as_str(), "deepseek");
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.key.map(|key| Secret::new(key.into())).transpose()
    }
    fn set(&self, _: &CredentialRef, _: &Secret) -> caidex_credentials::Result<()> {
        unreachable!()
    }
    fn remove(&self, _: &CredentialRef) -> caidex_credentials::Result<bool> {
        unreachable!()
    }
}
fn fixture_broker(key: Option<&'static str>) -> (Arc<Broker<Store>>, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    (
        Arc::new(Broker::new(
            Id::new("executor").unwrap(),
            Store {
                reads: reads.clone(),
                key,
            },
        )),
        reads,
    )
}
fn metadata(id: &str, native: &str) -> ModelMetadata {
    ModelMetadata::configured(id.into(), native.into(), vec![ResponsesDialect::Classic])
}
fn limits() -> Limits {
    Limits {
        in_flight: 1,
        header_timeout: Duration::from_secs(2),
        idle_timeout: Duration::from_secs(2),
        total_timeout: Duration::from_secs(4),
        ..Limits::default()
    }
}
fn request(wire: Value) -> CanonicalRequest {
    CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap()
}
fn basic(stream: bool) -> Value {
    json!({"model":"fixture", "input":"中文🙂", "stream":stream})
}
fn native() -> Value {
    json!({"id":"fixture","object":"response","created_at":1,"status":"completed","model":"native-fixture","store":false,"output":[
        {"type":"reasoning","id":"rs_one","status":"completed","content":[{"type":"reasoning_text","text":"Native thinking"}],"summary":[]},
        {"type":"message","id":"msg_one","status":"completed","role":"assistant","content":[{"type":"output_text","text":"中文🙂","annotations":[]}]},
        {"type":"future_item","n":18446744073709551616_u128}],
        "usage":{"input_tokens":2,"output_tokens":3,"total_tokens":5,"output_tokens_details":{"reasoning_tokens":1}},"future":{"n":18446744073709551616_u128}})
}
fn catalog() -> Value {
    json!({"object":"list","data":[{"object":"model","id":"native-fixture","owned_by":"native-owner","name":"Fixture","context_window":1048576,"max_output_tokens":393216,"effort":{"supported_levels":["low","high","max"]},"future":{"n":18446744073709551616_u128}}, {"object":"model","id":"other","owned_by":"other-owner"}]})
}
fn event(wire: Value) -> String {
    format!(
        "event: {}\ndata: {}\n\n",
        wire["type"].as_str().unwrap(),
        wire
    )
}
fn created() -> String {
    event(
        json!({"type":"response.created","response":{"id":"fixture","status":"in_progress","output":[]}}),
    )
}
fn terminal(wire: Value) -> String {
    event(json!({"type":format!("response.{}",wire["status"].as_str().unwrap()),"response":wire}))
}
#[derive(Clone)]
struct Reply {
    status: u16,
    media: &'static str,
    body: String,
    headers: String,
    stall: u8,
}
impl Reply {
    fn json(wire: Value) -> Self {
        Self {
            status: 200,
            media: "application/json",
            body: wire.to_string(),
            headers: "X-Request-Id: fixture-id\r\nSet-Cookie: forbidden\r\n".into(),
            stall: 0,
        }
    }
    fn stream(body: String) -> Self {
        Self {
            media: "text/event-stream",
            body,
            ..Self::json(Value::Null)
        }
    }
}
struct Captured {
    headers: String,
    body: Option<Value>,
}
impl Captured {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name).then(|| value.trim())
        })
    }
}
struct Fixture {
    base: String,
    requests: mpsc::UnboundedReceiver<Captured>,
    closed: mpsc::UnboundedReceiver<()>,
    accepted: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}
impl Fixture {
    async fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/proxy/v1", listener.local_addr().unwrap());
        let (tx, requests) = mpsc::unbounded_channel();
        let (closed_tx, closed) = mpsc::unbounded_channel();
        let accepted = Arc::new(AtomicUsize::new(0));
        let accepts = accepted.clone();
        let task = tokio::spawn(async move {
            let mut replies = VecDeque::from(replies);
            let mut sessions = JoinSet::new();
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                accepts.fetch_add(1, Ordering::SeqCst);
                let reply = if replies.len() > 1 {
                    replies.pop_front().unwrap()
                } else {
                    replies.front().unwrap().clone()
                };
                let (tx, closed_tx) = (tx.clone(), closed_tx.clone());
                sessions.spawn(async move {
                    let mut headers = Vec::new();
                    while !headers.ends_with(b"\r\n\r\n") {
                        match socket.read_u8().await {
                            Ok(byte) => headers.push(byte),
                            Err(_) => return,
                        };
                        assert!(headers.len() < 32768);
                    }
                    let headers = String::from_utf8(headers).unwrap();
                    let size = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    let mut bytes = vec![0; size];
                    socket.read_exact(&mut bytes).await.unwrap();
                    let _ = tx.send(Captured {
                        headers,
                        body: (!bytes.is_empty()).then(|| serde_json::from_slice(&bytes).unwrap()),
                    });
                    if reply.stall != 1 {
                        let length = if reply.stall == 2 {
                            "Transfer-Encoding: chunked\r\n".into()
                        } else {
                            format!("Content-Length: {}\r\n", reply.body.len())
                        };
                        let header = format!(
                            "HTTP/1.1 {} Fixture\r\nContent-Type: {}\r\n{length}{}\r\n",
                            reply.status, reply.media, reply.headers
                        );
                        if socket.write_all(header.as_bytes()).await.is_err() {
                            return;
                        }
                        if reply.stall == 2 {
                            let chunk = format!("{:x}\r\n{}\r\n", reply.body.len(), reply.body);
                            let _ = socket.write_all(chunk.as_bytes()).await;
                        } else {
                            let _ = socket.write_all(reply.body.as_bytes()).await;
                        }
                    }
                    if reply.stall != 0 {
                        let _ = socket.read(&mut [0]).await;
                    }
                    let _ = closed_tx.send(());
                });
                while sessions.try_join_next().is_some() {}
            }
        });
        Self {
            base,
            requests,
            closed,
            accepted,
            task,
        }
    }
    fn provider(
        &self,
        broker: Arc<Broker<Store>>,
        models: Vec<ModelMetadata>,
        limits: Limits,
    ) -> DeepSeekProvider<Store> {
        DeepSeekProvider::new(
            DeepSeekConfig::new(reference())
                .unwrap()
                .with_base_url(&self.base)
                .unwrap(),
            models,
            broker,
            limits,
        )
        .unwrap()
    }
    async fn captured(&mut self) -> Captured {
        tokio::time::timeout(WAIT, self.requests.recv())
            .await
            .unwrap()
            .unwrap()
    }
    async fn disconnected(&mut self) {
        tokio::time::timeout(WAIT, self.closed.recv())
            .await
            .unwrap()
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn native_catalog_without_created_preserves_raw_declarations_and_configured_intersection() {
    let mut fixture = Fixture::start(vec![Reply::json(catalog())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture.provider(
        broker,
        vec![
            metadata("fixture", "native-fixture"),
            metadata("missing", "absent"),
        ],
        limits(),
    );
    assert_eq!(
        provider.metadata("fixture").unwrap().source,
        EvidenceSource::Configured
    );
    assert_eq!(
        provider.capabilities("fixture").unwrap().vision,
        CapabilitySupport::Unknown
    );
    assert!(
        matches!(provider.credential_requirements("fixture").unwrap(),CredentialRequirement::Bearer { reference:r } if r==reference())
    );
    assert!(provider.metadata("unknown").is_err());
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let models = provider
        .discover_models(RequestContext::default())
        .await
        .unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id(), "native-fixture");
    assert_eq!(models[0].owned_by(), "native-owner");
    assert_eq!(*models[0].wire(), catalog()["data"][0]);
    assert!(models[0].wire().get("created").is_none());
    assert!(!format!("{:?}", models[0]).contains("native-owner"));
    let captured = fixture.captured().await;
    assert!(captured.headers.starts_with("GET /proxy/v1/models "));
    assert!(captured.body.is_none());
    assert_eq!(
        captured.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    let listed = provider.list_models().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "fixture");
    assert_eq!(listed[0].source, EvidenceSource::ProviderCatalog);
    assert!(listed[0].codex_compatibility.is_none());
    assert_eq!(
        listed[0].capabilities,
        provider.capabilities("fixture").unwrap()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn malformed_catalogs_fail_without_cached_or_fabricated_models() {
    let good = catalog()["data"][0].clone();
    let mut bad = vec![
        json!({"object":"wrong","data":[]}),
        json!({"object":"list","data":null}),
        json!({"object":"list","data":[good.clone(),good.clone()]}),
        json!({"object":"list","data":[null]}),
    ];
    for (key, value) in [
        ("id", json!("")),
        ("id", json!("bad\n")),
        ("id", Value::Null),
        ("owned_by", json!(" ")),
        ("owned_by", json!(1)),
        ("object", json!("other")),
    ] {
        let mut model = good.clone();
        model[key] = value;
        bad.push(json!({"object":"list","data":[model]}));
    }
    for wire in bad {
        let fixture = Fixture::start(vec![Reply::json(wire)]).await;
        let (broker, reads) = fixture_broker(Some(KEY));
        let provider = fixture.provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        );
        let error = provider.list_models().await.err().unwrap();
        assert_eq!(
            (error.http_status, error.code),
            (502, "deepseek_invalid_model_catalog")
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
    let fixture = Fixture::start(vec![Reply::json(json!({"object":"list","data":[]}))]).await;
    let (broker, _) = fixture_broker(Some(KEY));
    assert!(
        fixture
            .provider(
                broker,
                vec![metadata("fixture", "native-fixture")],
                limits()
            )
            .list_models()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn classic_text_json_and_sse_preserve_native_output_unknown_fields_and_message_history() {
    for streaming in [false, true] {
        let reply = if streaming {
            Reply::stream(
                created()
                    + &event(
                        json!({"type":"response.reasoning_text.delta","delta":"Native thinking"}),
                    )
                    + &event(json!({"type":"response.output_text.delta","delta":"中文🙂"}))
                    + &terminal(native()),
            )
        } else {
            Reply::json(native())
        };
        let mut fixture = Fixture::start(vec![reply]).await;
        let (broker, reads) = fixture_broker(Some(KEY));
        let provider = fixture.provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        );
        let history = json!([{"role":"system","content":"Instructions"},{"type":"message","role":"user","content":[{"type":"input_text","text":"中文🙂"}]},{"type":"message","id":"previous","status":"completed","role":"assistant","content":[{"type":"output_text","text":"Earlier","annotations":[]}]}]);
        let wire = json!({"model":"fixture","input":history,"instructions":"Explicit instruction","stream":streaming,"max_output_tokens":64,"text":{"format":{"type":"text"}}});
        let mut nullable = wire.clone();
        for key in [
            "instructions",
            "max_output_tokens",
            "text",
            "store",
            "background",
        ] {
            nullable[key] = Value::Null;
        }
        for wire in [wire, nullable] {
            if streaming {
                let mut response = provider
                    .stream_response(request(wire.clone()), RequestContext::default())
                    .await
                    .unwrap();
                let mut final_wire = None;
                let mut text = String::new();
                while let Some(event) = response.events.next().await {
                    if let ProviderStreamEvent::Model(event) = event.unwrap() {
                        if event.response.kind() == "response.output_text.delta" {
                            text.push_str(event.response.wire()["delta"].as_str().unwrap());
                        }
                        if event.response.terminal().is_some() {
                            final_wire = Some(event.response.wire()["response"].clone());
                        }
                    }
                }
                assert_eq!(text, "中文🙂");
                assert_eq!(final_wire, Some(native()));
            } else {
                let response = provider
                    .create_response(request(wire.clone()), RequestContext::default())
                    .await
                    .unwrap();
                assert_eq!(*response.response.wire(), native());
                assert_eq!(response.headers.get("x-request-id"), Some("fixture-id"));
                assert!(response.headers.get("set-cookie").is_none());
            }
            let captured = fixture.captured().await;
            assert!(captured.headers.starts_with("POST /proxy/v1/responses "));
            let mut expected = wire.clone();
            expected["model"] = "native-fixture".into();
            expected["store"] = false.into();
            assert_eq!(captured.body, Some(expected));
            assert_eq!(
                captured.header("authorization"),
                Some(format!("Bearer {KEY}").as_str())
            );
            assert!(
                captured
                    .header("x-openai-internal-codex-responses-lite")
                    .is_none()
            );
            assert!(captured.header("openai-organization").is_none());
        }
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn ignored_or_unimplemented_controls_and_bad_text_refuse_before_credentials_or_post() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture.provider(
        broker,
        vec![metadata("fixture", "native-fixture")],
        limits(),
    );
    let mut invalid = Vec::new();
    for (key, value) in [
        ("store", json!(true)),
        ("background", json!(true)),
        ("tools", json!([])),
        ("parallel_tool_calls", json!(false)),
        ("max_tool_calls", json!(1)),
        ("previous_response_id", json!("old")),
        ("include", json!([])),
        ("metadata", json!({})),
        ("client_metadata", json!({})),
        ("prompt_cache_key", json!("cache")),
        ("reasoning", json!({"effort":"high"})),
        ("text", json!({"verbosity":"low"})),
        ("text", json!({"format":{"type":"json_object"}})),
        ("temperature", json!(0.2)),
        ("top_p", json!(0.9)),
        ("instructions", json!([])),
        ("max_output_tokens", json!(0)),
        ("max_output_tokens", json!(1.5)),
        ("future", json!(true)),
    ] {
        let mut wire = basic(false);
        wire[key] = value;
        invalid.push(wire);
    }
    for item in [
        json!({"role":"developer","content":"Policy"}),
        json!({"type":"additional_tools","role":"developer","tools":[]}),
        json!({"type":"reasoning","encrypted_content":"opaque"}),
        json!({"type":"future","n":1}),
        json!({"type":"message","role":"user","content":[{"type":"input_image","image_url":"data:image/png;base64,YQ=="}]}),
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":1}]}),
        json!({"role":"user","content":null}),
        json!({"role":"user","id":"bad\n","content":"hi"}),
        json!({"role":"assistant","status":"in_progress","content":"hi"}),
    ] {
        let mut wire = basic(false);
        wire["input"] = json!([item]);
        invalid.push(wire);
    }
    for mut wire in invalid {
        for streaming in [false, true] {
            wire["stream"] = streaming.into();
            let error = if streaming {
                provider
                    .stream_response(request(wire.clone()), RequestContext::default())
                    .await
                    .err()
                    .unwrap()
            } else {
                provider
                    .create_response(request(wire.clone()), RequestContext::default())
                    .await
                    .err()
                    .unwrap()
            };
            assert_eq!(error.http_status, 400, "{wire}");
        }
    }
    let lite = CanonicalRequest::new(
        json!({"model":"fixture","input":[]}),
        ResponsesDialect::Lite,
    )
    .unwrap();
    assert_eq!(
        provider
            .create_response(lite, RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "unsupported_dialect"
    );
    let mut headers = ContextHeaders::default();
    headers
        .insert("session_id", "local".into(), REQUEST_HEADERS)
        .unwrap();
    assert_eq!(
        provider
            .create_response(
                request(basic(false)),
                RequestContext {
                    headers,
                    ..Default::default()
                }
            )
            .await
            .err()
            .unwrap()
            .code,
        "deepseek_unsupported_context"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[test]
fn executor_configuration_rejects_other_providers_unsafe_urls_lite_and_invalid_routes() {
    let (broker, _) = fixture_broker(Some(KEY));
    for base in [
        "http://example.com/v1",
        "http://localhost/v1",
        "https://user:pass@example.com/v1",
        "https://example.com/v1?key=secret",
        "https://example.com/v1#x",
        "file:///tmp/model",
    ] {
        assert!(
            DeepSeekConfig::new(reference())
                .unwrap()
                .with_base_url(base)
                .is_err()
        );
    }
    let mut other = reference();
    other.provider = Id::new("openai").unwrap();
    assert!(DeepSeekConfig::new(other).is_err());
    let mut unsupported = reference();
    unsupported.kind = SecretKind::ClientSecret;
    assert!(DeepSeekConfig::new(unsupported).is_err());
    let config = DeepSeekConfig::new(reference()).unwrap();
    assert!(!format!("{config:?}").contains("api.deepseek.com"));
    let mut model = metadata("fixture", "native-fixture");
    model.dialects = vec![ResponsesDialect::Lite];
    assert!(DeepSeekProvider::new(config, vec![model], broker.clone(), limits()).is_err());
    let config = DeepSeekConfig::new(reference()).unwrap();
    assert!(
        DeepSeekProvider::new(
            config,
            vec![metadata("same", "one"), metadata("same", "two")],
            broker,
            limits()
        )
        .is_err()
    );
}

#[tokio::test]
async fn budgets_capabilities_missing_keys_wrong_ownership_and_precancel_never_post() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let small = Limits {
        request_bytes: 16,
        ..limits()
    };
    let provider = fixture.provider(broker, vec![metadata("fixture", "native-fixture")], small);
    assert_eq!(
        provider
            .create_response(request(basic(false)), RequestContext::default())
            .await
            .err()
            .unwrap()
            .http_status,
        413
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    for streaming in [false, true] {
        let (broker, reads) = fixture_broker(Some(KEY));
        let mut model = metadata("fixture", "native-fixture");
        if streaming {
            model.capabilities.streaming = CapabilitySupport::Unsupported;
        } else {
            model.capabilities.text = CapabilitySupport::Unsupported;
        }
        let provider = fixture.provider(broker, vec![model], limits());
        let error = if streaming {
            provider
                .stream_response(request(basic(true)), RequestContext::default())
                .await
                .err()
                .unwrap()
        } else {
            provider
                .create_response(request(basic(false)), RequestContext::default())
                .await
                .err()
                .unwrap()
        };
        assert_eq!(error.http_status, 400);
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }
    let (broker, reads) = fixture_broker(None);
    let provider = fixture.provider(
        broker,
        vec![metadata("fixture", "native-fixture")],
        limits(),
    );
    assert_eq!(
        provider
            .create_response(request(basic(false)), RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "credential_missing"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    let (broker, reads) = fixture_broker(Some(KEY));
    let mut wrong = reference();
    wrong.owner = Id::new("other-executor").unwrap();
    let provider = DeepSeekProvider::new(
        DeepSeekConfig::new(wrong)
            .unwrap()
            .with_base_url(&fixture.base)
            .unwrap(),
        vec![metadata("fixture", "native-fixture")],
        broker,
        limits(),
    )
    .unwrap();
    assert_eq!(
        provider
            .create_response(request(basic(false)), RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "credential_unavailable"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture.provider(
        broker,
        vec![metadata("fixture", "native-fixture")],
        limits(),
    );
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert_eq!(
        provider
            .create_response(
                request(basic(false)),
                RequestContext {
                    cancellation,
                    ..Default::default()
                }
            )
            .await
            .err()
            .unwrap()
            .code,
        "provider_cancelled"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn native_terminal_states_and_unrequested_tool_json_are_checked_before_delivery() {
    for status in ["completed", "incomplete", "failed"] {
        let mut wire = native();
        wire["status"] = status.into();
        if status == "incomplete" {
            wire["incomplete_details"] = json!({"reason":"max_output_tokens"});
        }
        if status == "failed" {
            wire["error"] = json!({"code":"fixture","message":format!("diagnostic {KEY}")});
        }
        for streaming in [false, true] {
            let reply = if streaming {
                Reply::stream(created() + &terminal(wire.clone()))
            } else {
                Reply::json(wire.clone())
            };
            let fixture = Fixture::start(vec![reply]).await;
            let (broker, _) = fixture_broker(Some(KEY));
            let provider = fixture.provider(
                broker.clone(),
                vec![metadata("fixture", "native-fixture")],
                limits(),
            );
            let expected = || {
                if status == "failed" {
                    let mut v = wire.clone();
                    v["error"] = broker.redactor().json(&v["error"]);
                    v
                } else {
                    wire.clone()
                }
            };
            if streaming {
                let mut response = provider
                    .stream_response(request(basic(true)), RequestContext::default())
                    .await
                    .unwrap();
                let mut found = None;
                while let Some(event) = response.events.next().await {
                    if let ProviderStreamEvent::Model(event) = event.unwrap()
                        && event.response.terminal().is_some()
                    {
                        found = Some(event.response.wire()["response"].clone());
                    }
                }
                assert_eq!(found, Some(expected()));
            } else {
                assert_eq!(
                    *provider
                        .create_response(request(basic(false)), RequestContext::default())
                        .await
                        .unwrap()
                        .response
                        .wire(),
                    expected()
                );
            }
        }
        let mut bad = wire;
        bad["output"] = json!([{"type":"function_call","name":"exec","call_id":"native-call","arguments":"{}"}]);
        let fixture = Fixture::start(vec![Reply::json(bad), Reply::json(native())]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        let provider = fixture.provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        );
        assert_eq!(
            provider
                .create_response(request(basic(false)), RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            "deepseek_unexpected_tool"
        );
        assert_eq!(
            *provider
                .create_response(request(basic(false)), RequestContext::default())
                .await
                .unwrap()
                .response
                .wire(),
            native()
        );
    }
}

#[tokio::test]
async fn undeclared_tool_sse_is_rejected_closes_socket_and_releases_slot() {
    let call =
        json!({"type":"function_call","name":"exec","call_id":"native-call","arguments":"{}"});
    for bad in [
        json!({"type":"response.output_item.added","output_index":0,"item":call.clone()}),
        json!({"type":"response.output_item.done","output_index":0,"item":call}),
        json!({"type":"response.function_call_arguments.delta","delta":"{}"}),
        json!({"type":"response.custom_tool_call_input.done","input":"text"}),
        json!({"type":"response.completed","response":{"id":"fixture","status":"completed","output":[{"type":"custom_tool_call","name":"exec","call_id":"c","input":"text"}]}}),
    ] {
        let mut reply = Reply::stream(created() + &event(bad));
        reply.stall = 2;
        let mut fixture = Fixture::start(vec![reply, Reply::json(native())]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        let provider = fixture.provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        );
        let mut response = provider
            .stream_response(request(basic(true)), RequestContext::default())
            .await
            .unwrap();
        loop {
            match response.events.next().await.unwrap() {
                Ok(ProviderStreamEvent::Model(event)) => {
                    assert_eq!(event.response.kind(), "response.created")
                }
                Ok(ProviderStreamEvent::Heartbeat) => (),
                Err(error) => {
                    assert_eq!(error.code, "deepseek_unexpected_tool");
                    break;
                }
            }
        }
        assert!(response.events.next().await.is_none());
        fixture.disconnected().await;
        assert_eq!(
            *provider
                .create_response(request(basic(false)), RequestContext::default())
                .await
                .unwrap()
                .response
                .wire(),
            native()
        );
    }
}

#[tokio::test]
async fn native_http_errors_metadata_limits_stream_drop_and_cancellation_use_shared_transport() {
    for (status, media, code) in [
        (429, "application/json", "provider_rate_limited"),
        (307, "application/json", "provider_redirect_blocked"),
        (200, "text/html", "provider_invalid_content_type"),
    ] {
        let mut reply = Reply::json(json!({"error":{"message":KEY}}));
        reply.status = status;
        reply.media = media;
        reply.headers = "Retry-After: 7\r\nLocation: http://127.0.0.1:1/forbidden\r\n".into();
        let fixture = Fixture::start(vec![reply]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        let provider = fixture.provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        );
        let error = provider
            .discover_models(RequestContext::default())
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, code);
        assert!(!error.to_string().contains(KEY));
        if status == 429 {
            assert_eq!(error.retry_after_seconds, Some(7));
        }
        assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
    }
    let fixture = Fixture::start(vec![Reply::json(catalog())]).await;
    let (broker, _) = fixture_broker(Some(KEY));
    let provider = fixture.provider(
        broker,
        vec![metadata("fixture", "native-fixture")],
        Limits {
            response_bytes: 16,
            ..limits()
        },
    );
    assert_eq!(
        provider
            .discover_models(RequestContext::default())
            .await
            .err()
            .unwrap()
            .http_status,
        502
    );
    for cancel in [false, true] {
        let mut reply = Reply::stream(created());
        reply.stall = 2;
        let mut fixture = Fixture::start(vec![reply, Reply::json(native())]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        let provider = fixture.provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        );
        let cancellation = CancellationToken::new();
        let mut response = provider
            .stream_response(
                request(basic(true)),
                RequestContext {
                    cancellation: cancellation.clone(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(response.events.next().await.unwrap().is_ok());
        if cancel {
            cancellation.cancel();
            assert_eq!(
                response.events.next().await.unwrap().err().unwrap().code,
                "provider_cancelled"
            );
        }
        drop(response);
        fixture.disconnected().await;
        assert_eq!(
            *provider
                .create_response(request(basic(false)), RequestContext::default())
                .await
                .unwrap()
                .response
                .wire(),
            native()
        );
    }
}

#[tokio::test]
async fn injected_gateway_keeps_local_token_separate_and_refuses_unmapped_context() {
    let mut fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = Arc::new(fixture.provider(
        broker.clone(),
        vec![metadata("fixture", "native-fixture")],
        limits(),
    ));
    let gateway = caidex_model_gateway::start_with_provider(provider, broker.redactor(), limits())
        .await
        .unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let url = format!("http://{}/v1/responses", gateway.address());
    assert_eq!(
        client
            .post(&url)
            .bearer_auth("wrong")
            .header("content-type", "application/json")
            .body(basic(false).to_string())
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let denied = client
        .post(&url)
        .bearer_auth(gateway.token().expose())
        .header("x-client-request-id", "local")
        .header("content-type", "application/json")
        .body(basic(false).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 400);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let response = client
        .post(&url)
        .bearer_auth(gateway.token().expose())
        .header("x-api-key", "attacker")
        .header("openai-organization", "attacker")
        .header("content-type", "application/json")
        .body(basic(false).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&response.bytes().await.unwrap()).unwrap(),
        native()
    );
    let captured = fixture.captured().await;
    assert_eq!(
        captured.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert!(!captured.headers.contains(gateway.token().expose()));
    assert!(captured.header("x-api-key").is_none());
    assert!(captured.header("openai-organization").is_none());
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    gateway.shutdown().await.unwrap();
}

fn local_context() -> RequestContext {
    let mut headers = ContextHeaders::default();
    for name in ["session_id", "x-client-request-id", "x-codex-turn-metadata"] {
        headers
            .insert(name, format!("LOCAL_{name}"), REQUEST_HEADERS)
            .unwrap();
    }
    RequestContext {
        headers,
        ..Default::default()
    }
}

fn runtime_wire(streaming: bool) -> Value {
    json!({"model":"fixture","input":[
        {"type":"message","id":"dev1","role":"developer","content":[{"type":"input_text","text":"Priority 中文🙂"}]},
        {"role":"system","content":"Existing system"},
        {"role":"developer","content":"Second priority"},
        {"role":"user","content":"Conversation"}],
        "instructions":"Original instruction","stream":streaming,
        "client_metadata":{"session_id":"LOCAL_BODY_SESSION","future":"LOCAL_EXTENSION"},
        "prompt_cache_key":"LOCAL_CACHE","text":{"verbosity":"low","format":{"type":"text"}}})
}

#[tokio::test]
async fn runtime_context_compiles_priority_and_verbosity_without_forwarding_attribution() {
    for streaming in [false, true] {
        let reply = if streaming {
            Reply::stream(created() + &terminal(native()))
        } else {
            Reply::json(native())
        };
        let mut fixture = Fixture::start(vec![reply]).await;
        let (broker, reads) = fixture_broker(Some(KEY));
        let provider = fixture
            .provider(
                broker,
                vec![metadata("fixture", "native-fixture")],
                limits(),
            )
            .with_runtime_context()
            .with_verbosity_instruction("low".into(), "Executor concise guidance".into())
            .unwrap();
        let source = runtime_wire(streaming);
        let mut nullable = source.clone();
        for key in ["instructions", "client_metadata", "prompt_cache_key"] {
            nullable[key] = Value::Null;
        }
        nullable["text"]["verbosity"] = Value::Null;
        for source in [source, nullable] {
            if streaming {
                let mut response = provider
                    .stream_response(request(source.clone()), local_context())
                    .await
                    .unwrap();
                let mut found = None;
                while let Some(event) = response.events.next().await {
                    if let ProviderStreamEvent::Model(event) = event.unwrap()
                        && event.response.terminal().is_some()
                    {
                        found = Some(event.response.wire()["response"].clone());
                    }
                }
                assert_eq!(found, Some(native()));
            } else {
                assert_eq!(
                    *provider
                        .create_response(request(source.clone()), local_context())
                        .await
                        .unwrap()
                        .response
                        .wire(),
                    native()
                );
            }
            let captured = fixture.captured().await;
            for name in ["session_id", "x-client-request-id", "x-codex-turn-metadata"] {
                assert!(captured.header(name).is_none());
            }
            assert!(!captured.headers.contains("LOCAL_"));
            assert_eq!(
                captured.header("authorization"),
                Some(format!("Bearer {KEY}").as_str())
            );
            let mut expected = source.clone();
            expected["model"] = "native-fixture".into();
            expected["store"] = false.into();
            for index in [0, 2] {
                expected["input"][index]["role"] = "system".into();
            }
            expected.as_object_mut().unwrap().remove("client_metadata");
            expected.as_object_mut().unwrap().remove("prompt_cache_key");
            expected["text"]
                .as_object_mut()
                .unwrap()
                .remove("verbosity");
            if !source["text"]["verbosity"].is_null() {
                expected["instructions"] = "Original instruction\nExecutor concise guidance".into();
            }
            assert_eq!(captured.body, Some(expected));
        }
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn runtime_context_refuses_bad_local_controls_late_developer_and_unimplemented_history() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_runtime_context()
        .with_verbosity_instruction("low".into(), "Executor guidance".into())
        .unwrap();
    let mut cases = Vec::new();
    for (key, value) in [
        ("client_metadata", json!({"bad":1})),
        ("client_metadata", json!([])),
        ("prompt_cache_key", json!(" ")),
        ("prompt_cache_key", json!("bad\n")),
        ("prompt_cache_key", json!(1)),
        ("text", json!({"verbosity":"high"})),
        ("text", json!({"verbosity":1})),
        ("text", json!({"verbosity":"low","future":true})),
        (
            "text",
            json!({"verbosity":"low","format":{"type":"json_object"}}),
        ),
        ("instructions", json!([])),
        ("reasoning", json!({"summary":"auto"})),
        ("reasoning", json!({"context":"all_turns"})),
        ("include", json!(["reasoning.encrypted_content"])),
        ("parallel_tool_calls", json!(false)),
        ("tools", json!([])),
    ] {
        let mut wire = runtime_wire(false);
        wire[key] = value;
        cases.push(wire);
    }
    for input in [
        json!([{"role":"user","content":"Hi"},{"role":"developer","content":"Late priority"}]),
        json!([{"role":"assistant","content":"Earlier"},{"role":"developer","content":"Late priority"}]),
        json!([{"type":"reasoning","content":[]},{"role":"developer","content":"After native item"}]),
        json!([{"type":"future","role":"developer","content":"Unknown item"}]),
    ] {
        let mut wire = runtime_wire(false);
        wire["input"] = input;
        cases.push(wire);
    }
    for mut wire in cases {
        for streaming in [false, true] {
            wire["stream"] = streaming.into();
            let error = if streaming {
                provider
                    .stream_response(request(wire.clone()), local_context())
                    .await
                    .err()
                    .unwrap()
            } else {
                provider
                    .create_response(request(wire.clone()), local_context())
                    .await
                    .err()
                    .unwrap()
            };
            assert_eq!(error.http_status, 400, "{wire}");
        }
    }
    let mut state = local_context();
    state
        .headers
        .insert("x-codex-turn-state", "UNBOUND".into(), REQUEST_HEADERS)
        .unwrap();
    assert_eq!(
        provider
            .create_response(request(basic(false)), state)
            .await
            .err()
            .unwrap()
            .code,
        "deepseek_unsupported_context"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[test]
fn verbosity_mapping_is_executor_owned_validated_and_cannot_be_silently_replaced() {
    let (broker, reads) = fixture_broker(Some(KEY));
    let make = || {
        DeepSeekProvider::new(
            DeepSeekConfig::new(reference()).unwrap(),
            vec![metadata("fixture", "native-fixture")],
            broker.clone(),
            limits(),
        )
        .unwrap()
    };
    for (level, instruction) in [("unknown", "guide"), ("low", " ")] {
        assert_eq!(
            make()
                .with_verbosity_instruction(level.into(), instruction.into())
                .err()
                .unwrap()
                .code,
            "deepseek_invalid_verbosity_mapping"
        );
    }
    assert!(
        make()
            .with_verbosity_instruction("low".into(), "first".into())
            .unwrap()
            .with_verbosity_instruction("low".into(), "second".into())
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn runtime_context_original_and_expanded_budgets_refuse_before_key_or_post() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            Limits {
                request_bytes: 256,
                ..limits()
            },
        )
        .with_runtime_context()
        .with_verbosity_instruction("low".into(), "Guidance".repeat(100))
        .unwrap();
    for mut wire in [
        json!({"model":"fixture","input":"Hello","client_metadata":{"large":"x".repeat(512)}}),
        json!({"model":"fixture","input":"Hello","text":{"verbosity":"low"}}),
    ] {
        for streaming in [false, true] {
            wire["stream"] = streaming.into();
            let error = if streaming {
                provider
                    .stream_response(request(wire.clone()), local_context())
                    .await
                    .err()
                    .unwrap()
            } else {
                provider
                    .create_response(request(wire.clone()), local_context())
                    .await
                    .err()
                    .unwrap()
            };
            assert_eq!(
                (error.http_status, error.code),
                (413, "invalid_or_oversized_body")
            );
        }
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn local_catalog_context_preserves_cancellation_and_deadline_and_refuses_native_turn_state() {
    let mut fixture = Fixture::start(vec![Reply::json(catalog())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_runtime_context();
    assert_eq!(
        provider
            .discover_models(local_context())
            .await
            .unwrap()
            .len(),
        2
    );
    let captured = fixture.captured().await;
    for name in ["session_id", "x-client-request-id", "x-codex-turn-metadata"] {
        assert!(captured.header(name).is_none());
    }
    for cancel in [false, true] {
        let mut context = local_context();
        let code = if cancel {
            context.cancellation.cancel();
            "provider_cancelled"
        } else {
            context.deadline = Some(std::time::Instant::now() - Duration::from_secs(1));
            "provider_timeout"
        };
        assert_eq!(
            provider.discover_models(context).await.err().unwrap().code,
            code
        );
        let mut context = local_context();
        if cancel {
            context.cancellation.cancel();
        } else {
            context.deadline = Some(std::time::Instant::now() - Duration::from_secs(1));
        }
        assert_eq!(
            provider
                .create_response(request(basic(false)), context)
                .await
                .err()
                .unwrap()
                .code,
            code
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
    for streaming in [false, true] {
        let mut bad = if streaming {
            Reply::stream(created())
        } else {
            Reply::json(native())
        };
        bad.headers.push_str("X-Codex-Turn-State: UNBOUND\r\n");
        if streaming {
            bad.stall = 2;
        }
        let mut fixture = Fixture::start(vec![bad, Reply::json(native())]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        let provider = fixture
            .provider(
                broker,
                vec![metadata("fixture", "native-fixture")],
                limits(),
            )
            .with_runtime_context();
        let error = if streaming {
            provider
                .stream_response(request(basic(true)), local_context())
                .await
                .err()
                .unwrap()
        } else {
            provider
                .create_response(request(basic(false)), local_context())
                .await
                .err()
                .unwrap()
        };
        assert_eq!(
            (error.http_status, error.code),
            (502, "deepseek_unsupported_turn_state")
        );
        if streaming {
            fixture.disconnected().await;
        }
        assert_eq!(
            *provider
                .create_response(request(basic(false)), local_context())
                .await
                .unwrap()
                .response
                .wire(),
            native()
        );
    }
}

#[tokio::test]
async fn gateway_runtime_context_is_consumed_locally_and_preserves_original_instructions() {
    let mut fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_runtime_context()
        .with_verbosity_instruction("low".into(), "Executor guidance".into())
        .unwrap();
    let gateway =
        caidex_model_gateway::start_with_provider(Arc::new(provider), broker.redactor(), limits())
            .await
            .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut outgoing = client
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .body(runtime_wire(false).to_string());
    for (name, value) in local_context().headers.iter() {
        outgoing = outgoing.header(name, value);
    }
    let response = outgoing.send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&response.bytes().await.unwrap()).unwrap(),
        native()
    );
    let captured = fixture.captured().await;
    assert!(
        !captured.headers.contains("LOCAL_")
            && !captured.headers.contains(gateway.token().expose())
    );
    let body = captured.body.unwrap();
    assert_eq!(
        body["instructions"],
        "Original instruction\nExecutor guidance"
    );
    assert_eq!(body["input"][0]["role"], "system");
    assert_eq!(
        body["input"][0]["content"],
        runtime_wire(false)["input"][0]["content"]
    );
    assert!(body.get("client_metadata").is_none() && body.get("prompt_cache_key").is_none());
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    gateway.shutdown().await.unwrap();
}
