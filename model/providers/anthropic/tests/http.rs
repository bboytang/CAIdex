use caidex_credentials::{Broker, CredentialRef, Id, Secret, SecretKind, SecretStore};
use caidex_model_core::{CancellationToken, RequestContext};
use caidex_provider_anthropic::{AnthropicClient, AnthropicConfig, Limits};
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
};

#[path = "http/compiled.rs"]
mod compiled;
#[path = "http/context.rs"]
mod context;
#[path = "http/provider.rs"]
mod provider;
#[path = "http/streaming.rs"]
mod streaming;

const KEY: &str = "SYNTHETIC_ANTHROPIC_KEY";
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
        provider: Id::new("anthropic").unwrap(),
        profile: Id::new("fixture").unwrap(),
        kind: SecretKind::ApiKey,
    }
}
fn client(
    base: &str,
    key: Option<&'static str>,
    limits: Limits,
) -> (AnthropicClient<Store>, Arc<AtomicUsize>) {
    client_with_context(base, key, limits, false)
}
fn client_with_context(
    base: &str,
    key: Option<&'static str>,
    limits: Limits,
    local_context: bool,
) -> (AnthropicClient<Store>, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let broker = Arc::new(Broker::new(
        Id::new("executor").unwrap(),
        Store {
            reads: reads.clone(),
            key,
        },
    ));
    let mut config = AnthropicConfig::new(reference())
        .unwrap()
        .with_base_url(base)
        .unwrap()
        .with_workspace("fixture-workspace")
        .unwrap();
    if local_context {
        config = config.with_local_runtime_context();
    }
    (AnthropicClient::new(config, broker, limits).unwrap(), reads)
}
fn binding_client(base: &str, limits: Limits) -> (AnthropicClient<Store>, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let broker = Arc::new(Broker::new(
        Id::new("executor").unwrap(),
        Store {
            reads: reads.clone(),
            key: Some(KEY),
        },
    ));
    let config = AnthropicConfig::new(reference())
        .unwrap()
        .with_base_url(base)
        .unwrap()
        .with_thinking_binding_controls();
    (AnthropicClient::new(config, broker, limits).unwrap(), reads)
}
fn page(id: &str, more: bool) -> Value {
    json!({"data":[{"id":id,"type":"model","created_at":"2026-01-01T00:00:00Z",
        "display_name":id}],"has_more":more,"first_id":id,"last_id":id})
}
fn reply() -> Value {
    json!({"type":"message","role":"assistant","id":"msg-fixture","model":"native",
        "content":[{"type":"thinking","thinking":"private","signature":"sig+/==\n"},
        {"type":"text","text":"中文🙂"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"output_tokens":3}})
}
fn request() -> Value {
    json!({"model":"caller-model","max_tokens":32,"messages":[{"role":"user","content":"hello"}],"future":{"keep":true}})
}

// Actual loopback HTTP with captured headers/body and EOF observation. The
// fixture never contacts Anthropic or reads a user credential.
async fn fixture(
    replies: Vec<(u16, String)>,
    stall: bool,
) -> (
    String,
    mpsc::UnboundedReceiver<(String, Vec<u8>)>,
    mpsc::UnboundedReceiver<()>,
    tokio::task::JoinHandle<()>,
) {
    fixture_with_headers(replies, stall, "").await
}
async fn fixture_with_headers(
    replies: Vec<(u16, String)>,
    stall: bool,
    headers: &str,
) -> (
    String,
    mpsc::UnboundedReceiver<(String, Vec<u8>)>,
    mpsc::UnboundedReceiver<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/proxy/v1/", listener.local_addr().unwrap());
    let (tx, requests) = mpsc::unbounded_channel();
    let (closed_tx, closed) = mpsc::unbounded_channel();
    let headers = headers.to_owned();
    let task = tokio::spawn(async move {
        for (status, body) in replies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let (head, offset, length) = loop {
                let mut buffer = [0; 1024];
                let n = socket.read(&mut buffer).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
                if let Some(offset) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                    let head = String::from_utf8(bytes[..offset].to_vec()).unwrap();
                    let length = head
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    break (head, offset + 4, length);
                }
            };
            while bytes.len() < offset + length {
                let mut buffer = [0; 1024];
                let n = socket.read(&mut buffer).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
            }
            tx.send((head, bytes[offset..offset + length].to_vec()))
                .unwrap();
            if stall {
                let mut byte = [0];
                match socket.read(&mut byte).await {
                    Ok(0) | Err(_) => {
                        closed_tx.send(()).unwrap();
                    }
                    other => panic!("unexpected read {other:?}"),
                }
            } else {
                let response = format!(
                    "HTTP/1.1 {status} Fixture\r\ncontent-type: application/json\r\nretry-after: 7\r\ncontent-length: {}\r\nconnection: close\r\n{headers}\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        }
    });
    (base, requests, closed, task)
}
async fn received<T>(receiver: &mut mpsc::UnboundedReceiver<T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), receiver.recv())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn discovery_uses_native_auth_scope_encoded_cursor_and_complete_pages() {
    let id = "cursor+/=&?";
    let (base, mut requests, _, task) = fixture(
        vec![
            (200, page(id, true).to_string()),
            (200, page("last", false).to_string()),
        ],
        false,
    )
    .await;
    let (client, reads) = client(&base, Some(KEY), Limits::default());
    let models = client
        .discover_models(3, RequestContext::default())
        .await
        .unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    let (first, _) = received(&mut requests).await;
    let (second, _) = received(&mut requests).await;
    for head in [&first, &second] {
        let lower = head.to_ascii_lowercase();
        assert!(lower.contains("x-api-key: synthetic_anthropic_key"));
        assert!(lower.contains("anthropic-version: 2023-06-01"));
        assert!(lower.contains("anthropic-workspace-id: fixture-workspace"));
        assert!(!lower.contains("authorization:"));
    }
    assert!(first.starts_with("GET /proxy/v1/models?limit=1000 "));
    let path = second
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap();
    let url = reqwest::Url::parse(&format!("http://fixture{path}")).unwrap();
    assert_eq!(
        url.query_pairs()
            .find(|(key, _)| key == "after_id")
            .unwrap()
            .1,
        id
    );
    task.await.unwrap();
}
#[tokio::test]
async fn native_message_posts_once_and_replays_signed_content_without_translation() {
    let (base, mut requests, _, task) = fixture(
        vec![(200, reply().to_string()), (200, reply().to_string())],
        false,
    )
    .await;
    let (client, _) = client(&base, Some(KEY), Limits::default());
    let first = client
        .create_message("native", request(), RequestContext::default())
        .await
        .unwrap();
    let mut second = request();
    second["messages"]
        .as_array_mut()
        .unwrap()
        .push(first.replay_message());
    client
        .create_message("native", second.clone(), RequestContext::default())
        .await
        .unwrap();
    let (head, body) = received(&mut requests).await;
    assert!(head.starts_with("POST /proxy/v1/messages "));
    assert!(!head.to_ascii_lowercase().contains("anthropic-beta:"));
    let wire: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(wire["model"], "native");
    assert_eq!(wire["stream"], false);
    assert_eq!(wire["future"], request()["future"]);
    let (_, body) = received(&mut requests).await;
    let wire: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(wire["messages"], second["messages"]);
    task.await.unwrap();
}
#[tokio::test]
async fn errors_are_safe_and_rate_limit_is_metadata_without_retry() {
    let (base, mut requests, _, task) =
        fixture(vec![(429, format!("{{\"secret\":\"{KEY}\"}}"))], false).await;
    let (client, _) = client(&base, Some(KEY), Limits::default());
    let error = client
        .create_message("native", request(), RequestContext::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, "provider_rate_limited");
    assert_eq!(error.retry_after_seconds, Some(7));
    assert!(!format!("{error:?}").contains(KEY));
    received(&mut requests).await;
    task.await.unwrap();
}
#[tokio::test]
async fn cancelled_and_missing_credentials_never_send_a_request() {
    let (client, reads) = client("http://127.0.0.1:1/v1", None, Limits::default());
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let error = client
        .discover_models(
            3,
            RequestContext {
                cancellation,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "provider_cancelled");
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(
        client
            .discover_models(3, RequestContext::default())
            .await
            .unwrap_err()
            .code,
        "credential_missing"
    );
}
#[tokio::test]
async fn cancellation_and_header_deadline_close_real_sockets_and_release_permits() {
    for cancel in [true, false] {
        let (base, mut requests, mut closed, task) =
            fixture(vec![(200, String::new())], true).await;
        let limits = Limits {
            in_flight: 1,
            header_timeout: Duration::from_millis(300),
            ..Limits::default()
        };
        let (client, _) = client(&base, Some(KEY), limits);
        let client = Arc::new(client);
        let cancellation = CancellationToken::new();
        let context = RequestContext {
            cancellation: cancellation.clone(),
            ..Default::default()
        };
        let worker_client = client.clone();
        let worker = tokio::spawn(async move {
            worker_client
                .create_message("native", request(), context)
                .await
        });
        received(&mut requests).await;
        assert_eq!(
            client
                .discover_models(2, RequestContext::default())
                .await
                .unwrap_err()
                .code,
            "provider_busy"
        );
        if cancel {
            cancellation.cancel();
        }
        assert_eq!(
            worker.await.unwrap().unwrap_err().code,
            if cancel {
                "provider_cancelled"
            } else {
                "provider_timeout"
            }
        );
        received(&mut closed).await;
        task.await.unwrap();
        assert_ne!(
            client
                .discover_models(2, RequestContext::default())
                .await
                .unwrap_err()
                .code,
            "provider_busy"
        );
    }
}
#[tokio::test]
async fn aggregate_paging_budget_and_model_count_fail_instead_of_partial_results() {
    for byte_limit in [false, true] {
        let one = page("one", true).to_string();
        let two = page("two", false).to_string();
        let (base, _requests, _closed, task) =
            fixture(vec![(200, one.clone()), (200, two.clone())], false).await;
        let limits = Limits {
            response_bytes: if byte_limit {
                one.len() + two.len() - 1
            } else {
                10000
            },
            ..Limits::default()
        };
        let (client, _) = client(&base, Some(KEY), limits);
        let error = client
            .discover_models(if byte_limit { 3 } else { 1 }, RequestContext::default())
            .await
            .unwrap_err();
        assert_eq!(
            error.code,
            if byte_limit {
                "provider_oversized_response"
            } else {
                "anthropic_invalid_model_catalog"
            }
        );
        task.await.unwrap();
    }
}
#[test]
fn endpoint_scope_and_credential_types_are_executor_validated() {
    for base in [
        "http://example.com/v1",
        "https://a/v1?key=secret",
        "https://user:pass@a/v1",
        "https://a/v1#secret",
    ] {
        assert!(
            AnthropicConfig::new(reference())
                .unwrap()
                .with_base_url(base)
                .is_err()
        );
    }
    for workspace in ["", "bad\nheader", "bad scope"] {
        assert!(
            AnthropicConfig::new(reference())
                .unwrap()
                .with_workspace(workspace)
                .is_err()
        );
    }
    let mut wrong = reference();
    wrong.provider = Id::new("openai").unwrap();
    assert!(AnthropicConfig::new(wrong).is_err());
    let mut wrong = reference();
    wrong.kind = SecretKind::AccessToken;
    assert!(AnthropicConfig::new(wrong).is_err());
    assert!(!format!("{:?}", AnthropicConfig::new(reference()).unwrap()).contains("executor"));
}

#[tokio::test]
async fn binding_beta_requires_executor_opt_in_before_broker_or_network() {
    use caidex_provider_anthropic::{AnthropicProvider, ReasoningMapping};
    let (client, reads) = client("http://127.0.0.1:1/v1", Some(KEY), Limits::default());
    let mut native = request();
    native["thinking"] =
        json!({"type":"adaptive","block_binding":{"prefix_mismatch_behavior":"error"}});
    assert_eq!(
        client
            .create_message("native", native.clone(), RequestContext::default())
            .await
            .unwrap_err()
            .code,
        "anthropic_thinking_binding_beta_required"
    );
    assert_eq!(
        client
            .stream_message("native", native, RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "anthropic_thinking_binding_beta_required"
    );
    let mut profile = provider::profile();
    profile.reasoning_mappings = vec![
        ReasoningMapping::new(
            "high".into(),
            None,
            Some(json!({"type":"adaptive","block_binding":{"prefix_mismatch_behavior":"error"}})),
        )
        .unwrap(),
    ];
    assert_eq!(
        AnthropicProvider::new(client, vec![profile], 10)
            .err()
            .unwrap()
            .code,
        "anthropic_thinking_binding_beta_required"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn binding_beta_json_requires_reports_preserves_unknowns_and_never_retries() {
    for report in [
        None,
        Some(Value::Null),
        Some(json!([])),
        Some(json!([{"type":"future_transform","opaque":"PRIVATE_REPORT"}])),
    ] {
        let mut native = reply();
        if let Some(report) = &report {
            native["input_transformations"] = report.clone();
        }
        let (base, mut requests, _, task) = fixture(vec![(200, native.to_string())], false).await;
        let (client, reads) = binding_client(&base, Limits::default());
        let result = client
            .create_message("native", request(), RequestContext::default())
            .await;
        if report.as_ref().is_some_and(Value::is_array) {
            assert_eq!(result.unwrap().wire(), &native);
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.http_status, 502);
            assert_eq!(error.code, "anthropic_binding_report_missing");
            assert!(!format!("{error:?}").contains("PRIVATE_REPORT"));
        }
        let (head, _) = received(&mut requests).await;
        assert!(
            head.to_ascii_lowercase()
                .contains("anthropic-beta: thinking-binding-controls-2026-08-01")
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        task.await.unwrap();
        assert!(requests.try_recv().is_err());
    }
    let (base, mut requests, _, task) =
        fixture(vec![(400, format!("PRIVATE_DIAGNOSTIC {KEY}"))], false).await;
    let (client, reads) = binding_client(&base, Limits::default());
    let error = client
        .create_message("native", request(), RequestContext::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, "provider_request_rejected");
    assert!(!format!("{error:?}").contains("PRIVATE_DIAGNOSTIC"));
    received(&mut requests).await;
    task.await.unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert!(requests.try_recv().is_err());
    // Catalogue responses are not Messages and have no thinking report contract.
    let (base, mut requests, _, task) =
        fixture(vec![(200, page("native", false).to_string())], false).await;
    let (client, reads) = binding_client(&base, Limits::default());
    assert_eq!(
        client
            .discover_models(10, RequestContext::default())
            .await
            .unwrap()[0]
            .id(),
        "native"
    );
    let (head, _) = received(&mut requests).await;
    assert!(
        head.to_ascii_lowercase()
            .contains("anthropic-beta: thinking-binding-controls-2026-08-01")
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    task.await.unwrap();
}
