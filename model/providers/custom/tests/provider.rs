//! Independent inference client acceptance: synthetic keys and loopback only.
use caidex_credentials::{Broker, CredentialRef, Id, Secret, SecretKind, SecretStore};
use caidex_model_core::{
    CancellationToken, CanonicalRequest, CapabilitySupport, ContextHeaders, CredentialRequirement,
    EvidenceSource, ModelProvider, ProviderStreamEvent, REQUEST_HEADERS, RequestContext,
    ResponsesDialect, StreamState,
};
use caidex_provider_custom::{
    ClientOptions, ConfiguredModel, CustomResponses, CustomResponsesProvider, Limits,
};
use futures_util::StreamExt;
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose,
};
use rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};
use tokio_rustls::TlsAcceptor;

const KEY: &str = "CAIDEX_SYNTHETIC_DIRECT_PROVIDER_KEY";
const WAIT: Duration = Duration::from_secs(10);
const CREATED: &str = "data: {\"type\":\"response.created\",\"response\":{\"id\":\"fixture\"}}\n\n";
const DONE: &str = "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"fixture\",\"status\":\"completed\"}}\n\n";

fn reference() -> CredentialRef {
    CredentialRef {
        owner: Id::new("executor").unwrap(),
        provider: Id::new("custom").unwrap(),
        profile: Id::new("fixture").unwrap(),
        kind: SecretKind::ApiKey,
    }
}
struct Store(Arc<AtomicUsize>);
impl SecretStore for Store {
    fn get(&self, _: &CredentialRef) -> caidex_credentials::Result<Option<Secret>> {
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

#[derive(Clone)]
struct Reply {
    status: u16,
    content_type: &'static str,
    headers: String,
    body: Vec<u8>,
    stall: bool,
}
fn json_reply() -> Reply {
    Reply { status: 200, content_type: "application/json", headers: String::new(), body: json!({"id":"fixture", "status":"completed", "output":[
        {"type":"message", "content":[{"type":"output_text", "text":"中文🙂"}]},
        {"type":"function_call", "name":"data_only", "call_id":"call", "arguments":" { \"n\": 1.00 } "},
        {"type":"reasoning", "encrypted_content":"opaque+/==", "signature":"native-signature"}
    ], "usage":{"input_tokens":3,"cache_extension":4}, "future":{"big":18446744073709551616_u128}}).to_string().into_bytes(), stall: false }
}
struct Captured {
    headers: String,
    body: Value,
}
struct Fixture {
    endpoint: String,
    requests: mpsc::UnboundedReceiver<Captured>,
    closed: mpsc::UnboundedReceiver<()>,
    accepted: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}
impl Fixture {
    async fn start(reply: Reply, tls: Option<Arc<ServerConfig>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "{}://{}/v1/responses",
            if tls.is_some() { "https" } else { "http" },
            listener.local_addr().unwrap()
        );
        let (tx, requests) = mpsc::unbounded_channel();
        let (closed_tx, closed) = mpsc::unbounded_channel();
        let accepted = Arc::new(AtomicUsize::new(0));
        let accepts = accepted.clone();
        let task = tokio::spawn(async move {
            let mut sessions = JoinSet::new();
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                accepts.fetch_add(1, Ordering::SeqCst);
                let (reply, tx, closed_tx, tls) =
                    (reply.clone(), tx.clone(), closed_tx.clone(), tls.clone());
                sessions.spawn(async move {
                    if let Some(config) = tls {
                        if let Ok(mut socket) = TlsAcceptor::from(config).accept(socket).await {
                            let _ = serve(&mut socket, &reply, &tx).await;
                        }
                    } else {
                        let mut socket = socket;
                        let _ = serve(&mut socket, &reply, &tx).await;
                    }
                    let _ = closed_tx.send(());
                });
                // Completed sessions must not accumulate throughout the test.
                while sessions.try_join_next().is_some() {}
            }
        });
        Self {
            endpoint,
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
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn serve<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut S,
    reply: &Reply,
    tx: &mpsc::UnboundedSender<Captured>,
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
        .unwrap();
    let mut bytes = vec![0; size];
    socket.read_exact(&mut bytes).await?;
    let _ = tx.send(Captured {
        headers,
        body: serde_json::from_slice(&bytes).unwrap(),
    });
    let length = if reply.stall {
        "Transfer-Encoding: chunked\r\n".to_owned()
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
    if reply.stall {
        socket
            .write_all(format!("{:x}\r\n", reply.body.len()).as_bytes())
            .await?;
        socket.write_all(&reply.body).await?;
        socket.write_all(b"\r\n").await?;
        // A read EOF/reset here is proof that cancellation closed the real socket.
        let _ = socket.read(&mut [0]).await?;
    } else {
        socket.write_all(&reply.body).await?;
    }
    Ok(())
}

fn model(endpoint: &str) -> ConfiguredModel {
    ConfiguredModel::new(
        "fixture".into(),
        "native-fixture".into(),
        vec![ResponsesDialect::Classic, ResponsesDialect::Lite],
        CustomResponses::new(endpoint, Some(reference())).unwrap(),
    )
    .unwrap()
}
fn provider(
    models: Vec<ConfiguredModel>,
    limits: Limits,
    options: ClientOptions,
) -> (CustomResponsesProvider<Store>, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let broker = Arc::new(Broker::new(
        Id::new("executor").unwrap(),
        Store(reads.clone()),
    ));
    (
        CustomResponsesProvider::with_options(models, broker, limits, options).unwrap(),
        reads,
    )
}
fn request(stream: bool, dialect: ResponsesDialect) -> CanonicalRequest {
    CanonicalRequest::new(json!({"model":"fixture", "input":[{"type":"reasoning", "encrypted_content":"opaque+/==", "unknown":"retain"}], "stream":stream, "future":{"big":18446744073709551616_u128}}), dialect).unwrap()
}

#[tokio::test]
async fn all_six_provider_methods_work_without_gateway_and_preserve_wire_context() {
    let mut reply = json_reply();
    reply.headers = "X-Request-Id: provider-id\r\nX-Codex-Turn-State: state+/==\r\nSet-Cookie: forbidden\r\nX-Other: forbidden\r\n".into();
    let mut fixture = Fixture::start(reply.clone(), None).await;
    let (provider, reads) = provider(
        vec![model(&fixture.endpoint)],
        Limits::default(),
        ClientOptions::default(),
    );
    let provider: &dyn ModelProvider = &provider;
    let models = provider.list_models().await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].source, EvidenceSource::Configured);
    assert_eq!(models[0].codex_compatibility, None);
    assert_eq!(provider.metadata("fixture").unwrap(), models[0]);
    assert_eq!(
        provider.capabilities("fixture").unwrap().context_window,
        None
    );
    assert!(
        matches!(provider.credential_requirements("fixture").unwrap(), CredentialRequirement::Bearer { reference: r } if r == reference())
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let mut headers = ContextHeaders::default();
    for (name, value) in [
        ("session_id", "session"),
        ("x-client-request-id", "client"),
        ("x-codex-turn-state", "opaque+/=="),
        ("x-codex-turn-metadata", "metadata"),
    ] {
        headers.insert(name, value.into(), REQUEST_HEADERS).unwrap();
    }
    let response = provider
        .create_response(
            request(false, ResponsesDialect::Classic),
            RequestContext {
                headers,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        response.response.wire(),
        &serde_json::from_slice::<Value>(&reply.body).unwrap()
    );
    assert_eq!(
        response.response.output_text().collect::<Vec<_>>(),
        ["中文🙂"]
    );
    assert_eq!(
        response.headers.get("x-codex-turn-state"),
        Some("state+/==")
    );
    assert_eq!(response.headers.iter().count(), 2);
    let captured = fixture.request().await;
    assert_eq!(captured.body["model"], "native-fixture");
    assert_eq!(
        captured.body["input"],
        request(false, ResponsesDialect::Classic).wire()["input"]
    );
    assert_eq!(
        captured.body["future"],
        request(false, ResponsesDialect::Classic).wire()["future"]
    );
    assert!(
        captured
            .headers
            .contains(&format!("authorization: Bearer {KEY}\r\n"))
    );
    assert!(
        captured
            .headers
            .contains("x-codex-turn-state: opaque+/==\r\n")
    );
    assert!(!captured.headers.to_lowercase().contains("cookie:"));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let mut fixture = Fixture::start(
            Reply {
                content_type: "text/event-stream",
                body: format!("{CREATED}{DONE}").into_bytes(),
                ..json_reply()
            },
            None,
        )
        .await;
        let (direct, _) = self::provider(
            vec![model(&fixture.endpoint)],
            Limits::default(),
            ClientOptions::default(),
        );
        let mut stream = direct
            .stream_response(request(true, dialect), RequestContext::default())
            .await
            .unwrap();
        let mut terminals = vec![];
        while let Some(event) = stream.events.next().await {
            if let ProviderStreamEvent::Model(event) = event.unwrap() {
                terminals.extend(event.response.terminal());
            }
        }
        assert_eq!(terminals, [StreamState::Completed]);
        let captured = fixture.request().await;
        assert_eq!(
            captured
                .headers
                .contains("x-openai-internal-codex-responses-lite: true"),
            dialect == ResponsesDialect::Lite
        );
    }
}

#[tokio::test]
async fn rejected_requests_do_not_read_credentials_or_connect() {
    let fixture = Fixture::start(json_reply(), None).await;
    let route = model(&fixture.endpoint);
    let mut metadata = route.metadata().clone();
    metadata.capabilities.streaming = CapabilitySupport::Unsupported;
    let (provider, reads) = provider(
        vec![route.with_metadata(metadata).unwrap()],
        Limits::default(),
        ClientOptions::default(),
    );
    let token = CancellationToken::new();
    token.cancel();
    let error = provider
        .create_response(
            request(false, ResponsesDialect::Classic),
            RequestContext {
                cancellation: token,
                ..Default::default()
            },
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "provider_cancelled");
    let error = provider
        .create_response(
            request(false, ResponsesDialect::Classic),
            RequestContext {
                deadline: Some(std::time::Instant::now() - Duration::from_secs(1)),
                ..Default::default()
            },
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "provider_timeout");
    let error = provider
        .stream_response(
            request(true, ResponsesDialect::Classic),
            RequestContext::default(),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "unsupported_streaming");
    assert_eq!(
        provider.metadata("unknown").unwrap_err().code,
        "unknown_model"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[test]
fn retry_after_accepts_dates_seconds_and_rounds_without_retrying() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    assert_eq!(caidex_provider_custom::retry_after("12", now), Some(12));
    assert_eq!(
        caidex_provider_custom::retry_after(
            &httpdate::fmt_http_date(now + Duration::from_secs(12)),
            now + Duration::from_millis(500)
        ),
        Some(12)
    );
    assert_eq!(
        caidex_provider_custom::retry_after(
            &httpdate::fmt_http_date(now - Duration::from_secs(12)),
            now
        ),
        Some(0)
    );
    for bad in [
        "86401",
        "-1",
        "1.5",
        "",
        "invalid",
        "999999999999999999999999",
    ] {
        assert_eq!(caidex_provider_custom::retry_after(bad, now), None);
    }
    assert_eq!(
        caidex_provider_custom::retry_after(
            &httpdate::fmt_http_date(now + Duration::from_secs(86401)),
            now
        ),
        None
    );
}

#[tokio::test]
async fn http_date_hint_is_metadata_and_errors_never_echo_bodies_or_replay_posts() {
    let mut reply = json_reply();
    reply.status = 429;
    reply.headers = format!(
        "Retry-After: {}\r\n",
        httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(60))
    );
    reply.body = format!("{{\"error\":\"{KEY}\"}}").into_bytes();
    let mut fixture = Fixture::start(reply, None).await;
    let (provider, _) = provider(
        vec![model(&fixture.endpoint)],
        Limits::default(),
        ClientOptions::default(),
    );
    let error = provider
        .create_response(
            request(false, ResponsesDialect::Classic),
            RequestContext::default(),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "provider_rate_limited");
    assert!((1..=60).contains(&error.retry_after_seconds.unwrap()));
    assert!(!format!("{error:?} {}", error.wire()).contains(KEY));
    fixture.request().await;
    fixture.disconnected().await;
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn malformed_provider_context_is_a_provider_failure() {
    let mut reply = json_reply();
    reply.headers = "X-Codex-Turn-State: first\r\nX-Codex-Turn-State: second\r\n".into();
    let fixture = Fixture::start(reply, None).await;
    let (provider, _) = provider(
        vec![model(&fixture.endpoint)],
        Limits::default(),
        ClientOptions::default(),
    );
    let error = provider
        .create_response(
            request(false, ResponsesDialect::Classic),
            RequestContext::default(),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(
        (error.http_status, error.code),
        (502, "provider_invalid_context_header")
    );
}

#[tokio::test]
async fn stream_drop_cancel_and_unpolled_deadline_close_the_socket() {
    for mode in ["drop", "cancel", "deadline"] {
        let body = format!(
            "{CREATED}data: {{\"type\":\"response.output_text.delta\",\"delta\":\"first\"}}\n\ndata: {{\"type\":\"response.output_text.delta\",\"delta\":\"second\"}}\n\n"
        );
        let mut fixture = Fixture::start(
            Reply {
                content_type: "text/event-stream",
                body: body.into_bytes(),
                stall: true,
                ..json_reply()
            },
            None,
        )
        .await;
        let limits = Limits {
            in_flight: 1,
            total_timeout: if mode == "deadline" {
                Duration::from_millis(200)
            } else {
                WAIT
            },
            ..Default::default()
        };
        let (provider, _) = provider(
            vec![model(&fixture.endpoint)],
            limits,
            ClientOptions::default(),
        );
        let token = CancellationToken::new();
        let mut response = provider
            .stream_response(
                request(true, ResponsesDialect::Classic),
                RequestContext {
                    cancellation: token.clone(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        fixture.request().await;
        if mode == "drop" {
            drop(response);
            fixture.disconnected().await;
            continue;
        }
        // Fill the single slot before cancellation; error delivery cannot depend
        // on the consumer making room while the producer owns the socket.
        tokio::time::sleep(Duration::from_millis(30)).await;
        if mode == "cancel" {
            token.cancel();
        }
        fixture.disconnected().await;
        let mut errors = vec![];
        let mut terminals = vec![];
        while let Some(event) = tokio::time::timeout(WAIT, response.events.next())
            .await
            .unwrap()
        {
            match event {
                Err(error) => errors.push(error.code),
                Ok(ProviderStreamEvent::Model(event)) => {
                    terminals.extend(event.response.terminal())
                }
                Ok(ProviderStreamEvent::Heartbeat) => {}
            }
        }
        assert_eq!(
            errors,
            [if mode == "cancel" {
                "provider_cancelled"
            } else {
                "provider_timeout"
            }]
        );
        assert!(terminals.is_empty());
        drop(response);
        // All paths release the in-flight slot as the stream is dropped.
        let response = provider
            .stream_response(
                request(true, ResponsesDialect::Classic),
                RequestContext::default(),
            )
            .await
            .unwrap();
        drop(response);
    }
}

fn certificates(host: &str, expired: bool) -> (Arc<ServerConfig>, reqwest::Certificate) {
    let now = time::OffsetDateTime::now_utc();
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, "CAIdex fixture CA");
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    params.not_before = now - time::Duration::days(3);
    params.not_after = now + time::Duration::days(3);
    let issuer = CertifiedIssuer::self_signed(params, KeyPair::generate().unwrap()).unwrap();
    let mut leaf = CertificateParams::new(vec![host.into()]).unwrap();
    // Distinct issuer/subject and AKI make this an issued leaf on native chain
    // engines too; rcgen's identical default CNs resemble a self-issued leaf.
    leaf.distinguished_name.push(DnType::CommonName, host);
    leaf.use_authority_key_identifier_extension = true;
    leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    leaf.not_before = now - time::Duration::days(2);
    leaf.not_after = now + time::Duration::days(if expired { -1 } else { 1 });
    leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let key = KeyPair::generate().unwrap();
    let certificate = leaf.signed_by(&key, &issuer).unwrap();
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.der().clone()],
            PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        )
        .unwrap();
    // Only the public root reaches client configuration; private keys stay in
    // test memory and are never persisted or logged.
    (
        Arc::new(config),
        reqwest::Certificate::from_der(issuer.der()).unwrap(),
    )
}

#[tokio::test]
async fn tls_validates_trust_hostname_and_expiry_with_explicit_client_roots() {
    for (host, expired, trust, success) in [
        ("127.0.0.1", false, true, true),
        ("127.0.0.1", false, false, false),
        ("wrong.example", false, true, false),
        ("127.0.0.1", true, true, false),
    ] {
        let (server, root) = certificates(host, expired);
        let diagnostic_root = root.clone();
        let mut fixture = Fixture::start(json_reply(), Some(server)).await;
        let options = ClientOptions {
            root_certificates: if trust { vec![root] } else { vec![] },
        };
        let (provider, _) = provider(vec![model(&fixture.endpoint)], Limits::default(), options);
        let response = provider
            .create_response(
                request(false, ResponsesDialect::Classic),
                RequestContext::default(),
            )
            .await;
        if success {
            if response.is_err() {
                // Diagnose this synthetic TLS fixture only, with no auth or model content.
                // Production errors remain safe static classifications.
                let diagnostic = reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .retry(reqwest::retry::never())
                    .timeout(WAIT)
                    .add_root_certificate(diagnostic_root)
                    .build()
                    .unwrap()
                    .post(&fixture.endpoint)
                    .body("{}")
                    .send()
                    .await;
                panic!("synthetic TLS positive fixture failed: {diagnostic:?}");
            }
            assert_eq!(response.unwrap().response.state(), StreamState::Completed);
            assert!(
                fixture
                    .request()
                    .await
                    .headers
                    .contains(&format!("authorization: Bearer {KEY}"))
            );
        } else {
            let error = response.err().unwrap();
            assert_eq!(
                (error.http_status, error.code),
                (502, "provider_transport_error")
            );
            assert!(!format!("{error:?}").contains(&fixture.endpoint));
            fixture.disconnected().await;
            assert!(
                fixture.requests.try_recv().is_err(),
                "failed TLS must never carry an HTTP credential"
            );
        }
        assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
    }
}
