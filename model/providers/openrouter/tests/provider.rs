//! Native adapter acceptance uses synthetic credentials and actual loopback I/O.
use caidex_credentials::{Broker, CredentialRef, Id, Secret, SecretKind, SecretStore};
use caidex_model_core::{
    CancellationToken, CanonicalRequest, CapabilitySupport, CredentialRequirement, EvidenceSource,
    ModelMetadata, ModelProvider, ProviderStreamEvent, REQUEST_HEADERS, RequestContext,
    ResponsesDialect,
};
use caidex_provider_openrouter::{Limits, OpenRouterConfig, OpenRouterProvider};
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
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};

const KEY: &str = "CAIDEX_SYNTHETIC_NATIVE_OPENROUTER_KEY";
const WAIT: Duration = Duration::from_secs(10);
const CREATED: &str = "data: {\"type\":\"response.created\",\"response\":{\"id\":\"fixture\"}}\n\n";

fn reference() -> CredentialRef {
    CredentialRef {
        owner: Id::new("executor").unwrap(),
        provider: Id::new("openrouter").unwrap(),
        profile: Id::new("fixture").unwrap(),
        kind: SecretKind::ApiKey,
    }
}
struct Store {
    reads: Arc<AtomicUsize>,
    key: Option<&'static str>,
}
impl SecretStore for Store {
    fn get(&self, reference: &CredentialRef) -> caidex_credentials::Result<Option<Secret>> {
        assert_eq!(reference.provider.as_str(), "openrouter");
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
fn broker(key: Option<&'static str>) -> (Arc<Broker<Store>>, Arc<AtomicUsize>) {
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
fn model(id: &str, native: &str) -> ModelMetadata {
    ModelMetadata::configured(id.into(), native.into(), vec![ResponsesDialect::Classic])
}
fn limits() -> Limits {
    Limits {
        header_timeout: Duration::from_secs(2),
        idle_timeout: Duration::from_secs(2),
        total_timeout: Duration::from_secs(4),
        ..Limits::default()
    }
}
fn request(stream: bool, dialect: ResponsesDialect) -> CanonicalRequest {
    CanonicalRequest::new(
        json!({"model":"fixture", "input":[], "stream":stream}),
        dialect,
    )
    .unwrap()
}
fn response_wire() -> Value {
    json!({"id":"fixture", "status":"completed", "output":[
        {"type":"message", "id":"msg_reply", "status":"completed", "role":"assistant", "content":[{"type":"output_text", "text":"中文🙂", "annotations":[]}]},
        {"type":"reasoning", "encrypted_content":"opaque+/==", "signature":"native-signature"},
        {"type":"future_item", "number":18446744073709551616_u128}
    ], "usage":{"input_tokens":3, "output_tokens":2}, "future":"retain"})
}
fn catalog() -> Value {
    json!({"data":[
        {"id":"native-fixture", "name":"Fixture", "created":42, "architecture":{"input_modalities":["text"]}, "supported_parameters":["tools","reasoning"], "pricing":{"prompt":"0.0000001"}, "future":{"big":18446744073709551616_u128}},
        {"id":"embedding-only", "context_length":1234}
    ]})
}
#[derive(Clone)]
struct Reply {
    status: u16,
    content_type: &'static str,
    headers: String,
    body: Vec<u8>,
    stall: u8,
}
impl Reply {
    fn json(wire: Value) -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            headers: String::new(),
            body: wire.to_string().into_bytes(),
            stall: 0,
        }
    }
    fn stream(body: String) -> Self {
        Self {
            content_type: "text/event-stream",
            body: body.into_bytes(),
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
                    let _ = serve(&mut socket, reply, tx).await;
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
    async fn request(&mut self) -> Captured {
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
    fn provider(&self, broker: Arc<Broker<Store>>, limits: Limits) -> OpenRouterProvider<Store> {
        self.provider_with(reference(), broker, limits)
    }
    fn provider_with(
        &self,
        credential: CredentialRef,
        broker: Arc<Broker<Store>>,
        limits: Limits,
    ) -> OpenRouterProvider<Store> {
        OpenRouterProvider::new(
            OpenRouterConfig::new(credential)
                .unwrap()
                .with_base_url(&self.base)
                .unwrap(),
            vec![
                model("fixture", "native-fixture"),
                model("not-visible", "absent"),
            ],
            broker,
            limits,
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn serve(
    socket: &mut TcpStream,
    reply: Reply,
    tx: mpsc::UnboundedSender<Captured>,
) -> std::io::Result<()> {
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        headers.push(socket.read_u8().await?);
        assert!(headers.len() < 32768);
    }
    let headers = String::from_utf8(headers).unwrap();
    let size = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    let mut bytes = vec![0; size];
    socket.read_exact(&mut bytes).await?;
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
        socket
            .write_all(
                format!(
                    "HTTP/1.1 {} Fixture\r\nContent-Type: {}\r\n{length}{}\r\n",
                    reply.status, reply.content_type, reply.headers
                )
                .as_bytes(),
            )
            .await?;
        if reply.stall == 2 {
            socket
                .write_all(format!("{:x}\r\n", reply.body.len()).as_bytes())
                .await?;
            socket.write_all(&reply.body).await?;
            socket.write_all(b"\r\n").await?;
        } else {
            socket.write_all(&reply.body).await?;
        }
    }
    if reply.stall != 0 {
        let _ = socket.read(&mut [0]).await?;
    }
    Ok(())
}

#[tokio::test]
async fn openrouter_missing_wrong_owner_and_cancelled_credentials_never_reach_network() {
    let fixture = Fixture::start(vec![Reply::json(catalog())]).await;
    let (missing, reads) = broker(None);
    let error = fixture
        .provider(missing, limits())
        .list_models()
        .await
        .unwrap_err();
    assert_eq!(error.code, "credential_missing");
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    let (broker, reads) = broker(Some(KEY));
    let mut wrong = reference();
    wrong.owner = Id::new("different-host").unwrap();
    let error = fixture
        .provider_with(wrong, broker.clone(), limits())
        .list_models()
        .await
        .unwrap_err();
    assert_eq!(error.code, "credential_unavailable");
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let provider = fixture.provider(broker, limits());
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let error = provider
        .discover_models(RequestContext {
            cancellation,
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "provider_cancelled");
    let error = provider
        .discover_models(RequestContext {
            deadline: Some(std::time::Instant::now()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "provider_timeout");
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn openrouter_discovery_get_enforces_safe_http_errors_no_redirects_content_type_and_size() {
    for (reply, code) in [
        (
            Reply {
                status: 401,
                body: KEY.as_bytes().to_vec(),
                ..Reply::json(Value::Null)
            },
            "provider_authentication_failed",
        ),
        (
            Reply {
                status: 429,
                headers: "Retry-After: 7\r\n".into(),
                ..Reply::json(Value::Null)
            },
            "provider_rate_limited",
        ),
        (
            Reply {
                status: 302,
                headers: "Location: /other\r\n".into(),
                ..Reply::json(Value::Null)
            },
            "provider_redirect_blocked",
        ),
        (
            Reply {
                content_type: "text/html",
                ..Reply::json(catalog())
            },
            "provider_invalid_content_type",
        ),
        (
            Reply {
                body: b"{broken".to_vec(),
                ..Reply::json(Value::Null)
            },
            "provider_invalid_response",
        ),
        (Reply::json(catalog()), "provider_response_too_large"),
    ] {
        let fixture = Fixture::start(vec![reply]).await;
        let (broker, _) = broker(Some(KEY));
        let mut limits = limits();
        if code == "provider_response_too_large" {
            limits.response_bytes = 8;
        }
        let error = fixture
            .provider(broker, limits)
            .list_models()
            .await
            .unwrap_err();
        assert_eq!(error.code, code);
        assert!(!error.to_string().contains(KEY));
        if code == "provider_rate_limited" {
            assert_eq!(error.retry_after_seconds, Some(7));
        }
        assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn openrouter_discovery_header_and_body_timeouts_and_cancellation_close_actual_socket() {
    for (stall, cancel) in [(1, false), (2, false), (2, true)] {
        let mut fixture = Fixture::start(vec![Reply {
            stall,
            ..Reply::json(catalog())
        }])
        .await;
        let (broker, _) = broker(Some(KEY));
        let short = Limits {
            header_timeout: Duration::from_millis(250),
            idle_timeout: Duration::from_millis(250),
            ..limits()
        };
        let provider = fixture.provider(broker, short);
        let cancellation = CancellationToken::new();
        let task_cancel = cancellation.clone();
        let task = tokio::spawn(async move {
            provider
                .discover_models(RequestContext {
                    cancellation: task_cancel,
                    ..Default::default()
                })
                .await
        });
        fixture.request().await;
        if cancel {
            cancellation.cancel();
        }
        let error = tokio::time::timeout(WAIT, task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(
            error.code,
            if cancel {
                "provider_cancelled"
            } else {
                "provider_timeout"
            }
        );
        fixture.disconnected().await;
    }
}

#[tokio::test]
async fn openrouter_native_stream_drop_closes_socket_and_releases_only_inflight_slot() {
    let mut fixture = Fixture::start(vec![
        Reply {
            stall: 2,
            ..Reply::stream(CREATED.into())
        },
        Reply::json(response_wire()),
    ])
    .await;
    let (broker, _) = broker(Some(KEY));
    let provider = fixture.provider(
        broker,
        Limits {
            in_flight: 1,
            ..limits()
        },
    );
    let mut stream = provider
        .stream_response(
            request(true, ResponsesDialect::Classic),
            RequestContext::default(),
        )
        .await
        .unwrap()
        .events;
    fixture.request().await;
    assert!(stream.next().await.unwrap().is_ok());
    let busy = provider.list_models().await.unwrap_err();
    assert_eq!(busy.code, "provider_busy");
    drop(stream);
    fixture.disconnected().await;
    provider
        .create_response(
            request(false, ResponsesDialect::Classic),
            RequestContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn openrouter_catalog_raw_and_six_methods_preserve_configured_intersection() {
    let mut fixture = Fixture::start(vec![Reply::json(catalog())]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture.provider(broker, limits());
    assert_eq!(
        provider.metadata("fixture").unwrap().source,
        EvidenceSource::Configured
    );
    assert_eq!(
        provider.capabilities("fixture").unwrap().streaming,
        CapabilitySupport::Unknown
    );
    assert!(
        matches!(provider.credential_requirements("fixture").unwrap(), CredentialRequirement::Bearer{reference:r} if r==reference())
    );
    assert!(provider.metadata("absent").is_err());
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let models = provider
        .discover_models(RequestContext::default())
        .await
        .unwrap();
    assert_eq!(
        models.iter().map(|m| m.id()).collect::<Vec<_>>(),
        ["embedding-only", "native-fixture"]
    );
    assert_eq!(models[1].wire(), &catalog()["data"][0]);
    assert!(!format!("{:?}", models[1]).contains("Fixture"));
    assert_eq!(
        serde_json::to_value(&models[1]).unwrap(),
        catalog()["data"][0]
    );
    let available = provider.list_models().await.unwrap();
    assert_eq!(available.len(), 1);
    assert_eq!(available[0].id, "fixture");
    assert_eq!(available[0].source, EvidenceSource::ProviderCatalog);
    assert_eq!(
        available[0].capabilities.reasoning,
        CapabilitySupport::Unknown
    );
    assert_eq!(available[0].capabilities.context_window, None);
    assert_eq!(available[0].codex_compatibility, None);
    for _ in 0..2 {
        let captured = fixture.request().await;
        assert!(
            captured
                .headers
                .starts_with("GET /proxy/v1/models HTTP/1.1\r\n")
        );
        assert!(captured.body.is_none());
        assert_eq!(
            captured.header("authorization"),
            Some(format!("Bearer {KEY}").as_str())
        );
        assert!(captured.header("openai-organization").is_none());
    }
}
#[tokio::test]
async fn openrouter_bad_catalogs_reject_without_cache_or_capability_guesses() {
    for wire in [
        json!({}),
        json!({"data":null}),
        json!({"data":[null]}),
        json!({"data":[{"id":""}]}),
        json!({"data":[{"id":"bad\n"}]}),
        json!({"data":[{"id":4}]}),
        json!({"data":[{"id":"same"},{"id":"same"}]}),
    ] {
        let fixture = Fixture::start(vec![Reply::json(wire)]).await;
        let (broker, _) = broker(Some(KEY));
        assert_eq!(
            fixture
                .provider(broker, limits())
                .list_models()
                .await
                .unwrap_err()
                .code,
            "openrouter_invalid_model_catalog"
        );
    }
    let fixture = Fixture::start(vec![Reply::json(json!({"data":[]}))]).await;
    let (broker, _) = broker(Some(KEY));
    assert!(
        fixture
            .provider(broker, limits())
            .list_models()
            .await
            .unwrap()
            .is_empty()
    );
}
#[tokio::test]
async fn openrouter_classic_text_roundtrip_keeps_payload_and_stateless_controls() {
    let mut fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture.provider(broker, limits());
    let first = json!({"model":"fixture","input":[{"type":"message","role":"developer","content":"Rules"},{"role":"user","content":[{"type":"input_text","text":"Question"}]}],"instructions":"priority","max_output_tokens":100,"temperature":0.5,"top_p":0.9});
    let response = provider
        .create_response(
            CanonicalRequest::new(first.clone(), ResponsesDialect::Classic).unwrap(),
            RequestContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(response.response.wire(), &response_wire());
    let sent = fixture.request().await;
    let mut expected = first.clone();
    expected["model"] = "native-fixture".into();
    expected["store"] = false.into();
    expected["provider"] = json!({"require_parameters":true,"allow_fallbacks":false});
    assert_eq!(sent.body.unwrap(), expected);
    assert!(
        sent.headers
            .starts_with("POST /proxy/v1/responses HTTP/1.1\r\n")
    );
    let mut next = first;
    next["input"]
        .as_array_mut()
        .unwrap()
        .push(response.response.output()[0].clone());
    next["input"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role":"user","content":"Next"}));
    provider
        .create_response(
            CanonicalRequest::new(next.clone(), ResponsesDialect::Classic).unwrap(),
            RequestContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        fixture.request().await.body.unwrap()["input"],
        next["input"]
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn openrouter_unsupported_state_routing_tools_media_and_controls_reject_before_key() {
    let fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture.provider(broker, limits());
    for (key, value) in [
        ("store", json!(true)),
        ("store", json!(5)),
        ("background", json!(true)),
        ("previous_response_id", json!("old")),
        ("provider", json!({"allow_fallbacks":true})),
        ("models", json!(["fallback"])),
        ("plugins", json!([])),
        ("reasoning", json!({"effort":"high"})),
        ("text", json!({"verbosity":"low"})),
        ("tools", json!([])),
        ("parallel_tool_calls", json!(false)),
        ("include", json!([])),
        ("client_metadata", json!({})),
        ("prompt_cache_key", json!("secret")),
    ] {
        for stream in [false, true] {
            let mut wire = json!({"model":"fixture","input":[],"stream":stream});
            wire[key] = value.clone();
            let req = CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap();
            let err = if stream {
                provider
                    .stream_response(req, RequestContext::default())
                    .await
                    .err()
                    .unwrap()
            } else {
                provider
                    .create_response(req, RequestContext::default())
                    .await
                    .err()
                    .unwrap()
            };
            assert_eq!(err.http_status, 400, "{key}");
        }
    }
    for item in [
        json!({"type":"function_call","call_id":"x","name":"exec","arguments":"{}"}),
        json!({"role":"user","content":[{"type":"input_image","image_url":"x"}]}),
        json!({"type":"reasoning","encrypted_content":"unknown"}),
        json!({"type":"message","role":"user","id":"x","content":[]}),
        json!({"role":"user","content":[{"type":"input_text","text":3}]}),
    ] {
        assert!(
            provider
                .create_response(
                    CanonicalRequest::new(
                        json!({"model":"fixture","input":[item]}),
                        ResponsesDialect::Classic
                    )
                    .unwrap(),
                    RequestContext::default()
                )
                .await
                .is_err()
        );
    }
    assert_eq!(
        provider
            .stream_response(
                request(true, ResponsesDialect::Lite),
                RequestContext::default()
            )
            .await
            .err()
            .unwrap()
            .code,
        "unsupported_dialect"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn openrouter_invalid_scalar_parameters_and_context_refuse_before_key() {
    let fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture.provider(broker, limits());
    for (key, value) in [
        ("instructions", json!([])),
        ("max_output_tokens", json!(0)),
        ("max_output_tokens", json!(-1)),
        ("max_output_tokens", json!(1.2)),
        ("temperature", json!(-0.1)),
        ("top_p", json!(1.1)),
        ("temperature", json!("high")),
    ] {
        let mut wire = json!({"model":"fixture","input":[]});
        wire[key] = value;
        assert_eq!(
            provider
                .create_response(
                    CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                    RequestContext::default()
                )
                .await
                .err()
                .unwrap()
                .code,
            "openrouter_invalid_request"
        );
    }
    for header in REQUEST_HEADERS {
        let mut context = RequestContext::default();
        context
            .headers
            .insert(header, "private".into(), REQUEST_HEADERS)
            .unwrap();
        assert_eq!(
            provider
                .create_response(request(false, ResponsesDialect::Classic), context)
                .await
                .err()
                .unwrap()
                .code,
            "openrouter_unsupported_context"
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}
#[test]
fn openrouter_configuration_validates_executor_reference_url_and_classic_routes() {
    assert!(OpenRouterConfig::new(reference()).is_ok());
    for kind in [SecretKind::AccessToken, SecretKind::ClientSecret] {
        let mut r = reference();
        r.kind = kind;
        assert!(OpenRouterConfig::new(r).is_err());
    }
    let mut r = reference();
    r.provider = Id::new("openai").unwrap();
    assert!(OpenRouterConfig::new(r).is_err());
    for url in [
        "http://example.com/v1",
        "https://user:pass@example.com/v1",
        "https://example.com/v1?key=x",
        "https://example.com/v1#x",
        "file:///tmp/secret",
    ] {
        assert!(
            OpenRouterConfig::new(reference())
                .unwrap()
                .with_base_url(url)
                .is_err()
        );
    }
    let (broker, reads) = broker(Some(KEY));
    let mut m = model("fixture", "native-fixture");
    m.dialects = vec![ResponsesDialect::Lite];
    assert!(
        OpenRouterProvider::new(
            OpenRouterConfig::new(reference()).unwrap(),
            vec![m],
            broker,
            limits()
        )
        .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert!(
        !format!(
            "{:?}",
            OpenRouterConfig::new(reference())
                .unwrap()
                .with_base_url("https://private.example.com/v1")
                .unwrap()
        )
        .contains("private")
    );
}
#[tokio::test]
async fn openrouter_json_sse_terminal_and_opaque_response_stay_lossless() {
    for status in ["completed", "incomplete", "failed"] {
        let mut wire = response_wire();
        wire["status"] = status.into();
        let body = CREATED.to_owned()
            + &format!(
                "data: {}\n\n",
                json!({"type":format!("response.{status}"),"response":wire})
            );
        let fixture = Fixture::start(vec![Reply::stream(body)]).await;
        let (broker, _) = broker(Some(KEY));
        let provider = fixture.provider(broker, limits());
        let mut response = provider
            .stream_response(
                request(true, ResponsesDialect::Classic),
                RequestContext::default(),
            )
            .await
            .unwrap();
        let mut terminal = None;
        while let Some(event) = response.events.next().await {
            if let ProviderStreamEvent::Model(model) = event.unwrap()
                && model.response.kind() == format!("response.{status}")
            {
                terminal = Some(model.response.wire()["response"].clone());
            }
        }
        assert_eq!(terminal.unwrap(), wire);
    }
}
#[tokio::test]
async fn openrouter_unrequested_native_calls_and_storage_never_deliver_success() {
    for item in [
        json!({"type":"function_call","name":"exec","call_id":"x","arguments":"{}"}),
        json!({"type":"custom_tool_call","name":"exec","call_id":"x","input":"write"}),
        json!({"type":"mcp_call"}),
        json!({"type":"tool_search_call","execution":"client","call_id":"x","arguments":{}}),
    ] {
        for streaming in [false, true] {
            let mut wire = response_wire();
            wire["output"].as_array_mut().unwrap().push(item.clone());
            let reply = if streaming {
                Reply::stream(
                    CREATED.to_owned()
                        + &format!(
                            "data: {}\n\n",
                            json!({"type":"response.output_item.added","output_index":3,"item":item})
                        ),
                )
            } else {
                Reply::json(wire)
            };
            let fixture = Fixture::start(vec![reply]).await;
            let (broker, _) = broker(Some(KEY));
            let provider = fixture.provider(broker, limits());
            if streaming {
                let mut events = provider
                    .stream_response(
                        request(true, ResponsesDialect::Classic),
                        RequestContext::default(),
                    )
                    .await
                    .unwrap()
                    .events;
                assert!(events.next().await.unwrap().is_ok());
                assert_eq!(
                    events.next().await.unwrap().err().unwrap().code,
                    "openrouter_unexpected_tool"
                );
                assert!(events.next().await.is_none());
            } else {
                assert_eq!(
                    provider
                        .create_response(
                            request(false, ResponsesDialect::Classic),
                            RequestContext::default()
                        )
                        .await
                        .err()
                        .unwrap()
                        .code,
                    "openrouter_unexpected_tool"
                );
            }
        }
    }
    let mut wire = response_wire();
    wire["store"] = true.into();
    let fixture = Fixture::start(vec![Reply::json(wire)]).await;
    let (broker, _) = broker(Some(KEY));
    assert_eq!(
        fixture
            .provider(broker, limits())
            .create_response(
                request(false, ResponsesDialect::Classic),
                RequestContext::default()
            )
            .await
            .err()
            .unwrap()
            .code,
        "openrouter_unexpected_storage"
    );
}
#[tokio::test]
async fn openrouter_source_compiled_budget_and_native_turn_state_are_bounded() {
    let fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker(Some(KEY));
    let original = request(false, ResponsesDialect::Classic);
    for budget in [
        original.wire().to_string().len() - 1,
        original.wire().to_string().len(),
    ] {
        let provider = fixture.provider(
            broker.clone(),
            Limits {
                request_bytes: budget,
                ..limits()
            },
        );
        assert_eq!(
            provider
                .create_response(original.clone(), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            413
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
    let fixture = Fixture::start(vec![Reply {
        headers: "X-Codex-Turn-State: private-state\r\n".into(),
        ..Reply::json(response_wire())
    }])
    .await;
    assert_eq!(
        fixture
            .provider(broker, limits())
            .create_response(original, RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "openrouter_unexpected_turn_state"
    );
}
#[tokio::test]
async fn openrouter_gateway_keeps_auth_routing_and_runtime_context_local() {
    let mut fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = Arc::new(fixture.provider(broker.clone(), limits()));
    let gateway = caidex_model_gateway::start_with_provider(provider, broker.redactor(), limits())
        .await
        .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let url = format!("http://{}/v1/responses", gateway.address());
    let wire = request(false, ResponsesDialect::Classic).wire().clone();
    assert_eq!(
        client
            .post(&url)
            .bearer_auth("wrong")
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&wire).unwrap())
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let denied = client
        .post(&url)
        .bearer_auth(gateway.token().expose())
        .header("x-client-request-id", "private-id")
        .header("content-type", "application/json")
        .body(serde_json::to_vec(&wire).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 400);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
    let response = client
        .post(&url)
        .bearer_auth(gateway.token().expose())
        .header("openai-organization", "org-attacker")
        .header("x-api-key", "bad")
        .header("cookie", "private-cookie")
        .header("content-type", "application/json")
        .body(serde_json::to_vec(&wire).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&response.bytes().await.unwrap()).unwrap(),
        response_wire()
    );
    let captured = fixture.request().await;
    assert_eq!(
        captured.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert!(!captured.headers.contains(gateway.token().expose()));
    for header in [
        "openai-organization",
        "openai-project",
        "cookie",
        "x-api-key",
        "x-client-request-id",
        "x-openai-internal-codex-responses-lite",
    ] {
        assert!(captured.header(header).is_none());
    }
    assert_eq!(
        captured.body.unwrap()["provider"],
        json!({"require_parameters":true,"allow_fallbacks":false})
    );
    gateway.shutdown().await.unwrap();
}

fn runtime_context() -> RequestContext {
    let mut context = RequestContext::default();
    for name in ["session_id", "x-client-request-id", "x-codex-turn-metadata"] {
        context
            .headers
            .insert(name, "executor-private".into(), REQUEST_HEADERS)
            .unwrap();
    }
    context
}
fn runtime_wire() -> Value {
    json!({"model":"fixture", "input":[
        {"type":"message", "role":"developer", "id":"msg_dev", "content":[{"type":"input_text", "text":"guidance"}]},
        {"type":"message", "role":"user", "id":"msg_user", "content":"hello"}
    ], "client_metadata":{"thread_id":"executor-private"}, "prompt_cache_key":"executor-private", "text":{"format":{"type":"text"}, "verbosity":null}})
}

#[tokio::test]
async fn openrouter_runtime_context_consumes_identity_and_preserves_text_ids() {
    let mut fixture =
        Fixture::start(vec![Reply::json(response_wire()), Reply::json(catalog())]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture.provider(broker, limits()).with_runtime_context();
    let source = runtime_wire();
    let response = provider
        .create_response(
            CanonicalRequest::new(source.clone(), ResponsesDialect::Classic).unwrap(),
            runtime_context(),
        )
        .await
        .unwrap();
    assert_eq!(response.response.wire(), &response_wire());
    let captured = fixture.request().await;
    assert_eq!(captured.body.as_ref().unwrap()["input"], source["input"]);
    for key in ["client_metadata", "prompt_cache_key", "text"] {
        assert!(captured.body.as_ref().unwrap().get(key).is_none());
    }
    assert!(!captured.headers.contains("executor-private"));
    assert!(
        !captured
            .body
            .unwrap()
            .to_string()
            .contains("executor-private")
    );
    provider.discover_models(runtime_context()).await.unwrap();
    assert!(!fixture.request().await.headers.contains("executor-private"));
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn openrouter_runtime_context_default_and_turn_state_still_reject_before_key() {
    let fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture.provider(broker, limits());
    assert!(
        provider
            .create_response(request(false, ResponsesDialect::Classic), runtime_context())
            .await
            .is_err()
    );
    assert!(
        provider
            .create_response(
                CanonicalRequest::new(runtime_wire(), ResponsesDialect::Classic).unwrap(),
                RequestContext::default()
            )
            .await
            .is_err()
    );
    let provider = provider.with_runtime_context();
    let mut context = runtime_context();
    context
        .headers
        .insert("x-codex-turn-state", "opaque".into(), REQUEST_HEADERS)
        .unwrap();
    assert_eq!(
        provider
            .create_response(request(false, ResponsesDialect::Classic), context)
            .await
            .err()
            .unwrap()
            .code,
        "openrouter_unsupported_context"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn openrouter_runtime_body_validation_keeps_history_and_non_neutral_controls_closed() {
    let fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture.provider(broker, limits()).with_runtime_context();
    for (key, value) in [
        ("client_metadata", json!({"thread_id":42})),
        ("client_metadata", json!([])),
        ("prompt_cache_key", json!("")),
        ("prompt_cache_key", json!("bad\n")),
        ("prompt_cache_key", json!(42)),
        ("text", json!({"verbosity":"low"})),
        ("text", json!({"format":{"type":"json_object"}})),
        ("text", json!({"format":{"type":"text", "future":true}})),
        ("text", json!({"future":true})),
        ("include", json!(["reasoning.encrypted_content"])),
        ("reasoning", json!({"summary":"auto"})),
        ("reasoning", json!({"context":"all_turns"})),
        ("service_tier", json!("priority")),
        ("provider", json!({"allow_fallbacks":true})),
        ("session_id", json!("native-sticky")),
    ] {
        let mut wire = runtime_wire();
        wire[key] = value;
        assert!(
            provider
                .create_response(
                    CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                    runtime_context()
                )
                .await
                .is_err(),
            "{key}"
        );
    }
    for item in [
        json!({"type":"message","role":"system","id":"msg_sys","content":"x"}),
        json!({"role":"user","id":"msg_user","content":"x"}),
        json!({"type":"message","role":"user","id":"","content":"x"}),
        json!({"type":"message","role":"user","id":"bad\n","content":"x"}),
        json!({"type":"message","role":"user","id":42,"content":"x"}),
        json!({"type":"message","role":"user","status":"completed","content":"x"}),
        json!({"type":"reasoning","encrypted_content":"foreign"}),
    ] {
        let mut wire = runtime_wire();
        wire["input"] = json!([item]);
        assert!(
            provider
                .create_response(
                    CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                    runtime_context()
                )
                .await
                .is_err()
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn openrouter_effort_mapping_is_explicit_per_route_and_applied_once() {
    let mut fixture = Fixture::start(vec![Reply::json(response_wire()); 2]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture
        .provider(broker, limits())
        .with_reasoning_effort_mapping("fixture".into(), "high".into(), "low".into())
        .unwrap()
        .with_reasoning_effort_mapping("fixture".into(), "low".into(), "medium".into())
        .unwrap();
    for (source, native) in [("high", "low"), ("low", "medium")] {
        let wire = json!({"model":"fixture","input":"text","reasoning":{"effort":source}});
        provider
            .create_response(
                CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                RequestContext::default(),
            )
            .await
            .unwrap();
        let captured = fixture.request().await.body.unwrap();
        assert_eq!(captured["reasoning"], json!({"effort":native}));
        assert_eq!(
            captured["provider"],
            json!({"require_parameters":true,"allow_fallbacks":false})
        );
    }
    for wire in [
        json!({"model":"not-visible","input":[],"reasoning":{"effort":"high"}}),
        json!({"model":"fixture","input":[],"reasoning":{"effort":"medium"}}),
        json!({"model":"fixture","input":[],"reasoning":{"effort":"high","exclude":true}}),
        json!({"model":"fixture","input":[],"reasoning":{"effort":"high","summary":null}}),
        json!({"model":"fixture","input":[],"reasoning":null}),
    ] {
        assert!(
            provider
                .create_response(
                    CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                    RequestContext::default()
                )
                .await
                .is_err()
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn openrouter_effort_configuration_and_unsupported_capability_fail_closed() {
    let fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker(Some(KEY));
    for (route, source, native) in [
        ("absent", "high", "low"),
        ("fixture", "bad", "low"),
        ("fixture", "high", "bad"),
    ] {
        assert!(
            fixture
                .provider(broker.clone(), limits())
                .with_reasoning_effort_mapping(route.into(), source.into(), native.into())
                .is_err()
        );
    }
    assert!(
        fixture
            .provider(broker.clone(), limits())
            .with_reasoning_effort_mapping("fixture".into(), "high".into(), "low".into())
            .unwrap()
            .with_reasoning_effort_mapping("fixture".into(), "high".into(), "medium".into())
            .is_err()
    );
    let mut metadata = model("fixture", "native-fixture");
    metadata.capabilities.reasoning = CapabilitySupport::Unsupported;
    let provider = OpenRouterProvider::new(
        OpenRouterConfig::new(reference())
            .unwrap()
            .with_base_url(&fixture.base)
            .unwrap(),
        vec![metadata],
        broker,
        limits(),
    )
    .unwrap()
    .with_reasoning_effort_mapping("fixture".into(), "high".into(), "low".into())
    .unwrap();
    let wire = json!({"model":"fixture","input":[],"reasoning":{"effort":"high"}});
    assert_eq!(
        provider
            .create_response(
                CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                RequestContext::default()
            )
            .await
            .err()
            .unwrap()
            .code,
        "unsupported_reasoning"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn openrouter_runtime_source_budget_precedes_local_identity_consumption() {
    let fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker(Some(KEY));
    let mut configured = limits();
    configured.request_bytes = 256;
    let provider = fixture.provider(broker, configured).with_runtime_context();
    let wire = json!({"model":"fixture","input":[],"client_metadata":{"private":"x".repeat(257)}});
    assert_eq!(
        provider
            .create_response(
                CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                RequestContext::default()
            )
            .await
            .err()
            .unwrap()
            .code,
        "invalid_or_oversized_body"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn openrouter_runtime_context_effort_sse_combination_preserves_native_events() {
    let event = json!({"type":"response.reasoning.delta", "delta":"opaque-native", "future":18446744073709551616_u128});
    let terminal = json!({"type":"response.completed", "response":response_wire()});
    let mut fixture = Fixture::start(vec![Reply::stream(format!(
        "{CREATED}data: {event}\n\ndata: {terminal}\n\n"
    ))])
    .await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture
        .provider(broker, limits())
        .with_runtime_context()
        .with_reasoning_effort_mapping("fixture".into(), "xhigh".into(), "high".into())
        .unwrap();
    let mut wire = runtime_wire();
    wire["stream"] = true.into();
    wire["reasoning"] = json!({"effort":"xhigh"});
    let mut events = provider
        .stream_response(
            CanonicalRequest::new(wire.clone(), ResponsesDialect::Classic).unwrap(),
            runtime_context(),
        )
        .await
        .unwrap()
        .events;
    let captured = fixture.request().await;
    assert_eq!(
        captured.body.as_ref().unwrap()["reasoning"],
        json!({"effort":"high"})
    );
    assert!(!captured.headers.contains("executor-private"));
    let mut actual = Vec::new();
    while let Some(next) = events.next().await {
        if let ProviderStreamEvent::Model(model) = next.unwrap() {
            actual.push(model.response.wire().clone());
        }
    }
    assert!(actual.contains(&event));
    assert!(actual.contains(&terminal));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn openrouter_explicit_effort_levels_and_none_do_not_upgrade_capabilities() {
    let mut fixture = Fixture::start(vec![Reply::json(response_wire()); 7]).await;
    let (broker, reads) = broker(Some(KEY));
    let mut metadata = model("fixture", "native-fixture");
    metadata.capabilities.reasoning = CapabilitySupport::Unsupported;
    let none = OpenRouterProvider::new(
        OpenRouterConfig::new(reference())
            .unwrap()
            .with_base_url(&fixture.base)
            .unwrap(),
        vec![metadata],
        broker.clone(),
        limits(),
    )
    .unwrap()
    .with_reasoning_effort_mapping("fixture".into(), "none".into(), "none".into())
    .unwrap();
    let mut provider = fixture.provider(broker, limits());
    for level in ["minimal", "low", "medium", "high", "xhigh", "max"] {
        provider = provider
            .with_reasoning_effort_mapping("fixture".into(), level.into(), level.into())
            .unwrap();
    }
    for level in ["none", "minimal", "low", "medium", "high", "xhigh", "max"] {
        let selected = if level == "none" { &none } else { &provider };
        selected
            .create_response(
                CanonicalRequest::new(
                    json!({"model":"fixture","input":[],"reasoning":{"effort":level}}),
                    ResponsesDialect::Classic,
                )
                .unwrap(),
                RequestContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(
            fixture.request().await.body.unwrap()["reasoning"],
            json!({"effort":level})
        );
    }
    assert_eq!(
        none.capabilities("fixture").unwrap().reasoning,
        CapabilitySupport::Unsupported
    );
    assert_eq!(
        provider.capabilities("fixture").unwrap().reasoning,
        CapabilitySupport::Unknown
    );
    assert_eq!(reads.load(Ordering::SeqCst), 7);
}

#[tokio::test]
async fn openrouter_explicit_context_gateway_keeps_listener_and_model_identity_separate() {
    let mut fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = Arc::new(
        fixture
            .provider(broker.clone(), limits())
            .with_runtime_context()
            .with_reasoning_effort_mapping("fixture".into(), "high".into(), "medium".into())
            .unwrap(),
    );
    let gateway = caidex_model_gateway::start_with_provider(provider, broker.redactor(), limits())
        .await
        .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let url = format!("http://{}/v1/responses", gateway.address());
    let mut wire = runtime_wire();
    wire["reasoning"] = json!({"effort":"high"});
    let denied = client
        .post(&url)
        .bearer_auth("wrong")
        .header("content-type", "application/json")
        .body(serde_json::to_vec(&wire).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let response = client
        .post(&url)
        .bearer_auth(gateway.token().expose())
        .header("session_id", "executor-private")
        .header("x-client-request-id", "executor-private")
        .header("x-codex-turn-metadata", "executor-private")
        .header("x-api-key", "attacker")
        .header("cookie", "attacker")
        .header("content-type", "application/json")
        .body(serde_json::to_vec(&wire).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let captured = fixture.request().await;
    assert_eq!(
        captured.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert!(!captured.headers.contains(gateway.token().expose()));
    assert!(!captured.headers.contains("executor-private"));
    assert!(!captured.headers.contains("attacker"));
    assert_eq!(
        captured.body.as_ref().unwrap()["reasoning"],
        json!({"effort":"medium"})
    );
    assert!(
        !captured
            .body
            .unwrap()
            .to_string()
            .contains("executor-private")
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn openrouter_verbosity_guidance_is_per_route_and_keeps_original_instructions() {
    let mut fixture = Fixture::start(vec![Reply::json(response_wire()); 3]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture
        .provider(broker, limits())
        .with_runtime_context()
        .with_verbosity_instruction("fixture".into(), "low".into(), "Concise 中文🙂".into())
        .unwrap()
        .with_verbosity_instruction("fixture".into(), "medium".into(), "Balanced".into())
        .unwrap()
        .with_verbosity_instruction("fixture".into(), "high".into(), "Detailed".into())
        .unwrap();
    for (level, guidance, original) in [
        ("low", "Concise 中文🙂", json!("Original")),
        ("medium", "Balanced", Value::Null),
        ("high", "Detailed", json!("")),
    ] {
        let mut wire = runtime_wire();
        wire["instructions"] = original.clone();
        wire["text"]["verbosity"] = level.into();
        provider
            .create_response(
                CanonicalRequest::new(wire.clone(), ResponsesDialect::Classic).unwrap(),
                runtime_context(),
            )
            .await
            .unwrap();
        let captured = fixture.request().await.body.unwrap();
        assert_eq!(captured["input"], wire["input"]);
        assert_eq!(
            captured["instructions"],
            if original == "Original" {
                format!("Original\n{guidance}")
            } else {
                guidance.into()
            }
        );
        assert!(captured.get("text").is_none());
        assert!(captured.get("client_metadata").is_none());
    }
    let wire = json!({"model":"not-visible","input":[],"text":{"verbosity":"low"}});
    assert!(
        provider
            .create_response(
                CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                RequestContext::default()
            )
            .await
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn openrouter_verbosity_mapping_does_not_enable_runtime_identity_or_structured_output() {
    let mut fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture
        .provider(broker, limits())
        .with_verbosity_instruction("fixture".into(), "low".into(), "Concise".into())
        .unwrap();
    let wire = json!({"model":"fixture", "input":"plain", "text":{"verbosity":"low"}});
    provider
        .create_response(
            CanonicalRequest::new(wire.clone(), ResponsesDialect::Classic).unwrap(),
            RequestContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        fixture.request().await.body.unwrap()["instructions"],
        "Concise"
    );
    assert!(
        provider
            .create_response(
                CanonicalRequest::new(wire.clone(), ResponsesDialect::Classic).unwrap(),
                runtime_context()
            )
            .await
            .is_err()
    );
    for (key, value) in [
        ("client_metadata", json!({"thread_id":"private"})),
        ("text", json!({"format":{"type":"text"},"verbosity":"low"})),
        (
            "text",
            json!({"format":{"type":"json_schema"},"verbosity":"low"}),
        ),
        ("text", Value::Null),
    ] {
        let mut invalid = wire.clone();
        invalid[key] = value;
        assert!(
            provider
                .create_response(
                    CanonicalRequest::new(invalid, ResponsesDialect::Classic).unwrap(),
                    RequestContext::default()
                )
                .await
                .is_err()
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn openrouter_body_control_configuration_rejects_bad_routes_values_and_duplicates() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = broker(Some(KEY));
    for (route, level, guidance) in [
        ("absent", "low", "guide"),
        ("fixture", "bad", "guide"),
        ("fixture", "low", " "),
    ] {
        assert!(
            fixture
                .provider(broker.clone(), limits())
                .with_verbosity_instruction(route.into(), level.into(), guidance.into())
                .is_err()
        );
    }
    assert!(
        fixture
            .provider(broker.clone(), limits())
            .with_verbosity_instruction("fixture".into(), "low".into(), "guide".into())
            .unwrap()
            .with_verbosity_instruction("fixture".into(), "low".into(), "other".into())
            .is_err()
    );
    for (route, source, native) in [
        ("absent", "default", "default"),
        ("fixture", "bad", "default"),
        ("fixture", "default", "bad"),
        ("fixture", "priority", "default"),
        ("fixture", "flex", "priority"),
        ("fixture", "scale", "auto"),
    ] {
        assert!(
            fixture
                .provider(broker.clone(), limits())
                .with_service_tier_mapping(route.into(), source.into(), native.into())
                .is_err()
        );
    }
    assert!(
        fixture
            .provider(broker, limits())
            .with_service_tier_mapping("fixture".into(), "priority".into(), "fast".into())
            .unwrap()
            .with_service_tier_mapping("fixture".into(), "priority".into(), "priority".into())
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn openrouter_configured_body_controls_reject_invalid_and_history_controls_before_key() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture
        .provider(broker, limits())
        .with_runtime_context()
        .with_verbosity_instruction("fixture".into(), "low".into(), "guide".into())
        .unwrap()
        .with_service_tier_mapping("fixture".into(), "priority".into(), "fast".into())
        .unwrap();
    for (key, value) in [
        ("text", json!({"verbosity":42})),
        ("text", json!({"verbosity":"high"})),
        ("text", json!({"verbosity":"low","future":true})),
        (
            "text",
            json!({"verbosity":"low","format":{"type":"json_object"}}),
        ),
        (
            "text",
            json!({"verbosity":"low","format":{"type":"text","future":true}}),
        ),
        ("instructions", json!([])),
        ("service_tier", json!(42)),
        ("service_tier", json!("default")),
        ("service_tier", json!("bad")),
        ("provider", json!({"allow_fallbacks":true})),
        ("speed", json!("fast")),
        ("include", json!(["reasoning.encrypted_content"])),
        ("reasoning", json!({"summary":"auto"})),
        (
            "input",
            json!([{"type":"reasoning","encrypted_content":"foreign"}]),
        ),
    ] {
        let mut wire = runtime_wire();
        wire["text"]["verbosity"] = "low".into();
        wire["service_tier"] = "priority".into();
        wire[key] = value;
        assert!(
            provider
                .create_response(
                    CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                    runtime_context()
                )
                .await
                .is_err(),
            "{key}"
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn openrouter_service_tier_mapping_is_per_route_once_and_preserves_actual_tier() {
    let mut native = response_wire();
    native["service_tier"] = "default".into();
    let mut fixture = Fixture::start(vec![Reply::json(native.clone()); 6]).await;
    let (broker, reads) = broker(Some(KEY));
    let mut provider = fixture.provider(broker, limits());
    for (source, target) in [
        ("auto", "default"),
        ("default", "auto"),
        ("flex", "flex"),
        ("priority", "fast"),
        ("fast", "priority"),
        ("ultrafast", "ultrafast"),
    ] {
        provider = provider
            .with_service_tier_mapping("fixture".into(), source.into(), target.into())
            .unwrap();
    }
    for (source, target) in [
        ("auto", "default"),
        ("default", "auto"),
        ("flex", "flex"),
        ("priority", "fast"),
        ("fast", "priority"),
        ("ultrafast", "ultrafast"),
    ] {
        let wire = json!({"model":"fixture","input":[],"service_tier":source});
        let response = provider
            .create_response(
                CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                RequestContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(response.response.wire(), &native);
        let sent = fixture.request().await.body.unwrap();
        assert_eq!(sent["service_tier"], target);
        assert_eq!(
            sent["provider"],
            json!({"require_parameters":true,"allow_fallbacks":false})
        );
    }
    let wire = json!({"model":"not-visible","input":[],"service_tier":"priority"});
    assert!(
        provider
            .create_response(
                CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                RequestContext::default()
            )
            .await
            .is_err()
    );
    assert_eq!(
        provider.capabilities("fixture").unwrap().reasoning,
        CapabilitySupport::Unknown
    );
    assert_eq!(reads.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn openrouter_verbosity_guidance_source_and_compiled_budgets_precede_credentials() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = broker(Some(KEY));
    let mut configured = limits();
    configured.request_bytes = 256;
    let provider = fixture
        .provider(broker, configured)
        .with_runtime_context()
        .with_verbosity_instruction("fixture".into(), "low".into(), "g".repeat(257))
        .unwrap();
    for wire in [
        json!({"model":"fixture","input":[],"text":{"verbosity":"low"}}),
        json!({"model":"fixture","input":[],"text":{"verbosity":"low"},"client_metadata":{"private":"x".repeat(257)}}),
    ] {
        assert_eq!(
            provider
                .create_response(
                    CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                    RequestContext::default()
                )
                .await
                .err()
                .unwrap()
                .code,
            "invalid_or_oversized_body"
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn openrouter_body_context_effort_sse_combination_keeps_terminal_tier_and_native_output() {
    let mut native = response_wire();
    native["service_tier"] = "default".into();
    let terminal = json!({"type":"response.completed","response":native});
    let mut fixture = Fixture::start(vec![Reply::stream(format!(
        "{CREATED}data: {terminal}\n\n"
    ))])
    .await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture
        .provider(broker, limits())
        .with_runtime_context()
        .with_verbosity_instruction("fixture".into(), "low".into(), "Concise".into())
        .unwrap()
        .with_service_tier_mapping("fixture".into(), "priority".into(), "fast".into())
        .unwrap()
        .with_reasoning_effort_mapping("fixture".into(), "xhigh".into(), "high".into())
        .unwrap();
    let mut wire = runtime_wire();
    wire["stream"] = true.into();
    wire["text"]["verbosity"] = "low".into();
    wire["service_tier"] = "priority".into();
    wire["reasoning"] = json!({"effort":"xhigh"});
    let mut events = provider
        .stream_response(
            CanonicalRequest::new(wire.clone(), ResponsesDialect::Classic).unwrap(),
            runtime_context(),
        )
        .await
        .unwrap()
        .events;
    let sent = fixture.request().await;
    assert_eq!(sent.body.as_ref().unwrap()["instructions"], "Concise");
    assert_eq!(
        sent.body.as_ref().unwrap()["reasoning"],
        json!({"effort":"high"})
    );
    assert_eq!(sent.body.as_ref().unwrap()["service_tier"], "fast");
    assert_eq!(sent.body.as_ref().unwrap()["input"], wire["input"]);
    assert!(!sent.headers.contains("executor-private"));
    let mut actual = Vec::new();
    while let Some(next) = events.next().await {
        if let ProviderStreamEvent::Model(model) = next.unwrap() {
            actual.push(model.response.wire().clone());
        }
    }
    assert!(actual.contains(&terminal));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn openrouter_body_controls_do_not_enable_null_tier_or_unconfigured_default() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture.provider(broker.clone(), limits());
    for value in [json!("priority"), Value::Null] {
        let wire = json!({"model":"fixture","input":[],"service_tier":value});
        assert!(
            provider
                .create_response(
                    CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                    RequestContext::default()
                )
                .await
                .is_err()
        );
    }
    let provider = provider
        .with_verbosity_instruction("fixture".into(), "low".into(), "guide".into())
        .unwrap();
    let wire = json!({"model":"fixture","input":[],"service_tier":null,"text":{"verbosity":"low"}});
    assert!(
        provider
            .create_response(
                CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                RequestContext::default()
            )
            .await
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn openrouter_backend_selection_is_per_route_and_keeps_native_output() {
    let mut fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture
        .provider(broker, limits())
        .with_backend_selection("fixture".into(), "google-vertex/us-east5".into())
        .unwrap();
    for (route, backend) in [
        ("fixture", Some("google-vertex/us-east5")),
        ("not-visible", None),
    ] {
        let wire = json!({"model":route,"input":"hello"});
        let response = provider
            .create_response(
                CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                RequestContext::default(),
            )
            .await
            .unwrap();
        let sent = fixture.request().await.body.unwrap();
        let mut expected = json!({"require_parameters":true,"allow_fallbacks":false});
        if let Some(backend) = backend {
            expected["only"] = json!([backend]);
        }
        assert_eq!(sent["provider"], expected);
        assert_eq!(
            sent["model"],
            if route == "fixture" {
                "native-fixture"
            } else {
                "absent"
            }
        );
        assert_eq!(sent["store"], false);
        assert_eq!(response.response.wire(), &response_wire());
        assert_eq!(
            provider.metadata(route).unwrap().capabilities.reasoning,
            CapabilitySupport::Unknown
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn openrouter_backend_configuration_rejects_invalid_routes_slugs_and_duplicates() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = broker(Some(KEY));
    for slug in [
        "",
        " ",
        "openai\n",
        "open ai",
        "/openai",
        "openai/",
        "openai//fast",
        "https://example.test",
        "供应方",
    ] {
        assert!(
            fixture
                .provider(broker.clone(), limits())
                .with_backend_selection("fixture".into(), slug.into())
                .is_err(),
            "{slug:?}"
        );
    }
    assert!(
        fixture
            .provider(broker.clone(), limits())
            .with_backend_selection("missing".into(), "openai".into())
            .is_err()
    );
    assert!(
        fixture
            .provider(broker, limits())
            .with_backend_selection("fixture".into(), "openai".into())
            .unwrap()
            .with_backend_selection("fixture".into(), "azure".into())
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn openrouter_backend_selection_does_not_enable_caller_routing_tools_or_history() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture
        .provider(broker, limits())
        .with_runtime_context()
        .with_backend_selection("fixture".into(), "openai/fast".into())
        .unwrap();
    let mut wires = Vec::new();
    for (field, value) in [
        ("provider", json!({"only":["azure"]})),
        ("models", json!(["other"])),
        ("session_id", json!("sticky")),
        ("tools", json!([])),
        ("include", json!(["reasoning.encrypted_content"])),
    ] {
        let mut wire = json!({"model":"fixture","input":[]});
        wire[field] = value;
        wires.push(wire);
    }
    wires.push(
        json!({"model":"fixture","input":[{"type":"reasoning","encrypted_content":"foreign"}]}),
    );
    for wire in wires {
        assert!(
            provider
                .create_response(
                    CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                    RequestContext::default()
                )
                .await
                .is_err()
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn openrouter_backend_compiled_budget_precedes_credentials_and_post() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = broker(Some(KEY));
    let mut configured = limits();
    configured.request_bytes = 256;
    let provider = fixture
        .provider(broker, configured)
        .with_backend_selection("fixture".into(), "b".repeat(257))
        .unwrap();
    let error = provider
        .create_response(
            request(false, ResponsesDialect::Classic),
            RequestContext::default(),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "invalid_or_oversized_body");
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn openrouter_backend_sse_combines_controls_without_rewriting_routing_metadata() {
    let mut native = response_wire();
    native["service_tier"] = "default".into();
    native["openrouter_metadata"] = json!({"requested":"native-fixture","attempt":1,"endpoints":{"available":[{"provider":"Fixture Provider","model":"native-fixture","selected":true}]},"future":"retain"});
    let terminal = json!({"type":"response.completed","response":native});
    let mut fixture = Fixture::start(vec![Reply::stream(format!(
        "{CREATED}data: {terminal}\n\n"
    ))])
    .await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture
        .provider(broker, limits())
        .with_runtime_context()
        .with_backend_selection("fixture".into(), "fixture-provider/fast".into())
        .unwrap()
        .with_verbosity_instruction("fixture".into(), "low".into(), "Concise".into())
        .unwrap()
        .with_reasoning_effort_mapping("fixture".into(), "xhigh".into(), "high".into())
        .unwrap()
        .with_service_tier_mapping("fixture".into(), "priority".into(), "fast".into())
        .unwrap();
    let mut wire = runtime_wire();
    wire["stream"] = true.into();
    wire["instructions"] = "original".into();
    wire["text"]["verbosity"] = "low".into();
    wire["reasoning"] = json!({"effort":"xhigh"});
    wire["service_tier"] = "priority".into();
    let mut events = provider
        .stream_response(
            CanonicalRequest::new(wire.clone(), ResponsesDialect::Classic).unwrap(),
            runtime_context(),
        )
        .await
        .unwrap()
        .events;
    let sent = fixture.request().await;
    let body = sent.body.unwrap();
    assert_eq!(
        body["provider"],
        json!({"only":["fixture-provider/fast"],"require_parameters":true,"allow_fallbacks":false})
    );
    assert_eq!(body["input"], wire["input"]);
    assert_eq!(body["instructions"], "original\nConcise");
    assert_eq!(body["reasoning"], json!({"effort":"high"}));
    assert_eq!(body["service_tier"], "fast");
    assert!(!sent.headers.contains("executor-private"));
    let mut actual = Vec::new();
    while let Some(event) = events.next().await {
        if let ProviderStreamEvent::Model(model) = event.unwrap() {
            actual.push(model.response.wire().clone());
        }
    }
    assert!(actual.contains(&terminal));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

mod tools;
