//! Synthetic credentials and actual loopback HTTP only.
use caidex_credentials::{Broker, CredentialRef, Id, Secret, SecretKind, SecretStore};
use caidex_model_core::{CancellationToken, ContextHeaders, REQUEST_HEADERS, RequestContext};
use caidex_provider_google::{
    CandidateOutcome, GeminiClient, GeminiConfig, Limits, NativeStreamEvent,
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
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
    task::JoinHandle,
};

const KEY: &str = "CAIDEX_SYNTHETIC_GOOGLE_KEY";
const WAIT: Duration = Duration::from_secs(10);
struct Store {
    reads: Arc<AtomicUsize>,
    key: Option<&'static str>,
}
impl SecretStore for Store {
    fn get(&self, _: &CredentialRef) -> caidex_credentials::Result<Option<Secret>> {
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
fn reference() -> CredentialRef {
    CredentialRef {
        owner: Id::new("executor").unwrap(),
        provider: Id::new("google").unwrap(),
        profile: Id::new("fixture").unwrap(),
        kind: SecretKind::ApiKey,
    }
}
fn client(
    base: &str,
    key: Option<&'static str>,
    limits: Limits,
) -> (GeminiClient<Store>, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let broker = Arc::new(Broker::new(
        Id::new("executor").unwrap(),
        Store {
            reads: reads.clone(),
            key,
        },
    ));
    let config = GeminiConfig::new(reference())
        .unwrap()
        .with_base_url(base)
        .unwrap();
    (GeminiClient::new(config, broker, limits).unwrap(), reads)
}
fn model(name: &str) -> Value {
    json!({"name":name,"baseModelId":"fixture","version":"1","displayName":"合成模型",
        "supportedGenerationMethods":["generateContent"],"inputTokenLimit":4096,"thinking":true})
}
struct Reply {
    status: u16,
    media: &'static str,
    headers: String,
    body: String,
    stall: u8,
}
impl Reply {
    fn sse(chunks: &[Value]) -> Self {
        Self {
            status: 200,
            media: "text/event-stream; charset=utf-8",
            headers: String::new(),
            body: chunks
                .iter()
                .map(|wire| format!("data: {wire}\n\n"))
                .collect(),
            stall: 0,
        }
    }
    fn json(wire: Value) -> Self {
        Self {
            status: 200,
            media: "application/json",
            headers: String::new(),
            body: wire.to_string(),
            stall: 0,
        }
    }
}
struct Fixture {
    base: String,
    requests: mpsc::UnboundedReceiver<String>,
    closed: mpsc::UnboundedReceiver<()>,
    task: JoinHandle<()>,
}
impl Fixture {
    async fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/proxy/v1beta", listener.local_addr().unwrap());
        let (tx, requests) = mpsc::unbounded_channel();
        let (closed_tx, closed) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            for reply in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 1024];
                    let n = socket.read(&mut buffer).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        let head = std::str::from_utf8(&bytes[..end]).unwrap();
                        let length = head
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                if bytes.is_empty() {
                    continue;
                }
                tx.send(String::from_utf8(bytes).unwrap()).unwrap();
                if reply.stall != 1 {
                    let head = format!(
                        "HTTP/1.1 {} Fixture\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n{}\r\n",
                        reply.status,
                        reply.media,
                        reply.body.len() + usize::from(matches!(reply.stall, 3 | 4)),
                        reply.headers
                    );
                    socket.write_all(head.as_bytes()).await.unwrap();
                    socket
                        .write_all(if reply.stall == 2 {
                            b"{"
                        } else {
                            reply.body.as_bytes()
                        })
                        .await
                        .unwrap();
                }
                if matches!(reply.stall, 1..=3) {
                    let mut byte = [0];
                    match socket.read(&mut byte).await {
                        Ok(0) | Err(_) => {
                            closed_tx.send(()).unwrap();
                        }
                        other => panic!("unexpected read {other:?}"),
                    }
                }
            }
        });
        Self {
            base,
            requests,
            closed,
            task,
        }
    }
    async fn request(&mut self) -> String {
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
async fn discovery_authenticates_native_pages_and_encodes_opaque_tokens_without_key_urls() {
    let cursor = "next+/=&? https://elsewhere.invalid/?key=not-a-key";
    let mut first = model("models/z");
    first["future"] = serde_json::from_str("{\"big\":18446744073709551616}").unwrap();
    let mut fixture = Fixture::start(vec![
        Reply::json(json!({"models":[first.clone()],"nextPageToken":cursor})),
        Reply::json(json!({"models":[model("models/a")]})),
    ])
    .await;
    let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let models = client
        .discover_models(3, RequestContext::default())
        .await
        .unwrap();
    assert_eq!(
        models.iter().map(|m| m.name()).collect::<Vec<_>>(),
        ["models/a", "models/z"]
    );
    assert_eq!(models[1].wire(), &first);
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    for expected_token in [None, Some(cursor)] {
        let head = fixture.request().await;
        let lower = head.to_ascii_lowercase();
        assert!(lower.contains("x-goog-api-key: caidex_synthetic_google_key\r\n"));
        assert!(!lower.contains("authorization:") && !lower.contains("x-api-key:"));
        let target = head.lines().next().unwrap();
        assert!(target.starts_with("GET /proxy/v1beta/models?"));
        assert!(!target.contains(KEY));
        let url = reqwest::Url::parse(&format!(
            "http://fixture{}",
            target.split_whitespace().nth(1).unwrap()
        ))
        .unwrap();
        let pairs = url.query_pairs().collect::<Vec<_>>();
        assert_eq!(
            pairs.iter().find(|(k, _)| k == "pageSize").unwrap().1,
            "1000"
        );
        assert_eq!(
            pairs
                .iter()
                .find(|(k, _)| k == "pageToken")
                .map(|(_, v)| v.as_ref()),
            expected_token
        );
        assert!(!pairs.iter().any(|(k, _)| k == "key"));
    }
}

#[test]
fn executor_profiles_reject_wrong_credentials_and_unsafe_endpoint_overrides() {
    for kind in [
        SecretKind::AccessToken,
        SecretKind::RefreshToken,
        SecretKind::ClientSecret,
    ] {
        let mut r = reference();
        r.kind = kind;
        assert!(GeminiConfig::new(r).is_err());
    }
    let mut r = reference();
    r.provider = Id::new("anthropic").unwrap();
    assert!(GeminiConfig::new(r).is_err());
    for base in [
        "http://example.com/v1beta",
        "http://localhost/v1beta",
        "https://user:key@example.com/",
        "https://example.com/?key=bad",
        "https://example.com/#fragment",
        "file:///tmp",
    ] {
        assert!(
            GeminiConfig::new(reference())
                .unwrap()
                .with_base_url(base)
                .is_err(),
            "{base}"
        );
    }
    let config = GeminiConfig::new(reference())
        .unwrap()
        .with_base_url("https://example.com/proxy/v1beta")
        .unwrap();
    assert!(!format!("{config:?}").contains("example.com"));
}

#[tokio::test]
async fn invalid_or_cancelled_context_and_missing_credentials_do_not_send_requests() {
    let mut fixture = Fixture::start(vec![Reply::json(json!({}))]).await;
    let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        client
            .discover_models(
                1,
                RequestContext {
                    cancellation: cancel,
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
        client
            .discover_models(
                1,
                RequestContext {
                    deadline: Some(std::time::Instant::now() - Duration::from_secs(1)),
                    ..Default::default()
                }
            )
            .await
            .err()
            .unwrap()
            .code,
        "provider_timeout"
    );
    assert_eq!(
        client
            .discover_models(0, RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "invalid_catalog_limit"
    );
    let mut headers = ContextHeaders::default();
    headers
        .insert("session_id", "private".into(), REQUEST_HEADERS)
        .unwrap();
    assert_eq!(
        client
            .discover_models(
                1,
                RequestContext {
                    headers,
                    ..Default::default()
                }
            )
            .await
            .err()
            .unwrap()
            .code,
        "unsupported_native_context_header"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let (missing, missing_reads) = self::client(&fixture.base, None, Limits::default());
    assert_eq!(
        missing
            .discover_models(1, RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "credential_missing"
    );
    assert_eq!(missing_reads.load(Ordering::SeqCst), 1);
    let (invalid, invalid_reads) =
        self::client(&fixture.base, Some("SYNTHETIC\nKEY"), Limits::default());
    assert_eq!(
        invalid
            .discover_models(1, RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "credential_invalid_header"
    );
    assert_eq!(invalid_reads.load(Ordering::SeqCst), 1);
    let owner_reads = Arc::new(AtomicUsize::new(0));
    let broker = Arc::new(Broker::new(
        Id::new("other-host").unwrap(),
        Store {
            reads: owner_reads.clone(),
            key: Some(KEY),
        },
    ));
    let wrong_owner = GeminiClient::new(
        GeminiConfig::new(reference())
            .unwrap()
            .with_base_url(&fixture.base)
            .unwrap(),
        broker,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        wrong_owner
            .discover_models(1, RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "credential_unavailable"
    );
    assert_eq!(owner_reads.load(Ordering::SeqCst), 0);
    assert!(fixture.requests.try_recv().is_err());
}

#[tokio::test]
async fn status_content_type_json_and_paging_failures_are_safe_and_never_retried() {
    for (status, code) in [
        (401, "provider_authentication_failed"),
        (403, "provider_authentication_failed"),
        (429, "provider_rate_limited"),
        (400, "provider_request_rejected"),
        (302, "provider_redirect_blocked"),
        (503, "provider_unavailable"),
    ] {
        let mut fixture = Fixture::start(vec![Reply {
            status,
            media: "application/json",
            headers: "retry-after: 7\r\nlocation: https://example.invalid/\r\n".into(),
            body: KEY.into(),
            stall: 0,
        }])
        .await;
        let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
        let error = client
            .discover_models(2, RequestContext::default())
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, code);
        assert!(!format!("{error:?}").contains(KEY));
        assert_eq!(error.http_status, if status == 302 { 502 } else { status });
        assert_eq!(
            error.retry_after_seconds,
            if status == 429 { Some(7) } else { None }
        );
        fixture.request().await;
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert!(fixture.requests.try_recv().is_err());
    }
    for (media, body, code) in [
        ("text/html", "{}", "provider_invalid_content_type"),
        ("application/json", "not JSON", "provider_invalid_json"),
    ] {
        let fixture = Fixture::start(vec![Reply {
            status: 200,
            media,
            body: body.into(),
            headers: String::new(),
            stall: 0,
        }])
        .await;
        let (client, _) = client(&fixture.base, Some(KEY), Limits::default());
        assert_eq!(
            client
                .discover_models(1, RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            code
        );
    }
    for second in [
        json!({"models":[model("models/a")]}),
        json!({"nextPageToken":"same"}),
    ] {
        let fixture = Fixture::start(vec![
            Reply::json(json!({"models":[model("models/a")],"nextPageToken":"same"})),
            Reply::json(second),
        ])
        .await;
        let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
        assert_eq!(
            client
                .discover_models(2, RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            "google_invalid_model_catalog"
        );
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn aggregate_response_and_encoded_url_budgets_cover_the_whole_catalog() {
    let first = json!({"models":[model("models/a")],"nextPageToken":"next"});
    let fixture = Fixture::start(vec![
        Reply::json(first.clone()),
        Reply::json(json!({"models":[model("models/b")]})),
    ])
    .await;
    let limits = Limits {
        response_bytes: first.to_string().len() + 10,
        ..Default::default()
    };
    let (client, reads) = client(&fixture.base, Some(KEY), limits);
    assert_eq!(
        client
            .discover_models(2, RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "provider_oversized_response"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    let fixture = Fixture::start(vec![Reply::json(json!({"nextPageToken":"+".repeat(200)}))]).await;
    let (client, reads) = self::client(
        &fixture.base,
        Some(KEY),
        Limits {
            request_bytes: 256,
            ..Default::default()
        },
    );
    assert_eq!(
        client
            .discover_models(2, RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "invalid_or_oversized_body"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancellation_closes_socket_holds_catalog_permit_and_releases_it_without_retry() {
    let mut stalled = Reply::json(json!({}));
    stalled.stall = 1;
    let mut fixture = Fixture::start(vec![stalled, Reply::json(json!({}))]).await;
    let (client, reads) = client(
        &fixture.base,
        Some(KEY),
        Limits {
            in_flight: 1,
            ..Default::default()
        },
    );
    let client = Arc::new(client);
    let cancellation = CancellationToken::new();
    let work = tokio::spawn({
        let client = client.clone();
        let cancellation = cancellation.clone();
        async move {
            client
                .discover_models(
                    2,
                    RequestContext {
                        cancellation,
                        ..Default::default()
                    },
                )
                .await
        }
    });
    fixture.request().await;
    assert_eq!(
        client
            .discover_models(2, RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "provider_busy"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    cancellation.cancel();
    assert_eq!(
        work.await.unwrap().err().unwrap().code,
        "provider_cancelled"
    );
    fixture.disconnected().await;
    assert!(
        client
            .discover_models(2, RequestContext::default())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn header_body_and_caller_deadlines_close_incomplete_io() {
    for (stage, caller) in [(1, false), (2, false), (2, true)] {
        let mut reply = Reply::json(json!({"models":[]}));
        reply.stall = stage;
        let mut fixture = Fixture::start(vec![reply]).await;
        let (client, reads) = client(
            &fixture.base,
            Some(KEY),
            Limits {
                header_timeout: Duration::from_millis(300),
                idle_timeout: Duration::from_millis(300),
                ..Default::default()
            },
        );
        let context = if caller {
            RequestContext {
                deadline: Some(std::time::Instant::now() + Duration::from_millis(100)),
                ..Default::default()
            }
        } else {
            RequestContext::default()
        };
        assert_eq!(
            client.discover_models(2, context).await.err().unwrap().code,
            "provider_timeout"
        );
        fixture.disconnected().await;
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn cancelled_blocking_credential_read_never_sends_a_late_native_request() {
    struct BlockedStore {
        entered: mpsc::UnboundedSender<()>,
        finished: mpsc::UnboundedSender<()>,
        release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl SecretStore for BlockedStore {
        fn get(&self, _: &CredentialRef) -> caidex_credentials::Result<Option<Secret>> {
            self.entered.send(()).unwrap();
            self.release.lock().unwrap().recv_timeout(WAIT).unwrap();
            self.finished.send(()).unwrap();
            Ok(Some(Secret::new(KEY.into())?))
        }
        fn set(&self, _: &CredentialRef, _: &Secret) -> caidex_credentials::Result<()> {
            unreachable!()
        }
        fn remove(&self, _: &CredentialRef) -> caidex_credentials::Result<bool> {
            unreachable!()
        }
    }
    let mut fixture = Fixture::start(vec![Reply::json(json!({}))]).await;
    let (entered_tx, mut entered) = mpsc::unbounded_channel();
    let (finished_tx, mut finished) = mpsc::unbounded_channel();
    let (release, receiver) = std::sync::mpsc::channel();
    let broker = Arc::new(Broker::new(
        Id::new("executor").unwrap(),
        BlockedStore {
            entered: entered_tx,
            finished: finished_tx,
            release: std::sync::Mutex::new(receiver),
        },
    ));
    let config = GeminiConfig::new(reference())
        .unwrap()
        .with_base_url(&fixture.base)
        .unwrap();
    let client = GeminiClient::new(config, broker, Limits::default()).unwrap();
    let cancellation = CancellationToken::new();
    let work = tokio::spawn({
        let cancellation = cancellation.clone();
        async move {
            client
                .discover_models(
                    1,
                    RequestContext {
                        cancellation,
                        ..Default::default()
                    },
                )
                .await
        }
    });
    tokio::time::timeout(WAIT, entered.recv())
        .await
        .unwrap()
        .unwrap();
    cancellation.cancel();
    assert_eq!(
        work.await.unwrap().err().unwrap().code,
        "provider_cancelled"
    );
    release.send(()).unwrap();
    tokio::time::timeout(WAIT, finished.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(fixture.requests.try_recv().is_err());
}

#[tokio::test]
async fn native_api_key_only_crosses_a_trusted_valid_hostname_tls_connection() {
    use caidex_provider_google::ClientOptions;
    use rcgen::{
        BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose,
        IsCa, KeyPair, KeyUsagePurpose,
    };
    use rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer};
    use tokio_rustls::TlsAcceptor;
    for (host, expired, trust, success) in [
        ("127.0.0.1", false, true, true),
        ("127.0.0.1", false, false, false),
        ("wrong.invalid", false, true, false),
        ("127.0.0.1", true, true, false),
    ] {
        let now = time::OffsetDateTime::now_utc();
        let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca.distinguished_name
            .push(DnType::CommonName, "CAIdex Google fixture CA");
        ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        ca.not_before = now - time::Duration::days(3);
        ca.not_after = now + time::Duration::days(3);
        let issuer = CertifiedIssuer::self_signed(ca, KeyPair::generate().unwrap()).unwrap();
        let mut leaf = CertificateParams::new(vec![host.into()]).unwrap();
        leaf.distinguished_name.push(DnType::CommonName, host);
        leaf.use_authority_key_identifier_extension = true;
        leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        leaf.not_before = now - time::Duration::days(2);
        leaf.not_after = now + time::Duration::days(if expired { -1 } else { 1 });
        let key = KeyPair::generate().unwrap();
        let cert = leaf.signed_by(&key, &issuer).unwrap();
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("https://{}/v1beta", listener.local_addr().unwrap());
        let (tx, mut requests) = mpsc::unbounded_channel();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            if let Ok(mut socket) = TlsAcceptor::from(Arc::new(config)).accept(socket).await {
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 1024];
                    let n = socket.read(&mut buffer).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..n]);
                    if bytes.windows(4).any(|v| v == b"\r\n\r\n") {
                        break;
                    }
                }
                if !bytes.is_empty() {
                    tx.send(String::from_utf8(bytes).unwrap()).unwrap();
                    socket.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}").await.unwrap();
                }
            }
        });
        let reads = Arc::new(AtomicUsize::new(0));
        let broker = Arc::new(Broker::new(
            Id::new("executor").unwrap(),
            Store {
                reads: reads.clone(),
                key: Some(KEY),
            },
        ));
        let config = GeminiConfig::new(reference())
            .unwrap()
            .with_base_url(&base)
            .unwrap();
        let options = ClientOptions {
            root_certificates: if trust {
                vec![reqwest::Certificate::from_der(issuer.der()).unwrap()]
            } else {
                vec![]
            },
        };
        let client =
            GeminiClient::with_options(config, broker, Limits::default(), options).unwrap();
        let result = client.discover_models(1, RequestContext::default()).await;
        tokio::time::timeout(WAIT, server).await.unwrap().unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        if success {
            assert!(result.unwrap().is_empty());
            assert!(
                requests
                    .recv()
                    .await
                    .unwrap()
                    .to_ascii_lowercase()
                    .contains("x-goog-api-key: caidex_synthetic_google_key")
            );
        } else {
            assert_eq!(result.err().unwrap().code, "provider_transport_error");
            assert!(
                requests.try_recv().is_err(),
                "a failed TLS handshake must not carry an API key request"
            );
        }
    }
}

fn native_reply(reason: &str) -> Value {
    json!({"responseId":"native-fixture","modelVersion":"fixture-001",
        "candidates":[{"content":{"role":"model","parts":[{"text":"回复"}]},"finishReason":reason}],
        "usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2,"thoughtsTokenCount":4,"totalTokenCount":9}})
}
fn native_input() -> Value {
    json!({"contents":[{"role":"user","parts":[{"text":"你好"}]}],
        "generationConfig":{"maxOutputTokens":128,"future":true}})
}
fn request_body(request: &str) -> Value {
    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
}

#[tokio::test]
async fn native_generation_posts_exact_wire_and_replays_signed_tool_parts_without_executing() {
    let mut first = native_reply("STOP");
    first["candidates"][0]["content"]["parts"] = json!([
        {"text":"原生思考","thought":true,"thoughtSignature":"c2ln"},
        {"functionCall":{"name":"echo","id":"native-call","args":{"text":"参数原文"}},"thoughtSignature":"c2lnbmVk"},
        {"futurePart":{"future":true}}
    ]);
    first["future"] = serde_json::from_str("{\"number\":18446744073709551616}").unwrap();
    let mut fixture = Fixture::start(vec![
        Reply::json(first.clone()),
        Reply::json(native_reply("STOP")),
    ])
    .await;
    let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
    let mut input = native_input();
    input["systemInstruction"] = json!({"parts":[{"text":"固定指令"}]});
    input["tools"] =
        json!([{"functionDeclarations":[{"name":"echo","parameters":{"type":"object"}}]}]);
    input["contents"][0]["parts"]
        .as_array_mut()
        .unwrap()
        .push(json!({"inlineData":{"mimeType":"image/png","data":"AA=="}}));
    input["future"] = first["future"].clone();
    let response = client
        .generate_content(
            "models/fixture-001",
            input.clone(),
            RequestContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(response.wire(), &first);
    assert_eq!(
        response.wire()["future"]["number"].to_string(),
        "18446744073709551616"
    );
    assert_eq!(
        response.outcome(0),
        Some(caidex_provider_google::CandidateOutcome::ToolCall)
    );
    let first_request = fixture.request().await;
    assert!(
        first_request
            .starts_with("POST /proxy/v1beta/models/fixture-001:generateContent HTTP/1.1\r\n")
    );
    assert_eq!(request_body(&first_request), input);
    assert!(
        first_request
            .to_ascii_lowercase()
            .contains("x-goog-api-key: caidex_synthetic_google_key\r\n")
    );
    assert!(
        !first_request
            .to_ascii_lowercase()
            .contains("authorization:")
    );
    let second = json!({"contents":[input["contents"][0],response.candidates()[0]["content"],
        {"role":"user","parts":[{"functionResponse":{"name":"echo","id":"native-call","response":{"result":"工具结果原文"}}}]}]});
    client
        .generate_content(
            "models/fixture-001",
            second.clone(),
            RequestContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(request_body(&fixture.request().await), second);
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn native_generation_rejects_bad_routes_bodies_and_budgets_before_authentication() {
    let mut fixture = Fixture::start(vec![Reply::json(native_reply("STOP"))]).await;
    let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
    for name in [
        "fixture",
        "models/../other",
        "models/a/b",
        "models/a?key=x",
        "models/a%2Fb",
        "https://example.invalid/",
    ] {
        assert_eq!(
            client
                .generate_content(name, native_input(), RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            "google_invalid_request"
        );
    }
    for body in [
        json!([]),
        json!({}),
        json!({"contents":[]}),
        json!({"contents":[{"parts":[]}]}),
        json!({"contents":[{"role":"system","parts":[{"text":"x"}]}]}),
        json!({"model":"models/other","contents":[{"parts":[{"text":"x"}]}]}),
        json!({"contents":[{"parts":[{"functionCall":{"name":"echo","args":"{}"}}]}]}),
    ] {
        assert_eq!(
            client
                .generate_content("models/fixture", body, RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            "google_invalid_request"
        );
    }
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        client
            .generate_content(
                "models/fixture",
                native_input(),
                RequestContext {
                    cancellation: cancel,
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
    let (small, small_reads) = self::client(
        &fixture.base,
        Some(KEY),
        Limits {
            request_bytes: 128,
            ..Default::default()
        },
    );
    let huge = json!({"contents":[{"parts":[{"text":"x".repeat(256)}]}]});
    assert_eq!(
        small
            .generate_content("models/fixture", huge, RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "invalid_or_oversized_body"
    );
    assert_eq!(small_reads.load(Ordering::SeqCst), 0);
    assert!(fixture.requests.try_recv().is_err());
}

#[tokio::test]
async fn native_generation_classifies_errors_and_rejects_incomplete_json_without_retry() {
    for (status, wire, code) in [
        (
            429,
            json!({"error":{"message":KEY}}),
            "provider_rate_limited",
        ),
        (
            302,
            json!({"error":{"message":KEY}}),
            "provider_redirect_blocked",
        ),
        (
            200,
            json!({"error":{"message":KEY}}),
            "google_invalid_response",
        ),
        (
            200,
            json!({"candidates":[{"content":{"parts":[{"text":"partial"}]}}]}),
            "google_invalid_response",
        ),
    ] {
        let mut reply = Reply::json(wire);
        reply.status = status;
        reply.headers = "retry-after: 5\r\nlocation: https://example.invalid/\r\n".into();
        let mut fixture = Fixture::start(vec![reply]).await;
        let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
        let error = client
            .generate_content("models/fixture", native_input(), RequestContext::default())
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, code);
        assert!(!format!("{error:?}").contains(KEY));
        if status == 429 {
            assert_eq!(error.retry_after_seconds, Some(5));
        }
        assert!(fixture.request().await.starts_with("POST "));
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
    let fixture = Fixture::start(vec![Reply::json(native_reply("MAX_TOKENS"))]).await;
    let (client, _) = client(&fixture.base, Some(KEY), Limits::default());
    assert_eq!(
        client
            .generate_content("models/fixture", native_input(), RequestContext::default())
            .await
            .unwrap()
            .outcome(0),
        Some(caidex_provider_google::CandidateOutcome::MaxTokens)
    );
}

#[tokio::test]
async fn native_generation_cancel_closes_post_and_releases_shared_discovery_permit() {
    let mut stalled = Reply::json(native_reply("STOP"));
    stalled.stall = 2;
    let mut fixture = Fixture::start(vec![stalled, Reply::json(json!({}))]).await;
    let (client, reads) = client(
        &fixture.base,
        Some(KEY),
        Limits {
            in_flight: 1,
            ..Default::default()
        },
    );
    let client = Arc::new(client);
    let cancellation = CancellationToken::new();
    let work = tokio::spawn({
        let client = client.clone();
        let cancellation = cancellation.clone();
        async move {
            client
                .generate_content(
                    "models/fixture",
                    native_input(),
                    RequestContext {
                        cancellation,
                        ..Default::default()
                    },
                )
                .await
        }
    });
    assert!(fixture.request().await.starts_with("POST "));
    assert_eq!(
        client
            .discover_models(1, RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "provider_busy"
    );
    cancellation.cancel();
    assert_eq!(
        work.await.unwrap().err().unwrap().code,
        "provider_cancelled"
    );
    fixture.disconnected().await;
    assert!(
        client
            .discover_models(1, RequestContext::default())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn native_stream_posts_exact_wire_preserves_chunks_and_finishes_after_http_end() {
    let parts = json!([
        {"text":"思考","thought":true,"thoughtSignature":"c2ln"},
        {"functionCall":{"name":"echo","id":"native-call","args":{"text":"原文"}},"thoughtSignature":"c2lnMg=="},
        {"futurePart":{"opaque":true}}
    ]);
    let chunks = vec![
        json!({"responseId":"stream-fixture","modelVersion":"fixture-001","candidates":[
            {"index":1,"content":{"role":"model","parts":[{"text":"第二个"}]}},
            {"index":0,"content":{"role":"model","parts":parts}}]}),
        json!({"candidates":[{"index":0,"finishReason":"STOP"},{"index":1,"finishReason":"MAX_TOKENS"}]}),
        serde_json::from_str(r#"{"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2,"totalTokenCount":5},"future":{"big":18446744073709551616}}"#).unwrap(),
    ];
    let mut fixture = Fixture::start(vec![Reply::sse(&chunks)]).await;
    let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
    let mut input = native_input();
    input["generationConfig"]["candidateCount"] = json!(2);
    input["systemInstruction"] = json!({"parts":[{"text":"固定指令"}]});
    input["tools"] =
        json!([{"functionDeclarations":[{"name":"echo","parameters":{"type":"object"}}]}]);
    input["contents"][0]["parts"]
        .as_array_mut()
        .unwrap()
        .push(json!({"inlineData":{"mimeType":"image/png","data":"AA=="}}));
    input["future"] = chunks[2]["future"].clone();
    let mut stream = client
        .stream_content(
            "models/fixture-001",
            input.clone(),
            RequestContext::default(),
        )
        .await
        .unwrap();
    let request = fixture.request().await;
    assert!(request.starts_with(
        "POST /proxy/v1beta/models/fixture-001:streamGenerateContent?alt=sse HTTP/1.1\r\n"
    ));
    let head = request
        .split_once("\r\n\r\n")
        .unwrap()
        .0
        .to_ascii_lowercase();
    assert!(head.contains("x-goog-api-key: caidex_synthetic_google_key\r\n"));
    assert!(
        head.contains("accept: text/event-stream")
            && head.contains("content-type: application/json")
    );
    assert!(!head.contains("authorization:") && !request.lines().next().unwrap().contains(KEY));
    assert_eq!(request_body(&request), input);
    for chunk in &chunks {
        let NativeStreamEvent::Event(event) = stream.next().await.unwrap().unwrap() else {
            panic!("premature completion")
        };
        assert_eq!(event.wire(), chunk);
        assert!(!format!("{event:?}").contains("原文"));
    }
    let NativeStreamEvent::Completed(complete) = stream.next().await.unwrap().unwrap() else {
        panic!("missing completion")
    };
    assert_eq!(complete.chunks(), chunks);
    assert_eq!(
        complete.response().candidates()[0]["content"]["parts"],
        parts
    );
    assert_eq!(
        complete.response().outcome(0),
        Some(CandidateOutcome::ToolCall)
    );
    assert_eq!(
        complete.response().outcome(1),
        Some(CandidateOutcome::MaxTokens)
    );
    assert_eq!(
        complete.response().wire()["usageMetadata"]["totalTokenCount"],
        5
    );
    assert_eq!(
        complete.response().wire()["future"]["big"].to_string(),
        "18446744073709551616"
    );
    assert!(stream.next().await.is_none());
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn native_stream_rejects_invalid_input_context_counts_and_budgets_before_auth() {
    let mut fixture = Fixture::start(vec![Reply::sse(&[native_reply("STOP")])]).await;
    let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
    for (name, body) in [
        ("models/../other", native_input()),
        ("models/a?key=x", native_input()),
        ("models/fixture", json!({"contents":[]})),
        (
            "models/fixture",
            json!({"model":null,"contents":[{"parts":[{"text":"x"}]}]}),
        ),
        (
            "models/fixture",
            json!({"contents":[{"parts":[{"functionCall":{"name":"echo","args":"{}"}}]}]}),
        ),
    ] {
        assert_eq!(
            client
                .stream_content(name, body, RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            "google_invalid_request"
        );
    }
    for config in [
        json!([]),
        json!(false),
        json!({"candidateCount":0}),
        json!({"candidateCount":-1}),
        json!({"candidateCount":1.5}),
        json!({"candidateCount":"2"}),
    ] {
        let mut input = native_input();
        input["generationConfig"] = config;
        assert_eq!(
            client
                .stream_content("models/fixture", input, RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            "google_invalid_request"
        );
    }
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let mut headers = ContextHeaders::default();
    headers
        .insert("session_id", "fixture".into(), REQUEST_HEADERS)
        .unwrap();
    for (context, code) in [
        (
            RequestContext {
                cancellation,
                ..Default::default()
            },
            "provider_cancelled",
        ),
        (
            RequestContext {
                deadline: Some(std::time::Instant::now() - Duration::from_secs(1)),
                ..Default::default()
            },
            "provider_timeout",
        ),
        (
            RequestContext {
                headers,
                ..Default::default()
            },
            "unsupported_native_context_header",
        ),
    ] {
        assert_eq!(
            client
                .stream_content("models/fixture", native_input(), context)
                .await
                .err()
                .unwrap()
                .code,
            code
        );
    }
    let (small, small_reads) = self::client(
        &fixture.base,
        Some(KEY),
        Limits {
            request_bytes: 128,
            ..Default::default()
        },
    );
    for (name, input) in [
        (
            "models/fixture".to_owned(),
            json!({"contents":[{"parts":[{"text":"x".repeat(256)}]}]}),
        ),
        (format!("models/{}", "a".repeat(256)), native_input()),
    ] {
        assert_eq!(
            small
                .stream_content(&name, input, RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            "invalid_or_oversized_body"
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(small_reads.load(Ordering::SeqCst), 0);
    assert!(fixture.requests.try_recv().is_err());
}

#[tokio::test]
async fn native_stream_safe_errors_truncation_media_and_limits_never_retry_or_complete() {
    for (status, media, code, error_status) in [
        (429, "text/event-stream", "provider_rate_limited", 429),
        (302, "text/event-stream", "provider_redirect_blocked", 502),
        (
            200,
            "application/json",
            "provider_invalid_content_type",
            502,
        ),
    ] {
        let mut reply = Reply::sse(&[json!({"error":{"message":KEY}})]);
        reply.status = status;
        reply.media = media;
        reply.headers = "retry-after: 5\r\nlocation: https://example.invalid/\r\n".into();
        let mut fixture = Fixture::start(vec![reply]).await;
        let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
        let error = client
            .stream_content("models/fixture", native_input(), RequestContext::default())
            .await
            .err()
            .unwrap();
        assert_eq!((error.code, error.http_status), (code, error_status));
        assert!(!format!("{error:?}").contains(KEY));
        if status == 429 {
            assert_eq!(error.retry_after_seconds, Some(5));
        }
        fixture.request().await;
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert!(fixture.requests.try_recv().is_err());
    }
    let stop = Reply::sse(&[native_reply("STOP")]).body;
    for (body, stall, count, limits, code) in [
        (
            Reply::sse(&[json!({"error":{"code":429,"message":KEY}})]).body,
            0,
            1,
            Limits::default(),
            "provider_rate_limited",
        ),
        (
            format!("data: {KEY}\n\n"),
            0,
            1,
            Limits::default(),
            "google_invalid_stream",
        ),
        (
            "data: [DONE]\n\n".into(),
            0,
            1,
            Limits::default(),
            "google_invalid_stream",
        ),
        (
            Reply::sse(&[native_reply("")]).body,
            0,
            1,
            Limits::default(),
            "google_stream_truncated",
        ),
        (
            stop.clone(),
            0,
            2,
            Limits::default(),
            "google_stream_truncated",
        ),
        (
            format!("{stop}data: {{"),
            0,
            1,
            Limits::default(),
            "google_stream_truncated",
        ),
        (
            stop.clone(),
            4,
            1,
            Limits::default(),
            "provider_transport_error",
        ),
        (
            stop.clone(),
            0,
            1,
            Limits {
                frame_bytes: 16,
                ..Default::default()
            },
            "google_invalid_stream",
        ),
        (
            stop,
            0,
            1,
            Limits {
                response_bytes: 16,
                ..Default::default()
            },
            "google_stream_too_large",
        ),
    ] {
        let mut reply = Reply::sse(&[]);
        reply.body = body;
        reply.stall = stall;
        let mut fixture = Fixture::start(vec![reply]).await;
        let (client, reads) = client(&fixture.base, Some(KEY), limits);
        let mut input = native_input();
        input["generationConfig"]["candidateCount"] = json!(count);
        let mut stream = client
            .stream_content("models/fixture", input, RequestContext::default())
            .await
            .unwrap();
        loop {
            match tokio::time::timeout(WAIT, stream.next())
                .await
                .unwrap()
                .unwrap()
            {
                Ok(NativeStreamEvent::Event(_)) => (),
                Ok(NativeStreamEvent::Completed(_)) => {
                    panic!("failed HTTP/native stream completed")
                }
                Err(error) => {
                    assert_eq!(error.code, code);
                    assert!(!format!("{error:?}").contains(KEY));
                    break;
                }
            }
        }
        assert!(stream.next().await.is_none());
        fixture.request().await;
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert!(fixture.requests.try_recv().is_err());
    }
}

#[tokio::test]
async fn native_stream_drop_and_cancel_close_socket_and_release_shared_permit() {
    for cancel in [false, true] {
        let mut open = native_reply("");
        open["candidates"][0]
            .as_object_mut()
            .unwrap()
            .remove("finishReason");
        let mut reply = Reply::sse(&[open]);
        reply.stall = 3;
        let mut fixture = Fixture::start(vec![reply, Reply::json(json!({}))]).await;
        let (client, reads) = client(
            &fixture.base,
            Some(KEY),
            Limits {
                in_flight: 1,
                ..Default::default()
            },
        );
        let cancellation = CancellationToken::new();
        let mut stream = client
            .stream_content(
                "models/fixture",
                native_input(),
                RequestContext {
                    cancellation: cancellation.clone(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        fixture.request().await;
        assert!(matches!(
            stream.next().await.unwrap().unwrap(),
            NativeStreamEvent::Event(_)
        ));
        assert_eq!(
            client
                .discover_models(1, RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            "provider_busy"
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        if cancel {
            cancellation.cancel();
            assert_eq!(
                stream.next().await.unwrap().err().unwrap().code,
                "provider_cancelled"
            );
            assert!(stream.next().await.is_none());
        }
        drop(stream);
        fixture.disconnected().await;
        assert!(
            client
                .discover_models(1, RequestContext::default())
                .await
                .unwrap()
                .is_empty()
        );
        assert!(fixture.request().await.starts_with("GET "));
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn unconsumed_native_stream_deadlines_close_socket_keep_error_and_release_slot() {
    for caller in [false, true] {
        let mut open = native_reply("");
        open["candidates"][0]
            .as_object_mut()
            .unwrap()
            .remove("finishReason");
        let mut reply = Reply::sse(&[open.clone(), open.clone(), open]);
        reply.stall = 3;
        let mut fixture = Fixture::start(vec![reply, Reply::json(json!({}))]).await;
        let (client, reads) = client(
            &fixture.base,
            Some(KEY),
            Limits {
                in_flight: 1,
                total_timeout: Duration::from_secs(if caller { 15 } else { 3 }),
                header_timeout: Duration::from_secs(15),
                idle_timeout: Duration::from_secs(15),
                ..Default::default()
            },
        );
        let context = RequestContext {
            deadline: caller.then(|| std::time::Instant::now() + Duration::from_secs(3)),
            ..Default::default()
        };
        let mut stream = client
            .stream_content("models/fixture", native_input(), context)
            .await
            .unwrap();
        fixture.request().await;
        assert_eq!(
            client
                .discover_models(1, RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            "provider_busy"
        );
        fixture.disconnected().await; // No receiver polls: a full slot cannot hide the deadline.
        assert!(
            client
                .discover_models(1, RequestContext::default())
                .await
                .unwrap()
                .is_empty()
        );
        fixture.request().await;
        assert!(matches!(
            stream.next().await.unwrap().unwrap(),
            NativeStreamEvent::Event(_)
        ));
        assert_eq!(
            stream.next().await.unwrap().err().unwrap().code,
            "provider_timeout"
        );
        assert!(stream.next().await.is_none());
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn native_stream_header_and_idle_timeouts_never_complete_a_stalled_stop() {
    for stall in [1, 3] {
        let mut reply = Reply::sse(&[native_reply("STOP")]);
        reply.stall = stall;
        let mut fixture = Fixture::start(vec![reply]).await;
        let (client, _) = client(
            &fixture.base,
            Some(KEY),
            Limits {
                header_timeout: Duration::from_secs(if stall == 1 { 2 } else { 10 }),
                idle_timeout: Duration::from_millis(200),
                ..Default::default()
            },
        );
        let result = client
            .stream_content("models/fixture", native_input(), RequestContext::default())
            .await;
        let error = if stall == 1 {
            result.err().unwrap()
        } else {
            let mut stream = result.unwrap();
            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                NativeStreamEvent::Event(_)
            ));
            let error = stream.next().await.unwrap().err().unwrap();
            assert!(stream.next().await.is_none());
            error
        };
        assert_eq!((error.http_status, error.code), (504, "provider_timeout"));
        fixture.request().await;
        fixture.disconnected().await;
    }
}

#[tokio::test]
async fn signed_json_and_stream_history_restore_the_exact_native_content_for_a_second_post() {
    use caidex_model_core::{CanonicalRequest, ResponsesDialect};
    use caidex_provider_google::NativeHistory;
    for streaming in [false, true] {
        let mut first = native_reply("STOP");
        first["candidates"][0]["content"]["parts"] = json!([
            {"text":"原生思考","thought":true,"thoughtSignature":"c2lnMQ=="},
            {"functionCall":{"name":"echo","id":"native-call","args":{"text":"原文"}},"thoughtSignature":"c2lnMg=="},
            {"futurePart":{"opaque":true}}
        ]);
        let initial = if streaming {
            Reply::sse(std::slice::from_ref(&first))
        } else {
            Reply::json(first.clone())
        };
        let mut fixture = Fixture::start(vec![initial, Reply::json(native_reply("STOP"))]).await;
        let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
        let mut input = native_input();
        input["tools"] =
            json!([{"functionDeclarations":[{"name":"echo","parameters":{"type":"object"}}]}]);
        let history = if streaming {
            let mut stream = client
                .stream_content(
                    "models/fixture-001",
                    input.clone(),
                    RequestContext::default(),
                )
                .await
                .unwrap();
            loop {
                if let NativeStreamEvent::Completed(complete) =
                    stream.next().await.unwrap().unwrap()
                {
                    break NativeHistory::from_stream(
                        &complete,
                        "models/fixture-001",
                        &input,
                        Some(0),
                        "history-fixture",
                        128 * 1024,
                    )
                    .unwrap();
                }
            }
        } else {
            let response = client
                .generate_content(
                    "models/fixture-001",
                    input.clone(),
                    RequestContext::default(),
                )
                .await
                .unwrap();
            NativeHistory::from_response(
                &response,
                "models/fixture-001",
                &input,
                Some(0),
                "history-fixture",
                128 * 1024,
            )
            .unwrap()
        };
        assert_eq!(request_body(&fixture.request().await), input);
        let projection = history.to_responses(128 * 1024).unwrap();
        assert_eq!(projection.output()[1]["call_id"], "native-call");
        let canonical = CanonicalRequest::new(
            json!({"model":"alias","input":projection.output()}),
            ResponsesDialect::Lite,
        )
        .unwrap();
        let stored: Value =
            serde_json::from_slice(&serde_json::to_vec(&canonical).unwrap()).unwrap();
        let restored = NativeHistory::from_responses_output(
            stored["input"].as_array().unwrap(),
            "models/fixture-001",
            &input,
            128 * 1024,
        )
        .unwrap();
        assert_eq!(restored.native_response(), &first);
        let content = restored.replay_content().unwrap();
        assert_eq!(content, &first["candidates"][0]["content"]);
        let mut next = input.clone();
        next["contents"]
            .as_array_mut()
            .unwrap()
            .push(content.clone());
        next["contents"].as_array_mut().unwrap().push(json!({"role":"user","parts":[{"functionResponse":{"name":"echo","id":"native-call","response":{"result":"合成结果"}}}]}));
        client
            .generate_content(
                "models/fixture-001",
                next.clone(),
                RequestContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(request_body(&fixture.request().await), next);
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}
