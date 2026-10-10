//! Real loopback sockets and synthetic credentials, never a commercial model.
mod router;
use caidex_credentials::{Broker, CredentialRef, Id, Secret, SecretKind, SecretStore};
use caidex_model_core::{ResponsesDialect, ResponsesStream, StreamState};
use caidex_model_gateway::{CustomResponses, Limits, ModelRoute, RunningGateway};
use reqwest::{Client, StatusCode, header};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
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

const KEY: &str = "CAIDEX_SYNTHETIC_PROVIDER_KEY";
const WAIT: Duration = Duration::from_secs(10);
const DONE: &str = "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"fixture\",\"status\":\"completed\"}}\n\n";
const CREATED: &str = "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"fixture\"}}\n\n";

fn reference(owner: &str) -> CredentialRef {
    CredentialRef {
        owner: Id::new(owner).unwrap(),
        provider: Id::new("custom").unwrap(),
        profile: Id::new("fixture").unwrap(),
        kind: SecretKind::ApiKey,
    }
}
struct Store {
    value: Option<&'static str>,
    reads: Arc<AtomicUsize>,
}
impl SecretStore for Store {
    fn get(&self, _: &CredentialRef) -> caidex_credentials::Result<Option<Secret>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.value
            .map(|value| Secret::new(value.into()))
            .transpose()
    }
    fn set(&self, _: &CredentialRef, _: &Secret) -> caidex_credentials::Result<()> {
        unreachable!("gateway must not modify credentials")
    }
    fn remove(&self, _: &CredentialRef) -> caidex_credentials::Result<bool> {
        unreachable!("gateway must not remove credentials")
    }
}

#[derive(Clone)]
enum Scenario {
    Reply {
        status: u16,
        content_type: &'static str,
        extra: String,
        body: Vec<u8>,
        fragmented: bool,
    },
    StallHeaders,
    StallBody,
    Heartbeats,
    Flood,
}
fn sse(body: impl Into<Vec<u8>>) -> Scenario {
    Scenario::Reply {
        status: 200,
        content_type: "text/event-stream",
        extra: String::new(),
        body: body.into(),
        fragmented: false,
    }
}
struct Captured {
    headers: String,
    body: Value,
}
struct Fixture {
    address: SocketAddr,
    requests: mpsc::UnboundedReceiver<Captured>,
    closed: mpsc::UnboundedReceiver<()>,
    produced: Arc<AtomicUsize>,
    accepted: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}
impl Fixture {
    async fn start(scenario: Scenario) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, requests) = mpsc::unbounded_channel();
        let (closed_tx, closed) = mpsc::unbounded_channel();
        let produced = Arc::new(AtomicUsize::new(0));
        let count = produced.clone();
        let accepted = Arc::new(AtomicUsize::new(0));
        let accepts = accepted.clone();
        let task = tokio::spawn(async move {
            let mut sessions = JoinSet::new();
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                accepts.fetch_add(1, Ordering::SeqCst);
                let tx = tx.clone();
                let closed_tx = closed_tx.clone();
                let scenario = scenario.clone();
                let count = count.clone();
                sessions.spawn(async move {
                    let mut headers = Vec::new();
                    while !headers.ends_with(b"\r\n\r\n") {
                        let mut byte = [0];
                        if stream.read_exact(&mut byte).await.is_err() { return; }
                        headers.push(byte[0]);
                        assert!(headers.len() < 32768);
                    }
                    let headers = String::from_utf8(headers).unwrap();
                    let size: usize = headers.lines().find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse().unwrap())
                    }).unwrap();
                    let mut body = vec![0; size];
                    stream.read_exact(&mut body).await.unwrap();
                    tx.send(Captured { headers, body: serde_json::from_slice(&body).unwrap() }).unwrap();
                    let result: std::io::Result<()> = async {
                        match scenario {
                            Scenario::Reply { status, content_type, extra, body, fragmented } => {
                                stream.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{extra}\r\n", body.len()).as_bytes()).await?;
                                if fragmented {
                                    for byte in body { stream.write_all(&[byte]).await?; tokio::task::yield_now().await; }
                                } else { stream.write_all(&body).await?; }
                            }
                            Scenario::StallHeaders => { let _ = stream.read(&mut [0]).await; }
                            Scenario::StallBody => {
                                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").await?;
                                chunk(&mut stream, CREATED.as_bytes()).await?;
                                let _ = stream.read(&mut [0]).await;
                            }
                            Scenario::Heartbeats => {
                                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").await?;
                                loop { chunk(&mut stream, b": keepalive\n\n").await?; tokio::time::sleep(Duration::from_millis(10)).await; }
                            }
                            Scenario::Flood => {
                                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").await?;
                                let event = format!("data: {}\n\n", json!({"type":"response.output_text.delta", "delta":"x".repeat(8192)}));
                                for _ in 0..8192 {
                                    chunk(&mut stream, event.as_bytes()).await?;
                                    count.fetch_add(event.len(), Ordering::SeqCst);
                                }
                                chunk(&mut stream, DONE.as_bytes()).await?;
                                stream.write_all(b"0\r\n\r\n").await?;
                            }
                        }
                        Ok(())
                    }.await;
                    let _ = result; // A reset/broken pipe is an expected cancellation.
                    let _ = closed_tx.send(());
                });
            }
        });
        Self {
            address,
            requests,
            closed,
            produced,
            accepted,
            task,
        }
    }
    fn endpoint(&self) -> String {
        format!("http://{}/v1/responses", self.address)
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
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn chunk(stream: &mut TcpStream, bytes: &[u8]) -> std::io::Result<()> {
    stream
        .write_all(format!("{:x}\r\n", bytes.len()).as_bytes())
        .await?;
    stream.write_all(bytes).await?;
    stream.write_all(b"\r\n").await
}

fn route(
    endpoint: &str,
    credential: Option<CredentialRef>,
    dialects: Vec<ResponsesDialect>,
) -> ModelRoute {
    ModelRoute::new(
        "fixture".into(),
        "native-fixture".into(),
        dialects,
        CustomResponses::new(endpoint, credential).unwrap(),
    )
    .unwrap()
}
async fn gateway(
    fixture: &Fixture,
    limits: Limits,
    owner: &str,
    key: Option<&'static str>,
) -> (RunningGateway, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let broker = Arc::new(Broker::new(
        Id::new("executor").unwrap(),
        Store {
            value: key,
            reads: reads.clone(),
        },
    ));
    let gateway = caidex_model_gateway::start(
        vec![route(
            &fixture.endpoint(),
            Some(reference(owner)),
            vec![ResponsesDialect::Classic, ResponsesDialect::Lite],
        )],
        broker,
        limits,
    )
    .await
    .unwrap();
    (gateway, reads)
}
fn client() -> Client {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(WAIT)
        .build()
        .unwrap()
}
fn request(client: &Client, gateway: &RunningGateway, body: Value) -> reqwest::RequestBuilder {
    client
        .post(format!("http://{}/v1/responses", gateway.address()))
        .header(header::CONTENT_TYPE, "application/json")
        .bearer_auth(gateway.token().expose())
        .body(body.to_string())
}
fn classic() -> Value {
    json!({"model":"fixture", "input":[], "tools":[], "stream":true})
}
async fn wire(response: reqwest::Response) -> (StreamState, Vec<Value>) {
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.bytes().await.unwrap();
    let mut parser = ResponsesStream::new(2 * 1024 * 1024).unwrap();
    let mut events = Vec::new();
    for chunk in bytes.chunks(3) {
        events.extend(parser.push(chunk).unwrap());
    }
    (
        parser.finish().unwrap(),
        events
            .into_iter()
            .map(|event| event.response.wire().clone())
            .collect(),
    )
}
async fn error(response: reqwest::Response, status: StatusCode, code: &str) {
    assert_eq!(response.status(), status);
    let body: Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert_eq!(body["error"]["code"], code);
    assert!(!body.to_string().contains(KEY));
}

#[tokio::test]
async fn classic_and_lite_route_auth_and_opaque_stream_round_trip() {
    let opaque = json!({"type":"response.output_item.done", "sequence_number":2, "item":{"type":"reasoning", "encrypted_content":"opaque+/==", "signature":"unmodified", "provider_future":{"n":18446744073709551616_u128}}});
    let text = json!({"type":"response.output_text.delta", "delta":"中文🙂"});
    let bytes =
        format!("{CREATED}id: event-2\nretry: 123\ndata: {opaque}\n\ndata: {text}\n\n{DONE}")
            .into_bytes();
    let mut fixture = Fixture::start(Scenario::Reply {
        status: 200,
        content_type: "text/event-stream; charset=utf-8",
        extra: "Set-Cookie: private=value\r\nX-Api-Key: must-not-forward\r\n".into(),
        body: bytes,
        fragmented: true,
    })
    .await;
    let (gateway, reads) = gateway(&fixture, Limits::default(), "executor", Some(KEY)).await;
    let client = client();
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let mut body = classic();
        body["input"] = json!([{"type":"message", "role":"user", "content":[{"type":"input_image", "image_url":"data:image/png;base64,fixture"}]}, {"type":"function_call", "call_id":"call-1", "name":"fixture_tool", "arguments":" { \"x\": 1.00 } "}]);
        body["extension"] = json!({"reasoning_signature":"stay", "request_endpoint":"https://ignored.example", "large":18446744073709551616_u128});
        body["extension"]["decimal"] =
            serde_json::from_str("0.12345678901234567890123456789").unwrap();
        let mut outgoing = request(&client, &gateway, body.clone())
            .header("x-api-key", "client-header-must-not-forward")
            .header(header::COOKIE, "client-cookie");
        if dialect == ResponsesDialect::Lite {
            body.as_object_mut().unwrap().remove("tools");
            outgoing = request(&client, &gateway, body.clone())
                .header("x-openai-internal-codex-responses-lite", "true");
        }
        let response = outgoing.send().await.unwrap();
        assert!(!response.headers().contains_key(header::SET_COOKIE));
        assert!(!response.headers().contains_key("x-api-key"));
        let (state, events) = wire(response).await;
        assert_eq!(state, StreamState::Completed);
        assert!(events.contains(&opaque));
        assert!(events.contains(&text));
        let captured = fixture.request().await;
        body["model"] = "native-fixture".into();
        assert_eq!(captured.body, body);
        assert!(
            captured
                .headers
                .contains(&format!("authorization: Bearer {KEY}"))
        );
        assert!(!captured.headers.contains(gateway.token().expose()));
        assert!(!captured.headers.contains("client-header-must-not-forward"));
        assert!(!captured.headers.contains("client-cookie"));
        assert_eq!(
            captured
                .headers
                .contains("x-openai-internal-codex-responses-lite: true"),
            dialect == ResponsesDialect::Lite
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn nonstream_json_keeps_unknown_fields_and_tool_strings() {
    let response = json!({"id":"json-fixture", "status":"completed", "output":[{"type":"function_call", "name":"fixture", "call_id":"call-1", "arguments":" { \"n\": 1.00 } ", "provider_signature":"native"}], "usage":{"input_tokens":1, "provider_cost":"0.0001"}, "unknown":{"retain":true}});
    let mut fixture = Fixture::start(Scenario::Reply {
        status: 200,
        content_type: "application/json",
        extra: String::new(),
        body: response.to_string().into_bytes(),
        fragmented: true,
    })
    .await;
    let (gateway, _) = gateway(&fixture, Limits::default(), "executor", Some(KEY)).await;
    let mut body = classic();
    body["stream"] = false.into();
    let reply = request(&client(), &gateway, body).send().await.unwrap();
    assert_eq!(reply.status(), StatusCode::OK);
    let returned: Value = serde_json::from_slice(&reply.bytes().await.unwrap()).unwrap();
    assert_eq!(returned, response);
    assert!(
        fixture
            .request()
            .await
            .headers
            .contains("accept: application/json")
    );
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn ingress_auth_origin_routing_and_limits_reject_before_credential_resolution() {
    let fixture = Fixture::start(sse(DONE)).await;
    let (gateway, reads) = gateway(
        &fixture,
        Limits {
            request_bytes: 1024,
            ..Limits::default()
        },
        "executor",
        Some(KEY),
    )
    .await;
    let client = client();
    error(
        client
            .post(format!("http://{}/v1/responses", gateway.address()))
            .send()
            .await
            .unwrap(),
        StatusCode::UNAUTHORIZED,
        "gateway_unauthorized",
    )
    .await;
    for (builder, status, code) in [
        (
            request(&client, &gateway, classic()).header(header::AUTHORIZATION, "Bearer invalid"),
            StatusCode::UNAUTHORIZED,
            "gateway_unauthorized",
        ),
        (
            request(&client, &gateway, classic()).header(header::ORIGIN, "null"),
            StatusCode::FORBIDDEN,
            "gateway_request_forbidden",
        ),
        (
            request(&client, &gateway, classic()).header(header::CONTENT_ENCODING, "gzip"),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_content_type",
        ),
        (
            request(&client, &gateway, classic())
                .header("x-openai-internal-codex-responses-lite", "false"),
            StatusCode::BAD_REQUEST,
            "invalid_dialect",
        ),
        (
            request(&client, &gateway, classic())
                .header("x-openai-internal-codex-responses-lite", "true"),
            StatusCode::BAD_REQUEST,
            "invalid_model_request",
        ),
        (
            request(&client, &gateway, json!({"model":"unknown", "input":[]})),
            StatusCode::NOT_FOUND,
            "unknown_model",
        ),
        (
            request(&client, &gateway, json!({"model":"fixture"})),
            StatusCode::BAD_REQUEST,
            "invalid_model_request",
        ),
        (
            request(&client, &gateway, classic()).body("{"),
            StatusCode::BAD_REQUEST,
            "invalid_json",
        ),
        (
            request(&client, &gateway, classic()).body("x".repeat(1025)),
            StatusCode::PAYLOAD_TOO_LARGE,
            "invalid_or_oversized_body",
        ),
    ] {
        error(builder.send().await.unwrap(), status, code).await;
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_wrong_owner_and_invalid_header_credentials_never_reach_provider() {
    for (owner, key, code, expected_reads) in [
        ("other-host", Some(KEY), "credential_unavailable", 0),
        ("executor", None, "credential_missing", 1),
        (
            "executor",
            Some("fixture\r\nInjected: value"),
            "credential_invalid_header",
            1,
        ),
    ] {
        let fixture = Fixture::start(sse(DONE)).await;
        let (gateway, reads) = gateway(&fixture, Limits::default(), owner, key).await;
        error(
            request(&client(), &gateway, classic())
                .send()
                .await
                .unwrap(),
            StatusCode::SERVICE_UNAVAILABLE,
            code,
        )
        .await;
        assert_eq!(reads.load(Ordering::SeqCst), expected_reads);
        assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
        gateway.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn rate_limit_http_errors_and_redirects_are_safe_and_never_replayed() {
    for (status, expected_status, code, retry) in [
        (
            429,
            StatusCode::TOO_MANY_REQUESTS,
            "provider_rate_limited",
            true,
        ),
        (
            401,
            StatusCode::UNAUTHORIZED,
            "provider_authentication_failed",
            false,
        ),
        (
            503,
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_unavailable",
            false,
        ),
        (
            307,
            StatusCode::BAD_GATEWAY,
            "provider_redirect_blocked",
            false,
        ),
    ] {
        let fixture = Fixture::start(Scenario::Reply { status, content_type:"application/json", extra:"Retry-After: 5\r\nSet-Cookie: must-not-reflect\r\nLocation: http://127.0.0.1:1/unsafe\r\n".into(), body:format!("raw error echoed {KEY}").into_bytes(), fragmented:false }).await;
        let (gateway, _) = gateway(&fixture, Limits::default(), "executor", Some(KEY)).await;
        let response = request(&client(), &gateway, classic())
            .send()
            .await
            .unwrap();
        assert_eq!(response.headers().contains_key(header::RETRY_AFTER), retry);
        assert!(!response.headers().contains_key(header::SET_COOKIE));
        assert!(!response.headers().contains_key(header::LOCATION));
        error(response, expected_status, code).await;
        assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
        gateway.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn explicit_context_headers_round_trip_without_forwarding_auth_or_cookies() {
    let mut fixture = Fixture::start(Scenario::Reply {
        status: 200,
        content_type: "text/event-stream",
        extra: "X-Codex-Turn-State: provider-state+/==\r\nX-Request-Id: provider-request\r\nSet-Cookie: forbidden\r\nX-Arbitrary: forbidden\r\n".into(),
        body: DONE.as_bytes().to_vec(),
        fragmented: false,
    }).await;
    let (gateway, reads) = gateway(&fixture, Limits::default(), "executor", Some(KEY)).await;
    let response = request(&client(), &gateway, classic())
        .header("session_id", "session")
        .header("x-client-request-id", "client-request")
        .header("x-codex-turn-state", "client-state+/==")
        .header("x-codex-turn-metadata", "metadata")
        .header("cookie", "must-not-forward")
        .header("x-arbitrary", "must-not-forward")
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.headers()["x-codex-turn-state"],
        "provider-state+/=="
    );
    assert_eq!(response.headers()["x-request-id"], "provider-request");
    assert!(response.headers().get("set-cookie").is_none());
    assert!(response.headers().get("x-arbitrary").is_none());
    assert_eq!(wire(response).await.0, StreamState::Completed);
    let captured = fixture.request().await;
    for line in [
        "session_id: session",
        "x-client-request-id: client-request",
        "x-codex-turn-state: client-state+/==",
        "x-codex-turn-metadata: metadata",
    ] {
        assert!(captured.headers.contains(line));
    }
    assert!(
        captured
            .headers
            .contains(&format!("authorization: Bearer {KEY}"))
    );
    assert!(!captured.headers.contains(gateway.token().expose()));
    assert!(!captured.headers.contains("must-not-forward"));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    error(
        request(&client(), &gateway, classic())
            .header("x-codex-turn-state", "first")
            .header("x-codex-turn-state", "second")
            .send()
            .await
            .unwrap(),
        StatusCode::BAD_REQUEST,
        "duplicate_context_header",
    )
    .await;
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn http_date_retry_after_is_bounded_numeric_metadata_without_retry() {
    let mut fixture = Fixture::start(Scenario::Reply {
        status: 429,
        content_type: "application/json",
        extra: format!(
            "Retry-After: {}\r\n",
            httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_secs(60))
        ),
        body: format!("{{\"error\":\"{KEY}\"}}").into_bytes(),
        fragmented: false,
    })
    .await;
    let (gateway, _) = gateway(&fixture, Limits::default(), "executor", Some(KEY)).await;
    let response = request(&client(), &gateway, classic())
        .send()
        .await
        .unwrap();
    let hint = response.headers()[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert!((1..=60).contains(&hint));
    error(
        response,
        StatusCode::TOO_MANY_REQUESTS,
        "provider_rate_limited",
    )
    .await;
    fixture.request().await;
    fixture.disconnected().await;
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn malformed_truncated_oversized_and_mixed_streams_fail_without_success() {
    let cases = [
        (CREATED.as_bytes().to_vec(), "provider_stream_truncated"),
        (b"data: [DONE]\n\n".to_vec(), "provider_invalid_stream"),
        (b"data: \xff\n\n".to_vec(), "provider_invalid_stream"),
        (format!("data: {}\n\n", "x".repeat(1100)).into_bytes(), "provider_invalid_stream"),
        (format!("{CREATED}data: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"other\"}}}}\n\n").into_bytes(), "provider_invalid_stream"),
        (b"data: {\"type\":\"future\",\"sequence_number\":2}\n\ndata: {\"type\":\"future\",\"sequence_number\":1}\n\n".to_vec(), "provider_invalid_stream"),
    ];
    for (body, code) in cases {
        let fixture = Fixture::start(sse(body)).await;
        let (gateway, _) = gateway(
            &fixture,
            Limits {
                frame_bytes: 1024,
                ..Limits::default()
            },
            "executor",
            Some(KEY),
        )
        .await;
        let (state, events) = wire(
            request(&client(), &gateway, classic())
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(state, StreamState::Failed);
        assert_eq!(events.last().unwrap()["type"], "response.failed");
        assert_eq!(events.last().unwrap()["response"]["status"], "failed");
        assert_eq!(events.last().unwrap()["response"]["error"]["code"], code);
        assert!(
            !events
                .iter()
                .any(|event| event["type"] == "response.completed")
        );
        gateway.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn upstream_stream_error_diagnostics_are_redacted_without_modifying_opaque_items() {
    let opaque = json!({"type":"response.output_item.done", "item":{"type":"reasoning", "signature":"opaque+/==", "future":"retain"}});
    for terminal in [
        json!({"type":"error", "code":"fixture_error", "message":format!("echo {KEY}"), "param":null}),
        json!({"type":"error", "error":{"message":format!("echo {KEY}"), "authorization":"unknown-credential", "nested":{"api_key":"unknown"}}}),
        json!({"type":"response.failed", "response":{"id":"fixture", "error":{"message":format!("echo {KEY}")}}}),
    ] {
        let fixture = Fixture::start(sse(format!("data: {opaque}\n\ndata: {terminal}\n\n"))).await;
        let (gateway, _) = gateway(&fixture, Limits::default(), "executor", Some(KEY)).await;
        let (state, events) = wire(
            request(&client(), &gateway, classic())
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(state, StreamState::Failed);
        assert_eq!(events[0], opaque);
        assert!(!events.last().unwrap().to_string().contains(KEY));
        assert!(events.last().unwrap().to_string().contains("[REDACTED]"));
        gateway.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn incomplete_and_interrupted_terminals_are_not_promoted_to_completed() {
    for (reason, expected) in [
        ("max_output_tokens", StreamState::Incomplete),
        ("interrupted", StreamState::Interrupted),
    ] {
        let terminal = json!({"type":"response.incomplete", "response":{"id":"fixture", "status":"incomplete", "incomplete_details":{"reason":reason}, "future":"preserve"}});
        let fixture = Fixture::start(sse(format!("data: {terminal}\n\n"))).await;
        let (gateway, _) = gateway(&fixture, Limits::default(), "executor", Some(KEY)).await;
        let (state, events) = wire(
            request(&client(), &gateway, classic())
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(state, expected);
        assert_eq!(events, vec![terminal]);
        gateway.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn json_response_limits_and_invalid_content_type_fail_closed() {
    for (content_type, response, code) in [
        ("text/html", "unsafe response".into(), "provider_invalid_content_type"),
        ("application/json", "x".repeat(2049), "provider_response_too_large"),
        ("application/json", "{\"id\":\"fixture\",\"status\":\"in_progress\",\"output\":[]}".into(), "provider_invalid_response"),
        ("application/json", "{\"id\":\"fixture\",\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":-1}}".into(), "provider_invalid_response"),
    ] {
        let fixture = Fixture::start(Scenario::Reply { status:200, content_type, extra:String::new(), body:response.into_bytes(), fragmented:false }).await;
        let (gateway, _) = gateway(&fixture, Limits {response_bytes:2048, ..Limits::default()}, "executor", Some(KEY)).await;
        let mut body = classic(); body["stream"] = false.into();
        error(request(&client(), &gateway, body).send().await.unwrap(), StatusCode::BAD_GATEWAY, code).await;
        gateway.shutdown().await.unwrap();
    }
}

async fn raw_client(gateway: &RunningGateway) -> TcpStream {
    let mut stream = TcpStream::connect(gateway.address()).await.unwrap();
    let body = classic().to_string();
    stream.write_all(format!("POST /v1/responses HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", gateway.address(), gateway.token().expose(), body.len()).as_bytes()).await.unwrap();
    stream
}

#[tokio::test]
async fn disconnect_during_stream_closes_upstream_socket_and_releases_slot() {
    let mut fixture = Fixture::start(Scenario::StallBody).await;
    let (gateway, _) = gateway(
        &fixture,
        Limits {
            in_flight: 1,
            ..Limits::default()
        },
        "executor",
        Some(KEY),
    )
    .await;
    let client = client();
    let mut first = request(&client, &gateway, classic()).send().await.unwrap();
    assert!(first.chunk().await.unwrap().is_some());
    fixture.request().await;
    error(
        request(&client, &gateway, classic()).send().await.unwrap(),
        StatusCode::SERVICE_UNAVAILABLE,
        "gateway_busy",
    )
    .await;
    drop(first);
    fixture.disconnected().await;
    let mut next = request(&client, &gateway, classic()).send().await.unwrap();
    assert_eq!(next.status(), StatusCode::OK);
    assert!(next.chunk().await.unwrap().is_some());
    fixture.request().await;
    drop(next);
    fixture.disconnected().await;
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn disconnect_before_response_headers_also_cancels_upstream() {
    let mut fixture = Fixture::start(Scenario::StallHeaders).await;
    let (gateway, _) = gateway(&fixture, Limits::default(), "executor", Some(KEY)).await;
    let stream = raw_client(&gateway).await;
    fixture.request().await;
    drop(stream);
    fixture.disconnected().await;
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn header_idle_and_total_timeouts_close_real_socket_without_retry() {
    for scenario in [
        Scenario::StallHeaders,
        Scenario::StallBody,
        Scenario::Heartbeats,
    ] {
        let headers = matches!(scenario, Scenario::StallHeaders);
        let mut fixture = Fixture::start(scenario).await;
        let (gateway, _) = gateway(
            &fixture,
            Limits {
                header_timeout: Duration::from_millis(200),
                idle_timeout: Duration::from_millis(200),
                total_timeout: Duration::from_millis(500),
                ..Limits::default()
            },
            "executor",
            Some(KEY),
        )
        .await;
        let response = request(&client(), &gateway, classic())
            .send()
            .await
            .unwrap();
        if headers {
            error(response, StatusCode::GATEWAY_TIMEOUT, "provider_timeout").await;
        } else {
            let (state, events) = wire(response).await;
            assert_eq!(state, StreamState::Failed);
            assert_eq!(
                events.last().unwrap()["response"]["error"]["code"],
                "provider_timeout"
            );
        }
        fixture.disconnected().await;
        assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
        gateway.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn backpressure_bounds_upstream_reads_and_deadline_cancels_without_client_polling() {
    let mut fixture = Fixture::start(Scenario::Flood).await;
    let (gateway, _) = gateway(
        &fixture,
        Limits {
            total_timeout: Duration::from_millis(600),
            ..Limits::default()
        },
        "executor",
        Some(KEY),
    )
    .await;
    let stream = raw_client(&gateway).await; // Intentionally never read the response.
    fixture.request().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let produced = fixture.produced.load(Ordering::SeqCst);
    assert!(produced > 0);
    assert!(
        produced < 8192 * 8192,
        "gateway must not drain entire 64 MiB flood into memory"
    );
    fixture.disconnected().await; // The absolute deadline runs despite backpressure.
    assert!(fixture.produced.load(Ordering::SeqCst) < 8192 * 8192);
    drop(stream);
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn heartbeat_activity_reaches_client_without_creating_model_events_or_success() {
    let mut fixture = Fixture::start(Scenario::Heartbeats).await;
    let (gateway, _) = gateway(
        &fixture,
        Limits {
            total_timeout: Duration::from_secs(2),
            ..Limits::default()
        },
        "executor",
        Some(KEY),
    )
    .await;
    let mut response = request(&client(), &gateway, classic())
        .send()
        .await
        .unwrap();
    let first = response.chunk().await.unwrap().unwrap();
    assert!(first.starts_with(b": caidex keepalive"));
    let (state, events) = wire(response).await;
    assert_eq!(state, StreamState::Failed);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["response"]["error"]["code"], "provider_timeout");
    fixture.disconnected().await;
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_and_handle_drop_cancel_active_generations() {
    for explicit in [true, false] {
        let mut fixture = Fixture::start(Scenario::StallBody).await;
        let (gateway, _) = gateway(&fixture, Limits::default(), "executor", Some(KEY)).await;
        let address = gateway.address();
        let mut response = request(&client(), &gateway, classic())
            .send()
            .await
            .unwrap();
        assert!(response.chunk().await.unwrap().is_some());
        fixture.request().await;
        if explicit {
            gateway.shutdown().await.unwrap();
        } else {
            drop(gateway);
        }
        fixture.disconnected().await;
        assert!(TcpStream::connect(address).await.is_err());
        drop(response);
    }
}

#[tokio::test]
async fn unsafe_configuration_and_unconfigured_dialect_are_rejected() {
    for endpoint in [
        "http://example.com/responses",
        "http://localhost/responses",
        "ftp://127.0.0.1/responses",
        "https://user:secret@example.com/responses",
        "https://example.com/responses?api_key=secret",
        "https://example.com/responses#fragment",
    ] {
        let error = CustomResponses::new(endpoint, None).unwrap_err();
        assert_eq!(error.to_string(), "invalid provider endpoint");
        assert!(!format!("{error:?}").contains(endpoint));
    }
    assert!(CustomResponses::new("https://example.com/v1/responses", None).is_ok());
    assert!(CustomResponses::new("http://[::1]:11434/v1/responses", None).is_ok());
    let fixture = Fixture::start(sse(DONE)).await;
    let reads = Arc::new(AtomicUsize::new(0));
    let broker = Arc::new(Broker::new(
        Id::new("executor").unwrap(),
        Store {
            value: None,
            reads: reads.clone(),
        },
    ));
    let endpoint = fixture.endpoint();
    assert!(
        caidex_model_gateway::start(
            vec![
                route(&endpoint, None, vec![ResponsesDialect::Classic]),
                route(&endpoint, None, vec![ResponsesDialect::Classic])
            ],
            broker.clone(),
            Limits::default()
        )
        .await
        .is_err()
    );
    assert!(
        caidex_model_gateway::start(
            vec![route(&endpoint, None, vec![ResponsesDialect::Classic])],
            broker.clone(),
            Limits {
                in_flight: 0,
                ..Limits::default()
            }
        )
        .await
        .is_err()
    );
    let gateway = caidex_model_gateway::start(
        vec![route(&endpoint, None, vec![ResponsesDialect::Classic])],
        broker,
        Limits::default(),
    )
    .await
    .unwrap();
    let body = json!({"model":"fixture", "input":[], "stream":true});
    error(
        request(&client(), &gateway, body)
            .header("x-openai-internal-codex-responses-lite", "true")
            .send()
            .await
            .unwrap(),
        StatusCode::BAD_REQUEST,
        "unsupported_dialect",
    )
    .await;
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
    gateway.shutdown().await.unwrap();
}
