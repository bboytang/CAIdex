//! Native adapter acceptance uses synthetic credentials and actual loopback I/O.
use caidex_credentials::{Broker, CredentialRef, Id, Secret, SecretKind, SecretStore};
use caidex_model_core::{
    CancellationToken, CanonicalRequest, CapabilitySupport, ContextHeaders, CredentialRequirement,
    EvidenceSource, ModelMetadata, ModelProvider, ProviderStreamEvent, REQUEST_HEADERS,
    RequestContext, ResponsesDialect, StreamState,
};
use caidex_provider_openai::{Limits, OpenAiConfig, OpenAiProvider};
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

const KEY: &str = "CAIDEX_SYNTHETIC_NATIVE_OPENAI_KEY";
const WAIT: Duration = Duration::from_secs(10);
const CREATED: &str = "data: {\"type\":\"response.created\",\"response\":{\"id\":\"fixture\"}}\n\n";

fn reference() -> CredentialRef {
    CredentialRef {
        owner: Id::new("executor").unwrap(),
        provider: Id::new("openai").unwrap(),
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
        assert_eq!(reference.provider.as_str(), "openai");
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
    ModelMetadata::configured(
        id.into(),
        native.into(),
        vec![ResponsesDialect::Classic, ResponsesDialect::Lite],
    )
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
        {"type":"message", "role":"assistant", "phase":"final_answer", "content":[{"type":"output_text", "text":"中文🙂"}]},
        {"type":"reasoning", "encrypted_content":"opaque+/==", "signature":"native-signature"},
        {"type":"function_call", "name":"data_only", "call_id":"call", "arguments":" { \"n\": 1.00 } ", "encrypted_function_args":"args+/=="},
        {"type":"future_item", "number":18446744073709551616_u128}
    ], "usage":{"input_tokens":3, "output_tokens":2, "cache_extension":{"precise":"0.001"}}, "future":"retain"})
}
fn catalog() -> Value {
    json!({"object":"list", "data":[
        {"object":"model", "id":"native-fixture", "created":42, "owned_by":"fixture-owner", "shutdown_date":null, "future":{"big":18446744073709551616_u128}},
        {"object":"model", "id":"embedding-only", "created":1, "owned_by":"fixture-owner", "shutdown_date":"2099-01-01"}
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
    fn provider(&self, broker: Arc<Broker<Store>>, limits: Limits) -> OpenAiProvider<Store> {
        self.provider_with(reference(), broker, limits)
    }
    fn provider_with(
        &self,
        credential: CredentialRef,
        broker: Arc<Broker<Store>>,
        limits: Limits,
    ) -> OpenAiProvider<Store> {
        OpenAiProvider::new(
            OpenAiConfig::new(credential)
                .unwrap()
                .with_base_url(&self.base)
                .unwrap()
                .with_scope(Some("org-executor"), Some("proj-executor"))
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
async fn discovery_preserves_native_catalog_and_lists_only_configured_available_responses_models() {
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
        matches!(provider.credential_requirements("fixture").unwrap(), CredentialRequirement::Bearer{reference:r} if r == reference())
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let discovered = provider
        .discover_models(RequestContext::default())
        .await
        .unwrap();
    assert_eq!(
        discovered.iter().map(|m| m.id()).collect::<Vec<_>>(),
        ["embedding-only", "native-fixture"]
    );
    assert_eq!(discovered[0].shutdown_date(), Some("2099-01-01"));
    assert_eq!(discovered[1].owned_by(), "fixture-owner");
    assert_eq!(discovered[1].created(), 42);
    assert_eq!(discovered[1].wire(), &catalog()["data"][0]);
    assert!(!format!("{:?}", discovered[1]).contains("fixture-owner"));
    let available = provider.list_models().await.unwrap();
    assert_eq!(available.len(), 1);
    assert_eq!(available[0].id, "fixture");
    assert_eq!(available[0].source, EvidenceSource::ProviderCatalog);
    assert_eq!(available[0].capabilities.context_window, None);
    assert_eq!(available[0].codex_compatibility, None);
    assert!(provider.metadata("embedding-only").is_err());
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
        assert_eq!(captured.header("openai-organization"), Some("org-executor"));
        assert_eq!(captured.header("openai-project"), Some("proj-executor"));
        assert!(
            captured
                .header("x-openai-internal-codex-responses-lite")
                .is_none()
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn invalid_duplicate_catalogs_fail_without_fabricated_or_cached_fallback() {
    let item = catalog()["data"][0].clone();
    for wire in [
        json!({"data":[]}),
        json!({"object":"list","data":[item.clone(),item]}),
        json!({"object":"list","data":[{"object":"model","id":"broken","created":-1,"owned_by":"owner"}]}),
    ] {
        let fixture = Fixture::start(vec![Reply::json(wire)]).await;
        let (broker, _) = broker(Some(KEY));
        let error = fixture
            .provider(broker, limits())
            .list_models()
            .await
            .unwrap_err();
        assert_eq!(error.code, "provider_invalid_model_catalog");
    }
    let fixture = Fixture::start(vec![Reply::json(json!({"object":"list", "data":[]}))]).await;
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
async fn native_two_turn_stateless_history_and_structured_payload_are_lossless() {
    let mut reply = Reply::json(response_wire());
    reply.headers =
        "X-Request-Id: provider-id\r\nX-Codex-Turn-State: state+/==\r\nSet-Cookie: forbidden\r\n"
            .into();
    let mut fixture = Fixture::start(vec![reply]).await;
    let (broker, _) = broker(Some(KEY));
    let provider = fixture.provider(broker, limits());
    let mut wire = json!({"model":"fixture","input":[{"role":"user","content":[{"type":"input_text","text":"中文🙂"},{"type":"input_image","image_url":"data:image/png;base64,AA=="}]}],
        "instructions":"fixture", "tools":[{"type":"function","name":"data_only","parameters":{"type":"object"}}],
        "text":{"format":{"type":"json_schema","name":"fixture","schema":{"type":"object"}}},
        "reasoning":{"context":"all_turns"}, "future":{"big":18446744073709551616_u128}});
    let mut headers = ContextHeaders::default();
    headers
        .insert("session_id", "session".into(), REQUEST_HEADERS)
        .unwrap();
    let response = provider
        .create_response(
            CanonicalRequest::new(wire.clone(), ResponsesDialect::Classic).unwrap(),
            RequestContext {
                headers,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(response.response.wire(), &response_wire());
    assert_eq!(response.headers.get("x-request-id"), Some("provider-id"));
    assert_eq!(
        response.headers.get("x-codex-turn-state"),
        Some("state+/==")
    );
    assert_eq!(response.headers.iter().count(), 2);
    let first = fixture.request().await;
    wire["model"] = "native-fixture".into();
    wire["store"] = false.into();
    assert_eq!(first.body.as_ref().unwrap(), &wire);
    assert!(
        first
            .headers
            .starts_with("POST /proxy/v1/responses HTTP/1.1\r\n")
    );
    assert_eq!(first.header("session_id"), Some("session"));
    assert_eq!(first.header("openai-project"), Some("proj-executor"));
    let mut input = wire["input"].as_array().unwrap().clone();
    input.extend(response.response.output().iter().cloned());
    input.push(json!({"role":"user","content":"continue"}));
    let second = CanonicalRequest::new(
        json!({"model":"fixture", "input":input}),
        ResponsesDialect::Classic,
    )
    .unwrap();
    provider
        .create_response(second.clone(), RequestContext::default())
        .await
        .unwrap();
    let replay = fixture.request().await.body.unwrap();
    assert_eq!(replay["input"], second.wire()["input"]);
    assert_eq!(replay["input"][2]["encrypted_content"], "opaque+/==");
    assert_eq!(replay["input"][3]["encrypted_function_args"], "args+/==");
    assert_eq!(replay["input"][1]["phase"], "final_answer");
    assert_eq!(replay["store"], false);
}

#[tokio::test]
async fn explicit_storage_opt_in_and_foreground_validation_happen_before_credential_reads() {
    let mut fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = fixture.provider(broker, limits());
    for store in [Value::Null, json!(true), json!(false)] {
        let wire = json!({"model":"fixture", "input":[], "store":store, "background":false});
        provider
            .create_response(
                CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                RequestContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(
            fixture.request().await.body.unwrap()["store"],
            if store.is_null() { json!(false) } else { store }
        );
    }
    for (field, value, code) in [
        (
            "background",
            json!(true),
            "unsupported_background_generation",
        ),
        ("background", json!("true"), "invalid_model_request"),
        ("store", json!(1), "invalid_model_request"),
    ] {
        let mut wire = request(false, ResponsesDialect::Classic).wire().clone();
        wire[field] = value;
        let error = provider
            .create_response(
                CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                RequestContext::default(),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, code);
    }
    assert_eq!(reads.load(Ordering::SeqCst), 3);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn native_classic_and_explicit_lite_streams_keep_terminal_state_and_opaque_items() {
    let done = format!(
        "data: {}\n\n",
        json!({"type":"response.completed", "response":response_wire()})
    );
    let mut fixture = Fixture::start(vec![Reply::stream(format!("{CREATED}{done}"))]).await;
    let (broker, _) = broker(Some(KEY));
    let provider = fixture.provider(broker, limits());
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let mut stream = provider
            .stream_response(request(true, dialect), RequestContext::default())
            .await
            .unwrap()
            .events;
        let mut terminal = None;
        while let Some(event) = tokio::time::timeout(WAIT, stream.next()).await.unwrap() {
            if let ProviderStreamEvent::Model(event) = event.unwrap() {
                terminal = event.response.terminal();
                if terminal.is_some() {
                    assert_eq!(event.response.wire()["response"], response_wire());
                }
            }
        }
        assert_eq!(terminal, Some(StreamState::Completed));
        let captured = fixture.request().await;
        assert_eq!(
            captured.header("x-openai-internal-codex-responses-lite"),
            if dialect == ResponsesDialect::Lite {
                Some("true")
            } else {
                None
            }
        );
        let body = captured.body.unwrap();
        assert_eq!(body["model"], "native-fixture");
        assert_eq!(body["store"], false);
        assert!(body.get("instructions").is_none() && body.get("tools").is_none());
    }
}

#[test]
fn native_configuration_requires_openai_bearer_reference_and_fixed_safe_scope_url() {
    for (provider, kind) in [
        ("custom", SecretKind::ApiKey),
        ("openai", SecretKind::RefreshToken),
        ("openai", SecretKind::ClientSecret),
    ] {
        let mut r = reference();
        r.provider = Id::new(provider).unwrap();
        r.kind = kind;
        assert!(OpenAiConfig::new(r).is_err());
    }
    let mut r = reference();
    r.kind = SecretKind::AccessToken;
    assert!(OpenAiConfig::new(r).is_ok());
    for base in [
        "http://example.com/v1",
        "http://localhost/v1",
        "https://user:pass@example.com/v1",
        "https://example.com/v1?key=synthetic",
        "https://example.com/v1#fragment",
    ] {
        assert!(
            OpenAiConfig::new(reference())
                .unwrap()
                .with_base_url(base)
                .is_err()
        );
    }
    for scope in ["", "org\r\nattack: injected", "包含中文", "has space"] {
        assert!(
            OpenAiConfig::new(reference())
                .unwrap()
                .with_scope(Some(scope), None)
                .is_err()
        );
    }
    assert!(
        !format!(
            "{:?}",
            OpenAiConfig::new(reference())
                .unwrap()
                .with_scope(None, Some("proj-private"))
                .unwrap()
        )
        .contains("proj-private")
    );
}

#[tokio::test]
async fn missing_wrong_owner_and_cancelled_credentials_never_reach_network() {
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
async fn discovery_get_enforces_safe_http_errors_no_redirects_content_type_and_size() {
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
async fn discovery_header_and_body_timeouts_and_cancellation_close_actual_socket() {
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
async fn native_stream_drop_closes_socket_and_releases_only_inflight_slot() {
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
            request(true, ResponsesDialect::Lite),
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
async fn injected_native_gateway_keeps_executor_scope_token_and_context_boundaries() {
    let mut fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = Arc::new(fixture.provider(broker.clone(), limits()));
    let gateway = caidex_model_gateway::start_with_provider(provider, broker.redactor(), limits())
        .await
        .unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert!(
        !broker
            .redactor()
            .text(gateway.token().expose())
            .contains(gateway.token().expose())
    );
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let url = format!("http://{}/v1/responses", gateway.address());
    let denied = client
        .post(&url)
        .bearer_auth("wrong")
        .header("content-type", "application/json")
        .body(serde_json::to_vec(request(false, ResponsesDialect::Classic).wire()).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let response = client
        .post(&url)
        .bearer_auth(gateway.token().expose())
        .header("openai-organization", "org-attacker")
        .header("openai-project", "proj-attacker")
        .header("x-api-key", "attacker-key")
        .header("cookie", "forbidden")
        .header("x-client-request-id", "client-id")
        .header("content-type", "application/json")
        .body(serde_json::to_vec(request(false, ResponsesDialect::Classic).wire()).unwrap())
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
    assert_eq!(captured.header("openai-organization"), Some("org-executor"));
    assert_eq!(captured.header("openai-project"), Some("proj-executor"));
    assert_eq!(captured.header("x-client-request-id"), Some("client-id"));
    assert!(captured.header("cookie").is_none() && captured.header("x-api-key").is_none());
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    gateway.shutdown().await.unwrap();
}
