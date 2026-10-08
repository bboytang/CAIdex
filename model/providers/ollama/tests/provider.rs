//! Offline Ollama protocol fixtures: no daemon, model download or user key.
mod models;
use caidex_credentials::{Broker, CredentialRef, Id, Secret, SecretKind, SecretStore};
use caidex_model_core::{
    CancellationToken, CanonicalRequest, ContextHeaders, CredentialRequirement, EvidenceSource,
    ModelMetadata, ModelProvider, ProviderStreamEvent, REQUEST_HEADERS, RequestContext,
    ResponsesDialect, StreamState,
};
use caidex_provider_ollama::{Limits, OllamaConfig, OllamaProvider};
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
const KEY: &str = "CAIDEX_SYNTHETIC_OLLAMA_KEY";
const WAIT: Duration = Duration::from_secs(10);
const CREATED: &str = "event: response.created\ndata: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\"fixture\",\"status\":\"in_progress\",\"output\":[]}}\n\n";
fn reference() -> CredentialRef {
    CredentialRef {
        owner: Id::new("executor").unwrap(),
        provider: Id::new("ollama").unwrap(),
        profile: Id::new("fixture").unwrap(),
        kind: SecretKind::ApiKey,
    }
}
struct Store(Arc<AtomicUsize>);
impl SecretStore for Store {
    fn get(&self, r: &CredentialRef) -> caidex_credentials::Result<Option<Secret>> {
        assert_eq!(r.provider.as_str(), "ollama");
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Some(Secret::new(KEY.into())?))
    }
    fn set(&self, _: &CredentialRef, _: &Secret) -> caidex_credentials::Result<()> {
        unreachable!()
    }
    fn remove(&self, _: &CredentialRef) -> caidex_credentials::Result<bool> {
        unreachable!()
    }
}
fn broker() -> (Arc<Broker<Store>>, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    (
        Arc::new(Broker::new(
            Id::new("executor").unwrap(),
            Store(reads.clone()),
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
        in_flight: 1,
        ..Limits::default()
    }
}
fn request(wire: Value) -> CanonicalRequest {
    CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap()
}
fn response_wire() -> Value {
    json!({"id":"fixture", "object":"response", "status":"completed", "output":[
        {"type":"function_call", "id":"fc_1", "status":"completed", "name":"echo", "call_id":"c1", "arguments":" { \"n\": 1.00 } "}
    ], "usage":{"input_tokens":3, "output_tokens":2, "total_tokens":5},
    "future":{"big":18446744073709551616_u128}})
}
fn catalog() -> Value {
    json!({"object":"list", "data":[{"id":"native-fixture", "object":"model", "created":42,
        "owned_by":"library", "future":{"big":18446744073709551616_u128}},
        {"id":"unconfigured", "object":"model", "created":1, "owned_by":"library"}]})
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
    fn provider(&self, broker: Arc<Broker<Store>>, authenticated: bool) -> OllamaProvider<Store> {
        OllamaProvider::new(
            OllamaConfig::new(&self.base, authenticated.then(reference)).unwrap(),
            vec![
                model("fixture", "native-fixture"),
                model("absent", "not-in-catalog"),
            ],
            broker,
            limits(),
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
async fn unsupported_semantics_are_rejected_before_credentials_or_post() {
    let fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker();
    let provider = fixture.provider(broker, true);
    let unsupported = [
        json!({"previous_response_id":"saved"}),
        json!({"conversation":{"id":"saved"}}),
        json!({"store":true}),
        json!({"background":true}),
        json!({"parallel_tool_calls":false}),
        json!({"tool_choice":"required"}),
        json!({"include":["reasoning.encrypted_content"]}),
        json!({"reasoning":{"effort":"high"}}),
        json!({"text":{"verbosity":"low"}}),
        json!({"metadata":{"secret":"do-not-forward"}}),
        json!({"prompt_cache_key":"cache"}),
        json!({"service_tier":"priority"}),
        json!({"truncation":"auto"}),
        json!({"future_control":true}),
        json!({"tools":[{"type":"custom","name":"exec"}]}),
        json!({"tools":[{"type":"web_search"}]}),
        json!({"tools":[{"type":"tool_search"}]}),
        json!({"tools":[{"type":"namespace","name":"n","tools":[]}]}),
        json!({"tools":[{"type":"function","name":"echo","parameters":{},"strict":true}]}),
        json!({"tools":[{"type":"function","name":"echo","parameters":{},"defer_loading":true}]}),
        json!({"input":[{"role":"developer","content":"policy"}]}),
        json!({"input":[{"type":"future_item","opaque":"retain-or-reject"}]}),
        json!({"input":[{"type":"reasoning","encrypted_content":"other-provider"}]}),
        json!({"input":[{"role":"user","content":[{"type":"input_image","image_url":"https://invalid.test/image"}]}]}),
        json!({"input":[{"role":"user","content":"text","future_policy":true}]}),
        json!({"input":[{"type":"function_call_output","call_id":"missing","output":"orphan"}]}),
    ];
    for streaming in [false, true] {
        for extra in &unsupported {
            let mut wire = json!({"model":"fixture","input":"hello","stream":streaming});
            wire.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let error = if streaming {
                provider
                    .stream_response(request(wire), RequestContext::default())
                    .await
                    .err()
                    .unwrap()
            } else {
                provider
                    .create_response(request(wire), RequestContext::default())
                    .await
                    .err()
                    .unwrap()
            };
            assert_eq!(error.http_status, 400, "{extra}");
            assert!(error.code.starts_with("ollama_"), "{error:?}");
        }
    }
    let mut headers = ContextHeaders::default();
    headers
        .insert("session_id", "local-only".into(), REQUEST_HEADERS)
        .unwrap();
    let error = provider
        .create_response(
            request(json!({"model":"fixture","input":"hello"})),
            RequestContext {
                headers,
                ..RequestContext::default()
            },
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "ollama_unsupported_context_headers");
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[test]
fn configuration_requires_explicit_safe_ollama_credential_and_classic_route() {
    for url in [
        "http://example.test/v1",
        "http://localhost:11434/v1",
        "https://user:pass@example.test/v1",
        "https://example.test/v1?key=secret",
        "https://example.test/v1#secret",
    ] {
        assert!(OllamaConfig::new(url, None).is_err());
    }
    let mut wrong = reference();
    wrong.provider = Id::new("openai").unwrap();
    assert!(OllamaConfig::new("https://example.test/v1", Some(wrong)).is_err());
    let mut wrong = reference();
    wrong.kind = SecretKind::RefreshToken;
    assert!(OllamaConfig::new("https://example.test/v1", Some(wrong)).is_err());
    let (broker, reads) = broker();
    let mut lite = model("fixture", "native-fixture");
    lite.dialects = vec![ResponsesDialect::Lite];
    assert!(
        OllamaProvider::new(
            OllamaConfig::new("http://127.0.0.1:11434/v1", None).unwrap(),
            vec![lite],
            broker,
            limits()
        )
        .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn catalog_and_six_methods_use_native_inventory_without_capability_guesses() {
    for authenticated in [false, true] {
        let mut fixture = Fixture::start(vec![Reply::json(catalog())]).await;
        let (broker, reads) = broker();
        let provider = fixture.provider(broker, authenticated);
        assert_eq!(
            provider.metadata("fixture").unwrap().source,
            EvidenceSource::Configured
        );
        assert_eq!(
            provider.capabilities("fixture").unwrap(),
            Default::default()
        );
        let requirement = provider.credential_requirements("fixture").unwrap();
        assert_eq!(
            matches!(requirement, CredentialRequirement::Bearer { .. }),
            authenticated
        );
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        let native = provider
            .discover_models(RequestContext::default())
            .await
            .unwrap();
        assert_eq!(native.len(), 2);
        assert_eq!(native[0].owned_by(), "library");
        assert_eq!(native[0].created(), 42);
        assert_eq!(native[0].wire()["future"], catalog()["data"][0]["future"]);
        let available = provider.list_models().await.unwrap();
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].id, "fixture");
        assert_eq!(available[0].source, EvidenceSource::ProviderCatalog);
        assert!(available[0].codex_compatibility.is_none());
        for _ in 0..2 {
            let captured = fixture.request().await;
            assert!(captured.headers.starts_with("GET /proxy/v1/models "));
            assert_eq!(
                captured.header("authorization"),
                authenticated.then_some(format!("Bearer {KEY}")).as_deref()
            );
            assert!(captured.header("openai-project").is_none());
        }
        assert_eq!(
            reads.load(Ordering::SeqCst),
            if authenticated { 2 } else { 0 }
        );
    }
}

#[tokio::test]
async fn stateless_function_history_replays_exact_http_wire_and_preserves_response_extensions() {
    let message = json!({"type":"message","id":"msg_2","role":"assistant","status":"completed",
        "content":[{"type":"output_text","text":"中文🙂","annotations":[],"logprobs":[]}]});
    let mut final_wire = response_wire();
    final_wire["output"] = json!([message]);
    let mut fixture = Fixture::start(vec![
        Reply::json(response_wire()),
        Reply::json(final_wire.clone()),
        Reply::json(final_wire.clone()),
    ])
    .await;
    let (broker, reads) = broker();
    let provider = fixture.provider(broker, false);
    let original = json!({"model":"fixture","input":[{"role":"system","content":"policy"},
        {"role":"user","content":[{"type":"input_text","text":"hello"}]}],
        "instructions":"prefix","temperature":0.2,"top_p":0.9,"max_output_tokens":128,
        "tools":[{"type":"function","name":"echo","description":"data only","strict":false,
            "parameters":{"type":"object","properties":{"n":{"type":"number"}},"required":["n"]}}],
        "store":false,"background":false,"tool_choice":"auto","parallel_tool_calls":true});
    let first = provider
        .create_response(request(original.clone()), RequestContext::default())
        .await
        .unwrap();
    assert_eq!(first.response.wire(), &response_wire());
    let mut expected = original.clone();
    expected["model"] = "native-fixture".into();
    for key in ["store", "background", "tool_choice", "parallel_tool_calls"] {
        expected.as_object_mut().unwrap().remove(key);
    }
    let captured = fixture.request().await;
    assert!(captured.headers.starts_with("POST /proxy/v1/responses "));
    assert_eq!(captured.body, Some(expected));
    assert!(captured.header("authorization").is_none());
    assert!(
        captured
            .header("x-openai-internal-codex-responses-lite")
            .is_none()
    );
    let mut next = original.clone();
    next["input"]
        .as_array_mut()
        .unwrap()
        .extend(first.response.output().to_vec());
    next["input"].as_array_mut().unwrap().push(json!({"type":"function_call_output","call_id":"c1","output":"exact result\n { \"n\": 1.00 } 🙂"}));
    let second = provider
        .create_response(request(next.clone()), RequestContext::default())
        .await
        .unwrap();
    assert_eq!(second.response.wire(), &final_wire);
    let captured = fixture.request().await.body.unwrap();
    assert_eq!(captured["input"], next["input"]);
    assert_eq!(captured["tools"], original["tools"]);
    next["input"]
        .as_array_mut()
        .unwrap()
        .extend(second.response.output().to_vec());
    // IDs may be reused by later completed pairs, while pending IDs stay unique.
    next["input"].as_array_mut().unwrap().extend([
        json!({"role":"user","content":"again"}),
        json!({"type":"function_call","name":"echo","call_id":"c1","arguments":" { \"n\": 2.00 } "}),
        json!({"type":"function_call_output","call_id":"c1","output":[{"type":"output_text","text":"second result"}]}),
    ]);
    provider
        .create_response(request(next.clone()), RequestContext::default())
        .await
        .unwrap();
    assert_eq!(
        fixture.request().await.body.unwrap()["input"],
        next["input"]
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn typed_stream_keeps_function_arguments_usage_and_native_plaintext_reasoning() {
    let mut terminal = response_wire();
    terminal["output"].as_array_mut().unwrap().insert(0,json!({"type":"reasoning","id":"rs_1",
        "summary":[{"type":"summary_text","text":"plain thought🙂"}],"encrypted_content":"plain thought🙂"}));
    let done = json!({"type":"response.completed","sequence_number":2,"response":terminal});
    let delta = json!({"type":"response.reasoning_summary_text.delta","sequence_number":1,
        "item_id":"rs_1","output_index":0,"summary_index":0,"delta":"plain thought🙂"});
    let mut fixture=Fixture::start(vec![Reply::stream(format!("{CREATED}event: response.reasoning_summary_text.delta\ndata: {delta}\n\nevent: response.completed\ndata: {done}\n\n"))]).await;
    let (broker, reads) = broker();
    let provider = fixture.provider(broker, true);
    let mut stream = provider
        .stream_response(
            request(json!({"model":"fixture","input":"hello","stream":true})),
            RequestContext::default(),
        )
        .await
        .unwrap()
        .events;
    let mut events = Vec::new();
    while let Some(event) = tokio::time::timeout(WAIT, stream.next()).await.unwrap() {
        if let ProviderStreamEvent::Model(event) = event.unwrap() {
            events.push(event);
        }
    }
    assert_eq!(events.len(), 3);
    assert_eq!(events[1].response.wire(), &delta);
    assert_eq!(events[2].response.wire(), &done);
    assert_eq!(events[2].response.terminal(), Some(StreamState::Completed));
    assert_eq!(
        fixture.request().await.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancelled_expired_wrong_owner_and_busy_requests_do_not_post_and_drop_releases_socket() {
    let mut reply = Reply::stream(CREATED.into());
    reply.stall = 2;
    let mut fixture = Fixture::start(vec![reply]).await;
    let (broker, reads) = broker();
    let provider = fixture.provider(broker.clone(), true);
    let wire = json!({"model":"fixture","input":"hello","stream":true});
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert_eq!(
        provider
            .stream_response(
                request(wire.clone()),
                RequestContext {
                    cancellation: cancelled,
                    ..Default::default()
                }
            )
            .await
            .err()
            .unwrap()
            .code,
        "provider_cancelled"
    );
    assert_eq!(
        provider
            .stream_response(
                request(wire.clone()),
                RequestContext {
                    deadline: Some(std::time::Instant::now()),
                    ..Default::default()
                }
            )
            .await
            .err()
            .unwrap()
            .code,
        "provider_timeout"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let mut wrong = reference();
    wrong.owner = Id::new("other-host").unwrap();
    let wrong_provider = OllamaProvider::new(
        OllamaConfig::new(&fixture.base, Some(wrong)).unwrap(),
        vec![model("fixture", "native-fixture")],
        broker,
        limits(),
    )
    .unwrap();
    assert_eq!(
        wrong_provider
            .stream_response(request(wire.clone()), RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "credential_unavailable"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
    let mut stream = provider
        .stream_response(request(wire.clone()), RequestContext::default())
        .await
        .unwrap()
        .events;
    stream.next().await.unwrap().unwrap();
    fixture.request().await;
    assert_eq!(
        provider
            .stream_response(request(wire.clone()), RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "provider_busy"
    );
    drop(stream);
    fixture.disconnected().await;
    let cancellation = CancellationToken::new();
    let mut stream = provider
        .stream_response(
            request(wire.clone()),
            RequestContext {
                cancellation: cancellation.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .events;
    stream.next().await.unwrap().unwrap();
    fixture.request().await;
    cancellation.cancel();
    fixture.disconnected().await;
    assert_eq!(
        stream.next().await.unwrap().err().unwrap().code,
        "provider_cancelled"
    );
    drop(stream);
    let mut stream = provider
        .stream_response(
            request(wire),
            RequestContext {
                deadline: Some(std::time::Instant::now() + Duration::from_millis(200)),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .events;
    stream.next().await.unwrap().unwrap();
    fixture.request().await;
    fixture.disconnected().await;
    assert_eq!(
        stream.next().await.unwrap().err().unwrap().code,
        "provider_timeout"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 3);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn gateway_keeps_token_local_and_applies_ollama_guards_before_authentication() {
    let mut fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker();
    let provider = Arc::new(fixture.provider(broker.clone(), true));
    let gateway = caidex_model_gateway::start_with_provider(provider, broker.redactor(), limits())
        .await
        .unwrap();
    let client = reqwest::Client::builder()
        .no_proxy()
        .retry(reqwest::retry::never())
        .build()
        .unwrap();
    let endpoint = format!("http://{}/v1/responses", gateway.address());
    let response = client
        .post(&endpoint)
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .body(json!({"model":"fixture","input":"hello"}).to_string())
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
    let response = client
        .post(&endpoint)
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .body(json!({"model":"fixture","input":"hello","store":true}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    assert_eq!(
        serde_json::from_slice::<Value>(&response.bytes().await.unwrap()).unwrap()["error"]["code"],
        "ollama_unsupported_request"
    );
    let response = client
        .post(&endpoint)
        .bearer_auth(gateway.token().expose())
        .header("session_id", "local-only")
        .header("content-type", "application/json")
        .body(json!({"model":"fixture","input":"hello"}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    assert_eq!(
        serde_json::from_slice::<Value>(&response.bytes().await.unwrap()).unwrap()["error"]["code"],
        "ollama_unsupported_context_headers"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn invalid_parameters_history_and_explicit_unsupported_model_capabilities_are_local_errors() {
    let fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker();
    let provider = fixture.provider(broker.clone(), true);
    for extra in [
        json!({"max_output_tokens":0}),
        json!({"temperature":-1}),
        json!({"top_p":2}),
        json!({"instructions":{}}),
        json!({"tools":null}),
        json!({"input":[{"type":null,"role":"user","content":"hello"}]}),
        json!({"input":[{"type":"function_call","name":"echo","call_id":"c1","arguments":"[]"}]}),
        json!({"input":[{"type":"function_call","name":"echo","call_id":"c1","arguments":"{}"}]}),
        json!({"input":[{"type":"function_call","name":"echo","call_id":"c1","arguments":"{}"},
            {"type":"function_call_output","call_id":"c1","output":"first"},{"type":"function_call_output","call_id":"c1","output":"second"}]}),
    ] {
        let mut wire = json!({"model":"fixture","input":"hello"});
        wire.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert_eq!(
            provider
                .create_response(request(wire), RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            "ollama_invalid_request"
        );
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
    let mut configured = model("fixture", "native-fixture");
    configured.capabilities.native_tools = caidex_model_core::CapabilitySupport::Unsupported;
    let restricted = OllamaProvider::new(
        OllamaConfig::new(&fixture.base, Some(reference())).unwrap(),
        vec![configured],
        broker,
        limits(),
    )
    .unwrap();
    assert_eq!(restricted.create_response(request(json!({"model":"fixture","input":"hello","tools":[{"type":"function","name":"echo","parameters":{}}]})),RequestContext::default()).await.err().unwrap().code,"ollama_unsupported_capability");
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn bad_catalog_http_errors_and_unsupported_response_turn_state_never_fabricate_success() {
    let mut duplicate = catalog();
    duplicate["data"]
        .as_array_mut()
        .unwrap()
        .push(catalog()["data"][0].clone());
    let mut missing_time = catalog();
    missing_time["data"][0]
        .as_object_mut()
        .unwrap()
        .remove("created");
    for wire in [
        duplicate,
        missing_time,
        json!({"object":"list","data":null}),
    ] {
        let mut fixture = Fixture::start(vec![Reply::json(wire)]).await;
        let (broker, _) = broker();
        let provider = fixture.provider(broker, false);
        assert_eq!(
            provider.list_models().await.err().unwrap().code,
            "provider_invalid_model_catalog"
        );
        fixture.request().await;
        assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
    }
    for (status, code) in [
        (401, "provider_authentication_failed"),
        (429, "provider_rate_limited"),
        (302, "provider_redirect_blocked"),
    ] {
        let mut reply = Reply::json(json!({"error":{"message":KEY}}));
        reply.status = status;
        reply.headers = "Retry-After: 2\r\nLocation: https://invalid.test/secret\r\n".into();
        let mut fixture = Fixture::start(vec![reply]).await;
        let (broker, reads) = broker();
        let provider = fixture.provider(broker, true);
        let error = provider
            .create_response(
                request(json!({"model":"fixture","input":"hello"})),
                RequestContext::default(),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, code);
        assert!(!format!("{error:?}").contains(KEY));
        if status == 429 {
            assert_eq!(error.retry_after_seconds, Some(2));
        }
        fixture.request().await;
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
    }
    for streaming in [false, true] {
        let mut reply = if streaming {
            Reply::stream(CREATED.into())
        } else {
            Reply::json(response_wire())
        };
        reply.headers = "x-codex-turn-state: unsupported-native-state\r\n".into();
        let mut fixture = Fixture::start(vec![reply]).await;
        let (broker, _) = broker();
        let provider = fixture.provider(broker, false);
        let wire = json!({"model":"fixture","input":"hello","stream":streaming});
        let error = if streaming {
            provider
                .stream_response(request(wire), RequestContext::default())
                .await
                .err()
                .unwrap()
        } else {
            provider
                .create_response(request(wire), RequestContext::default())
                .await
                .err()
                .unwrap()
        };
        assert_eq!(error.code, "ollama_unsupported_turn_state");
        fixture.request().await;
    }
}

#[tokio::test]
async fn failed_incomplete_and_truncated_responses_retain_their_actual_terminal_state() {
    for (status, expected) in [
        ("failed", StreamState::Failed),
        ("incomplete", StreamState::Incomplete),
    ] {
        let mut wire = response_wire();
        wire["status"] = status.into();
        wire["output"] = json!([]);
        if status == "failed" {
            wire["error"] = json!({"message":KEY});
        }
        let fixture = Fixture::start(vec![Reply::json(wire)]).await;
        let (broker, _) = broker();
        let provider = fixture.provider(broker, true);
        let response = provider
            .create_response(
                request(json!({"model":"fixture","input":"hello"})),
                RequestContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(response.response.state(), expected);
        assert!(!response.response.wire()["error"].to_string().contains(KEY));
    }
    let fixture = Fixture::start(vec![Reply::stream(CREATED.into())]).await;
    let (broker, _) = broker();
    let provider = fixture.provider(broker, false);
    let mut stream = provider
        .stream_response(
            request(json!({"model":"fixture","input":"hello","stream":true})),
            RequestContext::default(),
        )
        .await
        .unwrap()
        .events;
    stream.next().await.unwrap().unwrap();
    assert_eq!(
        stream.next().await.unwrap().err().unwrap().code,
        "provider_stream_truncated"
    );
    assert!(stream.next().await.is_none());
}
