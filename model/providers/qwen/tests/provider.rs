//! Offline native HTTP/SSE; synthetic executor key and isolated loopback only.
use caidex_credentials::{Broker, CredentialRef, Id, Secret, SecretKind, SecretStore};
use caidex_model_core::{
    CancellationToken, CanonicalRequest, CapabilitySupport, CredentialRequirement, EvidenceSource,
    ModelMetadata, ModelProvider, ProviderStreamEvent, REQUEST_HEADERS, RequestContext,
    ResponsesDialect,
};
use caidex_provider_qwen::{Limits, QwenConfig, QwenProvider};
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

const KEY: &str = "CAIDEX_SYNTHETIC_QWEN_KEY";
const WAIT: Duration = Duration::from_secs(10);
fn reference() -> CredentialRef {
    CredentialRef {
        owner: Id::new("executor").unwrap(),
        provider: Id::new("qwen").unwrap(),
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
        assert_eq!(r.provider.as_str(), "qwen");
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
        {"type":"reasoning","id":"rs_one","status":"completed","summary":[{"type":"summary_text","text":"Native thinking"}]},
        {"type":"message","id":"msg_one","status":"completed","role":"assistant","content":[{"type":"output_text","text":"中文🙂","annotations":[]}]},
        {"type":"future_item","n":18446744073709551616_u128}],
        "usage":{"input_tokens":2,"output_tokens":3,"total_tokens":5,"output_tokens_details":{"reasoning_tokens":1}},"future":{"n":18446744073709551616_u128}})
}
fn model(id: &str) -> Value {
    json!({"model":id,"name":"Fixture","features":["function-calling"],"published_time":null,"model_info":{"context_window":null,"max_input_tokens":null},"future":{"n":18446744073709551616_u128}})
}
fn page(total: u64, page_no: u64, models: Vec<Value>) -> Value {
    json!({"success":true,"code":null,"message":null,"output":{"total":total,"page_no":page_no,"page_size":20,"models":models},"request_id":"catalog-id"})
}
fn catalog() -> Value {
    page(1, 1, vec![model("native-fixture")])
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
    delay: Duration,
}
impl Reply {
    fn json(wire: Value) -> Self {
        Self {
            status: 200,
            media: "application/json",
            body: wire.to_string(),
            headers: "X-Request-Id: fixture-id\r\nSet-Cookie: forbidden\r\n".into(),
            stall: 0,
            delay: Duration::ZERO,
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
        let base = format!("http://{}/proxy", listener.local_addr().unwrap());
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
                    if !reply.delay.is_zero() {
                        tokio::time::sleep(reply.delay).await;
                    }
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
    ) -> QwenProvider<Store> {
        QwenProvider::new(
            QwenConfig::new(&self.base, reference()).unwrap(),
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
async fn native_paginated_catalog_and_six_method_contract_preserve_raw_data() {
    let first = page(
        21,
        1,
        (0..20).map(|i| model(&format!("model-{i:02}"))).collect(),
    );
    let second = page(21, 2, vec![model("native-fixture")]);
    let mut fixture = Fixture::start(vec![
        Reply::json(first.clone()),
        Reply::json(second.clone()),
        Reply::json(catalog()),
    ])
    .await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture.provider(
        broker,
        vec![
            metadata("fixture", "native-fixture"),
            metadata("absent", "missing"),
        ],
        limits(),
    );
    assert_eq!(
        provider.metadata("fixture").unwrap().source,
        EvidenceSource::Configured
    );
    assert_eq!(
        provider.capabilities("fixture").unwrap().native_tools,
        CapabilitySupport::Unknown
    );
    assert!(provider.metadata("unknown").is_err());
    assert!(
        matches!(provider.credential_requirements("fixture").unwrap(), CredentialRequirement::Bearer { reference:r } if r==reference())
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let models = provider
        .discover_models(RequestContext::default())
        .await
        .unwrap();
    assert_eq!(models.len(), 21);
    assert_eq!(models[20].id(), "native-fixture");
    assert_eq!(*models[20].wire(), second["output"]["models"][0]);
    assert_eq!(
        serde_json::to_value(&models[20]).unwrap(),
        *models[20].wire()
    );
    assert!(models[20].wire().get("created").is_none());
    assert!(models[20].wire().get("provider").is_none());
    assert!(!format!("{:?}", models[20]).contains("native-fixture"));
    for page in [1, 2] {
        let captured = fixture.captured().await;
        assert!(captured.headers.starts_with(&format!(
            "GET /proxy/api/v1/models?page_no={page}&page_size=20 "
        )));
        assert!(captured.body.is_none());
        assert_eq!(
            captured.header("authorization"),
            Some(format!("Bearer {KEY}").as_str())
        );
    }
    let listed = provider.list_models().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "fixture");
    assert_eq!(listed[0].source, EvidenceSource::ProviderCatalog);
    assert_eq!(
        listed[0].capabilities.native_tools,
        CapabilitySupport::Unknown
    );
    assert!(listed[0].codex_compatibility.is_none());
    assert_eq!(reads.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn malformed_pages_duplicate_ids_and_changing_totals_never_return_partial_catalogs() {
    let mut invalid = Vec::new();
    for (pointer, value) in [
        ("/success", json!(false)),
        ("/success", json!("true")),
        ("/output/total", json!(1.5)),
        ("/output/total", json!(5121)),
        ("/output/page_no", json!(2)),
        ("/output/page_size", json!(10)),
        ("/output/models", json!([])),
        ("/output/models/0/model", json!("bad\n")),
        ("/output/models/0/model", json!(null)),
    ] {
        let mut p = catalog();
        *p.pointer_mut(pointer).unwrap() = value;
        invalid.push(vec![Reply::json(p)]);
    }
    invalid.push(vec![Reply::json(json!({"object":"list","data":[]}))]);
    let first = page(
        21,
        1,
        (0..20).map(|i| model(&format!("model-{i}"))).collect(),
    );
    invalid.push(vec![
        Reply::json(first.clone()),
        Reply::json(page(21, 2, vec![model("model-0")])),
    ]);
    invalid.push(vec![
        Reply::json(first),
        Reply::json(page(22, 2, vec![model("next"), model("later")])),
    ]);
    for replies in invalid {
        let fixture = Fixture::start(replies).await;
        let (broker, _) = fixture_broker(Some(KEY));
        let provider = fixture.provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        );
        let error = provider.list_models().await.unwrap_err();
        assert!(matches!(
            error.code,
            "qwen_invalid_model_catalog" | "qwen_catalog_limit"
        ));
        assert!(!error.to_string().contains(KEY));
    }
    let fixture = Fixture::start(vec![Reply::json(page(0, 1, vec![]))]).await;
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
async fn classic_text_preserves_developer_priority_native_reasoning_and_unknown_json_sse() {
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
        let wire = json!({"model":"fixture","input":[{"role":"developer","content":[{"type":"input_text","text":"Keep priority"}]},{"role":"user","content":"中文🙂"},{"type":"message","id":"old","status":"completed","role":"assistant","content":[{"type":"output_text","text":"Earlier","annotations":[]}]}],"instructions":"Explicit","stream":streaming,"max_output_tokens":64,"temperature":0.5,"top_p":0.8,"background":false});
        if streaming {
            let mut stream = provider
                .stream_response(request(wire.clone()), RequestContext::default())
                .await
                .unwrap();
            let mut end = None;
            let mut text = String::new();
            while let Some(e) = stream.events.next().await {
                if let ProviderStreamEvent::Model(e) = e.unwrap() {
                    if e.response.kind() == "response.output_text.delta" {
                        text.push_str(e.response.wire()["delta"].as_str().unwrap());
                    }
                    if e.response.terminal().is_some() {
                        end = Some(e.response.wire()["response"].clone());
                    }
                }
            }
            assert_eq!(text, "中文🙂");
            assert_eq!(end, Some(native()));
        } else {
            let reply = provider
                .create_response(request(wire.clone()), RequestContext::default())
                .await
                .unwrap();
            assert_eq!(*reply.response.wire(), native());
            assert_eq!(reply.headers.get("x-request-id"), Some("fixture-id"));
            assert!(reply.headers.get("set-cookie").is_none());
        }
        let captured = fixture.captured().await;
        assert!(
            captured
                .headers
                .starts_with("POST /proxy/compatible-mode/v1/responses ")
        );
        let mut expected = wire;
        expected["model"] = "native-fixture".into();
        expected["store"] = false.into();
        expected.as_object_mut().unwrap().remove("background");
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
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn unsupported_controls_invalid_text_and_lite_refuse_before_credentials() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture.provider(
        broker,
        vec![metadata("fixture", "native-fixture")],
        limits(),
    );
    for (key, value) in [
        ("store", json!(true)),
        ("background", json!(true)),
        ("previous_response_id", json!("old")),
        ("conversation", json!("other")),
        ("tools", json!([])),
        ("tool_choice", json!("none")),
        ("parallel_tool_calls", json!(false)),
        ("reasoning", json!({"effort":"high"})),
        ("enable_thinking", json!(true)),
        ("thinking_budget", json!(500)),
        ("text", json!({"format":{"type":"text"}})),
        ("include", json!(["reasoning.encrypted_content"])),
        ("metadata", json!({})),
        ("client_metadata", json!({})),
        ("prompt_cache_key", json!("sensitive")),
        ("future_control", json!(true)),
        ("instructions", json!(7)),
        ("max_output_tokens", json!(15)),
        ("max_output_tokens", json!(1.5)),
        ("temperature", json!(2)),
        ("top_p", json!(0)),
    ] {
        let mut wire = basic(false);
        wire[key] = value;
        assert!(
            provider
                .create_response(request(wire), RequestContext::default())
                .await
                .is_err(),
            "{key}"
        );
    }
    for item in [
        json!({"type":"reasoning","id":"r","summary":[]}),
        json!({"type":"function_call","name":"x","arguments":"{}","call_id":"c"}),
        json!({"role":"user","content":[{"type":"input_image","image_url":"https://example.com/image"}]}),
        json!({"role":"assistant","content":[{"type":"input_text","text":"wrong role"}]}),
        json!({"role":"developer","content":[{"type":"input_text","text":2}]}),
        json!({"role":"user","content":"x","id":"unowned"}),
        json!({"type":"message","role":"assistant","id":"x","status":"in_progress","content":"x"}),
        json!({"role":"user","content":"x","future":true}),
    ] {
        let mut wire = basic(false);
        wire["input"] = json!([item]);
        assert!(
            provider
                .create_response(request(wire), RequestContext::default())
                .await
                .is_err()
        );
    }
    let lite = CanonicalRequest::new(
        json!({"model":"fixture","input":[{"role":"user","content":"x"}]}),
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
    let mut context = RequestContext::default();
    context
        .headers
        .insert("session_id", "local".into(), REQUEST_HEADERS)
        .unwrap();
    assert_eq!(
        provider
            .create_response(request(basic(false)), context)
            .await
            .err()
            .unwrap()
            .code,
        "qwen_unsupported_context"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn endpoint_provider_and_credential_ownership_are_explicit_and_never_fallback() {
    for url in [
        "http://example.com/",
        "http://localhost/",
        "https://user:pass@example.com/",
        "https://example.com/?key=bad",
        "https://example.com/#fragment",
        "file:///tmp/models",
    ] {
        assert!(QwenConfig::new(url, reference()).is_err());
    }
    let mut r = reference();
    r.provider = Id::new("openai").unwrap();
    assert!(QwenConfig::new("https://dashscope-intl.aliyuncs.com/", r).is_err());
    let mut r = reference();
    r.kind = SecretKind::AccessToken;
    assert!(QwenConfig::new("https://dashscope-intl.aliyuncs.com/", r).is_err());
    let c = QwenConfig::new(
        "https://workspace.cn-beijing.maas.aliyuncs.com/",
        reference(),
    )
    .unwrap();
    assert!(!format!("{c:?}").contains("workspace"));
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    for (key, owner, code, read_count) in [
        (None, "executor", "credential_missing", 1),
        (Some(KEY), "other", "credential_unavailable", 0),
    ] {
        let reads = Arc::new(AtomicUsize::new(0));
        let broker = Arc::new(Broker::new(
            Id::new(owner).unwrap(),
            Store {
                reads: reads.clone(),
                key,
            },
        ));
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
            code
        );
        assert_eq!(reads.load(Ordering::SeqCst), read_count);
    }
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
    let (broker, _) = fixture_broker(Some(KEY));
    assert!(
        QwenProvider::new(
            QwenConfig::new(&fixture.base, reference()).unwrap(),
            vec![ModelMetadata::configured(
                "fixture".into(),
                "native-fixture".into(),
                vec![ResponsesDialect::Lite]
            )],
            broker,
            limits()
        )
        .is_err()
    );
}

#[tokio::test]
async fn original_compiled_query_and_catalog_byte_budgets_are_enforced() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    for bytes in [
        basic(false).to_string().len() - 1,
        basic(false).to_string().len(),
    ] {
        let (broker, reads) = fixture_broker(Some(KEY));
        let mut bounded = limits();
        bounded.request_bytes = bytes;
        let provider =
            fixture.provider(broker, vec![metadata("fixture", "native-fixture")], bounded);
        assert_eq!(
            provider
                .create_response(request(basic(false)), RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            "invalid_or_oversized_body"
        );
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }
    let (broker, reads) = fixture_broker(Some(KEY));
    let mut bounded = limits();
    bounded.request_bytes = 10;
    let provider = fixture.provider(broker, vec![metadata("fixture", "native-fixture")], bounded);
    assert_eq!(
        provider
            .discover_models(RequestContext::default())
            .await
            .unwrap_err()
            .code,
        "invalid_or_oversized_body"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
    let first = page(21, 1, (0..20).map(|i| model(&format!("m{i}"))).collect());
    let second = page(21, 2, vec![model("native-fixture")]);
    let mut bound = limits();
    bound.response_bytes = first.to_string().len() + second.to_string().len() - 1;
    let fixture = Fixture::start(vec![Reply::json(first), Reply::json(second)]).await;
    let (broker, _) = fixture_broker(Some(KEY));
    assert_eq!(
        fixture
            .provider(broker, vec![metadata("fixture", "native-fixture")], bound)
            .discover_models(RequestContext::default())
            .await
            .unwrap_err()
            .code,
        "qwen_catalog_limit"
    );
}

#[tokio::test]
async fn cancellation_and_deadlines_refuse_before_key_and_close_incomplete_streams() {
    let mut stalled = Reply::stream(created());
    stalled.stall = 2;
    let mut fixture = Fixture::start(vec![stalled, Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture.provider(
        broker,
        vec![metadata("fixture", "native-fixture")],
        limits(),
    );
    for discovery in [false, true] {
        for expired in [false, true] {
            let mut context = RequestContext::default();
            if expired {
                context.deadline = Some(std::time::Instant::now() - Duration::from_secs(1));
            } else {
                context.cancellation.cancel();
            }
            let error = if discovery {
                provider.discover_models(context).await.err().unwrap()
            } else {
                provider
                    .create_response(request(basic(false)), context)
                    .await
                    .err()
                    .unwrap()
            };
            assert_eq!(
                error.code,
                if expired {
                    "provider_timeout"
                } else {
                    "provider_cancelled"
                }
            );
        }
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
    let token = CancellationToken::new();
    let mut response = provider
        .stream_response(
            request(basic(true)),
            RequestContext {
                cancellation: token.clone(),
                ..RequestContext::default()
            },
        )
        .await
        .unwrap();
    fixture.captured().await;
    assert!(response.events.next().await.unwrap().is_ok());
    token.cancel();
    assert_eq!(
        response.events.next().await.unwrap().unwrap_err().code,
        "provider_cancelled"
    );
    assert!(response.events.next().await.is_none());
    fixture.disconnected().await;
    provider
        .create_response(request(basic(false)), RequestContext::default())
        .await
        .unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn dropped_stream_closes_native_socket_and_releases_shared_slot() {
    let mut stalled = Reply::stream(created());
    stalled.stall = 2;
    let mut fixture = Fixture::start(vec![stalled, Reply::json(native())]).await;
    let (broker, _) = fixture_broker(Some(KEY));
    let provider = fixture.provider(
        broker,
        vec![metadata("fixture", "native-fixture")],
        limits(),
    );
    let response = provider
        .stream_response(request(basic(true)), RequestContext::default())
        .await
        .unwrap();
    fixture.captured().await;
    assert_eq!(
        provider
            .create_response(request(basic(false)), RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "provider_busy"
    );
    drop(response);
    fixture.disconnected().await;
    provider
        .create_response(request(basic(false)), RequestContext::default())
        .await
        .unwrap();
}

#[tokio::test]
async fn unexpected_native_tool_events_are_not_delivered_and_close_sockets() {
    for item in [
        json!({"type":"function_call","name":"danger","arguments":"{}","call_id":"c"}),
        json!({"type":"mcp_call","name":"remote"}),
        json!({"type":"web_search_call","action":{"type":"search"}}),
    ] {
        let mut wire = native();
        wire["output"] = json!([item.clone()]);
        let fixture = Fixture::start(vec![Reply::json(wire)]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        assert_eq!(
            fixture
                .provider(
                    broker,
                    vec![metadata("fixture", "native-fixture")],
                    limits()
                )
                .create_response(request(basic(false)), RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            "qwen_unexpected_tool"
        );
        let mut bad = Reply::stream(
            created()
                + &event(json!({"type":"response.output_item.added","item":item,"output_index":0})),
        );
        bad.stall = 2;
        let mut fixture = Fixture::start(vec![bad, Reply::json(native())]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        let provider = fixture.provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        );
        let mut reply = provider
            .stream_response(request(basic(true)), RequestContext::default())
            .await
            .unwrap();
        fixture.captured().await;
        assert!(reply.events.next().await.unwrap().is_ok());
        assert_eq!(
            reply.events.next().await.unwrap().unwrap_err().code,
            "qwen_unexpected_tool"
        );
        assert!(reply.events.next().await.is_none());
        fixture.disconnected().await;
        provider
            .create_response(request(basic(false)), RequestContext::default())
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn http_errors_turn_state_and_terminal_semantics_are_not_success() {
    for (status, code) in [
        (401, "provider_authentication_failed"),
        (429, "provider_rate_limited"),
        (503, "provider_unavailable"),
        (302, "provider_redirect_blocked"),
    ] {
        let mut reply = Reply::json(json!({"message":KEY}));
        reply.status = status;
        reply.headers = "Retry-After: 3\r\nLocation: https://untrusted.example/\r\n".into();
        let fixture = Fixture::start(vec![reply]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        let provider = fixture.provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        );
        let e = provider
            .create_response(request(basic(false)), RequestContext::default())
            .await
            .err()
            .unwrap();
        assert_eq!(e.code, code);
        assert!(!e.to_string().contains(KEY));
        assert_eq!(
            e.retry_after_seconds,
            if status == 429 { Some(3) } else { None }
        );
        assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
    }
    for streaming in [false, true] {
        let mut reply = if streaming {
            Reply::stream(created() + &terminal(native()))
        } else {
            Reply::json(native())
        };
        reply.headers = "X-Codex-Turn-State: unbound\r\n".into();
        let fixture = Fixture::start(vec![reply]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        let provider = fixture.provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        );
        let e = if streaming {
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
        assert_eq!(e.code, "qwen_unsupported_turn_state");
    }
    for (status, error) in [
        ("incomplete", Value::Null),
        ("failed", json!({"message":KEY})),
    ] {
        let mut wire = native();
        wire["status"] = status.into();
        wire["error"] = error;
        let fixture = Fixture::start(vec![Reply::json(wire)]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        let p = fixture.provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        );
        let response = p
            .create_response(request(basic(false)), RequestContext::default())
            .await
            .unwrap();
        assert_eq!(response.response.wire()["status"], status);
        assert!(!response.response.wire()["error"].to_string().contains(KEY));
    }
    let fixture = Fixture::start(vec![Reply::stream(created() + "data: [DONE]\n\n")]).await;
    let (broker, _) = fixture_broker(Some(KEY));
    let p = fixture.provider(
        broker,
        vec![metadata("fixture", "native-fixture")],
        limits(),
    );
    let mut response = p
        .stream_response(request(basic(true)), RequestContext::default())
        .await
        .unwrap();
    let mut failed = false;
    while let Some(event) = response.events.next().await {
        match event {
            Err(error) => {
                assert_eq!(error.code, "provider_invalid_stream");
                failed = true;
            }
            Ok(ProviderStreamEvent::Model(event)) => assert!(event.response.terminal().is_none()),
            Ok(ProviderStreamEvent::Heartbeat) => (),
        }
    }
    assert!(failed);
}

#[tokio::test]
async fn gateway_uses_executor_key_and_keeps_local_gateway_token_out_of_native_requests() {
    let mut fixture = Fixture::start(vec![Reply::stream(created() + &terminal(native()))]).await;
    let (broker, _) = fixture_broker(Some(KEY));
    let p = fixture.provider(
        broker.clone(),
        vec![metadata("fixture", "native-fixture")],
        limits(),
    );
    let gateway =
        caidex_model_gateway::start_with_provider(Arc::new(p), broker.redactor(), limits())
            .await
            .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let r = client
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .body(basic(true).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let text = r.text().await.unwrap();
    assert!(text.contains("response.completed"));
    assert!(!text.contains(KEY));
    assert!(!text.contains(gateway.token().expose()));
    let captured = fixture.captured().await;
    assert_eq!(
        captured.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert!(!captured.headers.contains(gateway.token().expose()));
    assert_eq!(captured.body.unwrap()["store"], false);
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn catalog_pagination_shares_one_absolute_deadline_instead_of_renewing_it() {
    let mut first = Reply::json(page(
        21,
        1,
        (0..20).map(|i| model(&format!("m{i}"))).collect(),
    ));
    let mut second = Reply::json(page(21, 2, vec![model("native-fixture")]));
    first.delay = Duration::from_millis(1200);
    second.delay = Duration::from_millis(1200);
    let fixture = Fixture::start(vec![first, second]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let mut bound = limits();
    bound.total_timeout = Duration::from_secs(2);
    let provider = fixture.provider(broker, vec![metadata("fixture", "native-fixture")], bound);
    assert_eq!(
        provider
            .discover_models(RequestContext::default())
            .await
            .unwrap_err()
            .code,
        "provider_timeout"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn shared_metadata_query_encodes_values_and_preserves_endpoint_security() {
    use caidex_provider_custom::{ConfiguredModel, CustomResponses, CustomResponsesProvider};
    let mut fixture = Fixture::start(vec![Reply::json(catalog())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let endpoint = CustomResponses::new(
        &format!("{}/api/v1/models", fixture.base),
        Some(reference()),
    )
    .unwrap();
    let route = ConfiguredModel::new(
        "fixture".into(),
        "native-fixture".into(),
        vec![ResponsesDialect::Classic],
        CustomResponses::new(
            &format!("{}/compatible-mode/v1/responses", fixture.base),
            Some(reference()),
        )
        .unwrap(),
    )
    .unwrap();
    let provider = CustomResponsesProvider::new(vec![route], broker, limits()).unwrap();
    let value = provider
        .get_json_with_query(
            &endpoint,
            &[("cursor", "a b&other=?#🙂")],
            RequestContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(value, catalog());
    let captured = fixture.captured().await;
    assert!(
        captured
            .headers
            .starts_with("GET /proxy/api/v1/models?cursor=a+b%26other%3D%3F%23%F0%9F%99%82 ")
    );
    assert!(captured.body.is_none());
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert!(
        CustomResponses::new(
            &format!("{}?api_key=forbidden", fixture.base),
            Some(reference())
        )
        .is_err()
    );
}

#[tokio::test]
async fn declared_capability_limits_and_incomplete_output_messages_refuse_before_authentication() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    for streaming in [false, true] {
        let (broker, reads) = fixture_broker(Some(KEY));
        let mut model = metadata("fixture", "native-fixture");
        if streaming {
            model.capabilities.streaming = CapabilitySupport::Unsupported;
        } else {
            model.capabilities.text = CapabilitySupport::Unsupported;
        }
        let p = fixture.provider(broker, vec![model], limits());
        let error = if streaming {
            p.stream_response(request(basic(true)), RequestContext::default())
                .await
                .err()
                .unwrap()
        } else {
            p.create_response(request(basic(false)), RequestContext::default())
                .await
                .err()
                .unwrap()
        };
        assert_eq!(
            error.code,
            if streaming {
                "unsupported_streaming"
            } else {
                "unsupported_text"
            }
        );
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }
    let (broker, reads) = fixture_broker(Some(KEY));
    let p = fixture.provider(
        broker,
        vec![metadata("fixture", "native-fixture")],
        limits(),
    );
    for item in [
        json!({"type":"message","role":"assistant","id":"old","content":[{"type":"output_text","text":"x"}]}),
        json!({"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"x"}]}),
        json!({"type":"message","role":"assistant","id":"old","status":"completed","content":"x"}),
    ] {
        let mut wire = basic(false);
        wire["input"] = json!([item]);
        assert_eq!(
            p.create_response(request(wire), RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            "qwen_invalid_request"
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}
