//! Offline native HTTP/SSE; synthetic executor key and isolated loopback only.
use caidex_credentials::{Broker, CredentialRef, Id, Secret, SecretKind, SecretStore};
use caidex_model_core::{
    CancellationToken, CanonicalRequest, CapabilitySupport, ContextHeaders, CredentialRequirement,
    EvidenceSource, ModelMetadata, ModelProvider, ProviderStreamEvent, REQUEST_HEADERS,
    RequestContext, ResponsesDialect,
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

fn runtime_context() -> RequestContext {
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
fn runtime_request(streaming: bool) -> Value {
    json!({"model":"fixture","input":[
        {"role":"developer","content":[{"type":"input_text","text":"Priority 中文🙂"}]},
        {"role":"user","content":"Question"},
        {"role":"developer","content":"Next-turn priority"}],
        "instructions":"Original instructions","stream":streaming,
        "client_metadata":{"session_id":"LOCAL_BODY_SESSION","future":"LOCAL_EXTENSION"},
        "prompt_cache_key":"LOCAL_CACHE", "text":{"verbosity":"low","format":{"type":"text"}},
        "reasoning":{"effort":"high"}})
}

#[tokio::test]
async fn explicit_runtime_controls_preserve_developer_priority_and_native_json_sse() {
    for streaming in [false, true] {
        let reply = if streaming {
            Reply::stream(created() + &terminal(native()))
        } else {
            Reply::json(native())
        };
        let mut fixture = Fixture::start(vec![reply]).await;
        let (broker, reads) = fixture_broker(Some(KEY));
        let p = fixture
            .provider(
                broker,
                vec![metadata("fixture", "native-fixture")],
                limits(),
            )
            .with_runtime_context()
            .with_verbosity_instruction("low".into(), "Executor concise guidance".into())
            .unwrap()
            .with_reasoning_effort_mapping("fixture".into(), "high".into(), "xhigh".into())
            .unwrap()
            .with_reasoning_effort_mapping("fixture".into(), "xhigh".into(), "none".into())
            .unwrap();
        let source = runtime_request(streaming);
        let mut nullable = source.clone();
        for key in ["instructions", "client_metadata", "prompt_cache_key"] {
            nullable[key] = Value::Null;
        }
        nullable["text"]["verbosity"] = Value::Null;
        for source in [source, nullable] {
            if streaming {
                let mut response = p
                    .stream_response(request(source.clone()), runtime_context())
                    .await
                    .unwrap();
                let mut end = None;
                while let Some(e) = response.events.next().await {
                    if let ProviderStreamEvent::Model(e) = e.unwrap()
                        && e.response.terminal().is_some()
                    {
                        end = Some(e.response.wire()["response"].clone());
                    }
                }
                assert_eq!(end, Some(native()));
            } else {
                assert_eq!(
                    *p.create_response(request(source.clone()), runtime_context())
                        .await
                        .unwrap()
                        .response
                        .wire(),
                    native()
                );
            }
            let captured = fixture.captured().await;
            assert!(!captured.headers.contains("LOCAL_"));
            let mut expected = source.clone();
            expected["model"] = "native-fixture".into();
            expected["store"] = false.into();
            expected["reasoning"]["effort"] = "xhigh".into();
            for key in ["client_metadata", "prompt_cache_key", "text"] {
                expected.as_object_mut().unwrap().remove(key);
            }
            if !source["text"]["verbosity"].is_null() {
                expected["instructions"] =
                    "Original instructions\nExecutor concise guidance".into();
            }
            assert_eq!(captured.body, Some(expected));
            assert_eq!(
                captured.header("authorization"),
                Some(format!("Bearer {KEY}").as_str())
            );
        }
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn reasoning_mapping_is_per_model_and_all_native_levels_are_explicit() {
    let mut fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let mut p = fixture.provider(
        broker,
        vec![
            metadata("fixture", "native-fixture"),
            metadata("other", "native-other"),
        ],
        limits(),
    );
    for level in ["none", "minimal", "low", "medium", "high", "xhigh", "max"] {
        p = p
            .with_reasoning_effort_mapping("fixture".into(), level.into(), level.into())
            .unwrap();
    }
    p = p
        .with_reasoning_effort_mapping("other".into(), "high".into(), "low".into())
        .unwrap();
    for level in ["none", "minimal", "low", "medium", "high", "xhigh", "max"] {
        let mut source = basic(false);
        source["reasoning"] = json!({"effort":level});
        p.create_response(request(source), RequestContext::default())
            .await
            .unwrap();
        assert_eq!(
            fixture.captured().await.body.unwrap()["reasoning"],
            json!({"effort":level})
        );
    }
    let source = json!({"model":"other","input":"Other route","reasoning":{"effort":"high"}});
    p.create_response(request(source.clone()), RequestContext::default())
        .await
        .unwrap();
    let sent = fixture.captured().await.body.unwrap();
    assert_eq!(sent["model"], "native-other");
    assert_eq!(sent["reasoning"]["effort"], "low");
    let mut unmapped = source;
    unmapped["reasoning"]["effort"] = "max".into();
    assert!(
        p.create_response(request(unmapped), RequestContext::default())
            .await
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 8);
    let mut meta = metadata("fixture", "native-fixture");
    meta.capabilities.reasoning = CapabilitySupport::Unsupported;
    let (broker, reads) = fixture_broker(Some(KEY));
    let p = fixture
        .provider(broker, vec![meta], limits())
        .with_reasoning_effort_mapping("fixture".into(), "high".into(), "high".into())
        .unwrap()
        .with_reasoning_effort_mapping("fixture".into(), "none".into(), "none".into())
        .unwrap();
    assert!(
        p.create_response(
            request(json!({"model":"fixture","input":"x","reasoning":{"effort":"high"}})),
            RequestContext::default()
        )
        .await
        .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    p.create_response(
        request(json!({"model":"fixture","input":"x","reasoning":{"effort":"none"}})),
        RequestContext::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        fixture.captured().await.body.unwrap()["reasoning"]["effort"],
        "none"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn bad_runtime_controls_and_partial_policies_never_read_keys_or_post() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let make = || {
        fixture.provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
    };
    let enabled = || {
        make()
            .with_runtime_context()
            .with_verbosity_instruction("low".into(), "Guidance".into())
            .unwrap()
            .with_reasoning_effort_mapping("fixture".into(), "high".into(), "xhigh".into())
            .unwrap()
    };
    for p in [
        make(),
        make().with_runtime_context(),
        make()
            .with_verbosity_instruction("low".into(), "Guidance".into())
            .unwrap(),
        make()
            .with_reasoning_effort_mapping("fixture".into(), "high".into(), "xhigh".into())
            .unwrap(),
    ] {
        assert!(
            p.create_response(request(runtime_request(false)), runtime_context())
                .await
                .is_err()
        );
    }
    let p = enabled();
    for (key, value) in [
        ("client_metadata", json!([])),
        ("client_metadata", json!({"nested":{}})),
        ("prompt_cache_key", json!("\n")),
        ("text", json!({"verbosity":"unknown"})),
        ("text", json!({"verbosity":1})),
        ("text", json!({"format":{"type":"json_schema","schema":{}}})),
        ("text", json!({"format":{"type":"text","future":true}})),
        ("text", json!({"future":true})),
        ("reasoning", json!({"effort":"ultra"})),
        ("reasoning", json!({"effort":"medium"})),
        ("reasoning", json!({})),
        ("reasoning", json!({"effort":"high","summary":"auto"})),
        ("reasoning", json!({"context":"all_turns"})),
        ("include", json!(["reasoning.encrypted_content"])),
        ("enable_thinking", json!(true)),
        ("thinking_budget", json!(500)),
        ("store", json!(true)),
        ("previous_response_id", json!("old")),
        ("tools", json!([])),
        ("metadata", json!({})),
    ] {
        for streaming in [false, true] {
            let mut source = runtime_request(streaming);
            source[key] = value.clone();
            let error = if streaming {
                p.stream_response(request(source), runtime_context())
                    .await
                    .err()
                    .unwrap()
            } else {
                p.create_response(request(source), runtime_context())
                    .await
                    .err()
                    .unwrap()
            };
            assert_eq!(error.http_status, 400, "{key}");
            assert!(!error.to_string().contains(KEY));
        }
    }
    let mut context = runtime_context();
    context
        .headers
        .insert("x-codex-turn-state", "UNBOUND".into(), REQUEST_HEADERS)
        .unwrap();
    assert!(
        p.create_response(request(runtime_request(false)), context)
            .await
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn executor_control_configuration_refuses_unknown_duplicate_or_empty_mappings() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let make = || {
        fixture.provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
    };
    for (model, source, target) in [
        ("unknown", "high", "high"),
        ("fixture", "ultra", "high"),
        ("fixture", "high", "ultra"),
        ("fixture", "", "high"),
    ] {
        assert!(
            make()
                .with_reasoning_effort_mapping(model.into(), source.into(), target.into())
                .is_err()
        );
    }
    assert!(
        make()
            .with_reasoning_effort_mapping("fixture".into(), "high".into(), "xhigh".into())
            .unwrap()
            .with_reasoning_effort_mapping("fixture".into(), "high".into(), "high".into())
            .is_err()
    );
    for (level, text) in [("low", ""), ("medium", " "), ("unknown", "Guidance")] {
        assert!(
            make()
                .with_verbosity_instruction(level.into(), text.into())
                .is_err()
        );
    }
    assert!(
        make()
            .with_verbosity_instruction("low".into(), "One".into())
            .unwrap()
            .with_verbosity_instruction("low".into(), "Two".into())
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn original_and_compiled_runtime_budgets_refuse_before_authentication() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let budget = Limits {
        request_bytes: 512,
        ..limits()
    };
    let p = fixture
        .provider(broker, vec![metadata("fixture", "native-fixture")], budget)
        .with_runtime_context()
        .with_verbosity_instruction("low".into(), "Guide".repeat(300))
        .unwrap();
    let mut oversized = basic(false);
    oversized["client_metadata"] = json!({"data":"x".repeat(1024)});
    let mut expanded = basic(false);
    expanded["text"] = json!({"verbosity":"low"});
    for source in [oversized, expanded] {
        let error = p
            .create_response(request(source), runtime_context())
            .await
            .err()
            .unwrap();
        assert_eq!(error.http_status, 413);
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn gateway_runtime_controls_and_catalog_attribution_stay_executor_local() {
    let mut fixture = Fixture::start(vec![
        Reply::json(catalog()),
        Reply::stream(created() + &terminal(native())),
    ])
    .await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let p = fixture
        .provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_runtime_context()
        .with_verbosity_instruction("low".into(), "Guidance".into())
        .unwrap()
        .with_reasoning_effort_mapping("fixture".into(), "high".into(), "xhigh".into())
        .unwrap();
    assert_eq!(p.discover_models(runtime_context()).await.unwrap().len(), 1);
    assert!(!fixture.captured().await.headers.contains("LOCAL_"));
    let gateway =
        caidex_model_gateway::start_with_provider(Arc::new(p), broker.redactor(), limits())
            .await
            .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut r = client
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .body(runtime_request(true).to_string());
    for name in ["session_id", "x-client-request-id", "x-codex-turn-metadata"] {
        r = r.header(name, format!("LOCAL_{name}"));
    }
    let response = r.send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert!(
        response
            .text()
            .await
            .unwrap()
            .contains("response.completed")
    );
    let captured = fixture.captured().await;
    assert!(!captured.headers.contains("LOCAL_"));
    assert!(!captured.headers.contains(gateway.token().expose()));
    let body = captured.body.unwrap();
    assert_eq!(body["reasoning"]["effort"], "xhigh");
    assert!(body.get("client_metadata").is_none());
    assert!(body.get("text").is_none());
    assert_eq!(body["instructions"], "Original instructions\nGuidance");
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn consumed_context_retains_cancellation_deadline_and_stream_socket_lifetime() {
    let mut reply = Reply::stream(created());
    reply.stall = 2;
    let mut fixture = Fixture::start(vec![reply]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let p = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_runtime_context();
    for expired in [false, true] {
        let mut context = runtime_context();
        if expired {
            context.deadline = Some(std::time::Instant::now() - Duration::from_millis(1));
        } else {
            context.cancellation.cancel();
        }
        let error = p
            .stream_response(request(basic(true)), context)
            .await
            .err()
            .unwrap();
        assert_eq!(
            error.code,
            if expired {
                "provider_timeout"
            } else {
                "provider_cancelled"
            }
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let cancellation = CancellationToken::new();
    let mut context = runtime_context();
    context.cancellation = cancellation.clone();
    let mut response = p
        .stream_response(request(basic(true)), context)
        .await
        .unwrap();
    response.events.next().await.unwrap().unwrap();
    fixture.captured().await;
    cancellation.cancel();
    assert_eq!(
        response.events.next().await.unwrap().err().unwrap().code,
        "provider_cancelled"
    );
    fixture.disconnected().await;
    let mut response = p
        .stream_response(request(basic(true)), runtime_context())
        .await
        .unwrap();
    response.events.next().await.unwrap().unwrap();
    fixture.captured().await;
    drop(response);
    fixture.disconnected().await;
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn native_summary_history_roundtrips_full_wire_and_compiled_prefix() {
    let mut first = native();
    first["output"][2]["id"] = json!(18446744073709551616_u128);
    first["output"][2]["status"] = "future_phase".into();
    first["output"][0]["summary"] = json!([
        {"type":"summary_text","text":"First ","future":{"n":18446744073709551616_u128}},
        {"type":"summary_text","text":"second"}
    ]);
    let mut fixture = Fixture::start(vec![Reply::json(first.clone()), Reply::json(native())]).await;
    let (broker, _) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_native_history();
    let source = json!({"model":"fixture","input":[{"role":"developer","content":"Keep the rules"},{"role":"user","content":"Start"}],"instructions":"Original instructions"});
    let response = provider
        .create_response(request(source.clone()), RequestContext::default())
        .await
        .unwrap();
    let sent = fixture.captured().await.body.unwrap();
    let carrier = &response.response.output()[0];
    let encoded = carrier["encrypted_content"]
        .as_str()
        .expect("bound native carrier");
    let capsule: Value = serde_json::from_str(
        encoded
            .strip_prefix("caidex.qwen.native-history.v1:")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(capsule["response"], first);
    assert_eq!(capsule["request"], sent);
    assert_eq!(
        carrier["summary"],
        json!([{"type":"summary_text","text":"First second"}])
    );
    assert!(!encoded.contains(KEY));
    let mut followup = source;
    let input = followup["input"].as_array_mut().unwrap();
    input.extend(
        serde_json::from_str::<Vec<Value>>(
            &serde_json::to_string(response.response.output()).unwrap(),
        )
        .unwrap(),
    );
    input.push(json!({"role":"user","content":"Next"}));
    provider
        .create_response(request(followup), RequestContext::default())
        .await
        .unwrap();
    let second = fixture.captured().await.body.unwrap();
    let mut expected = sent["input"].as_array().unwrap().clone();
    expected.extend(first["output"].as_array().unwrap().clone());
    expected.push(json!({"role":"user","content":"Next"}));
    assert_eq!(second["input"], json!(expected));
    assert_eq!(second["store"], false);
    assert_eq!(second["model"], "native-fixture");
}

fn capsule(output: &[Value]) -> Value {
    serde_json::from_str(
        output[0]["encrypted_content"]
            .as_str()
            .unwrap()
            .strip_prefix("caidex.qwen.native-history.v1:")
            .unwrap(),
    )
    .unwrap()
}
fn followup(source: &Value, output: &[Value]) -> Value {
    let mut next = source.clone();
    if let Some(text) = next["input"].as_str() {
        next["input"] = json!([{"role":"user","content":text}]);
    }
    next["input"]
        .as_array_mut()
        .unwrap()
        .extend(output.iter().cloned());
    next["input"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role":"user","content":"Follow-up"}));
    next
}
fn summary_events(response: &Value) -> Vec<Value> {
    let mut events = vec![
        json!({"type":"response.created","response":{"id":"fixture","status":"in_progress","output":[]}}),
    ];
    for (index, item) in response["output"].as_array().unwrap().iter().enumerate() {
        let mut added = item.clone();
        if item["type"] == "reasoning" {
            added["summary"] = json!([]);
        } else if item["type"] == "message" {
            added["content"] = json!([]);
        } else {
            continue;
        }
        added.as_object_mut().unwrap().remove("status");
        events.push(json!({"type":"response.output_item.added","output_index":index,"item":added}));
        if item["type"] == "reasoning" {
            let text: String = item["summary"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["text"].as_str().unwrap())
                .collect();
            events.push(json!({"type":"response.reasoning_text.delta","output_index":index,"item_id":item["id"],"delta":text}));
            events.push(json!({"type":"response.reasoning_text.done","output_index":index,"item_id":item["id"],"text":text}));
        } else {
            for (part, content) in item["content"].as_array().unwrap().iter().enumerate() {
                events.push(json!({"type":"response.content_part.added","output_index":index,"content_index":part,"item_id":item["id"],"part":{"type":"output_text","text":"","annotations":[]}}));
                events.push(json!({"type":"response.output_text.delta","output_index":index,"content_index":part,"item_id":item["id"],"delta":content["text"]}));
                events.push(json!({"type":"response.output_text.done","output_index":index,"content_index":part,"item_id":item["id"],"text":content["text"]}));
                events.push(json!({"type":"response.content_part.done","output_index":index,"content_index":part,"item_id":item["id"],"part":content}));
            }
        }
        events.push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
    }
    events.push(json!({"type":"response.completed","response":response}));
    for (index, event) in events.iter_mut().enumerate() {
        event["sequence_number"] = index.into();
    }
    events
}
fn events_body(events: &[Value]) -> String {
    events.iter().cloned().map(event).collect()
}

#[tokio::test]
async fn native_summary_stream_projects_flat_parts_and_replays_exact_raw_chunks() {
    for interleaved in [false, true] {
        let mut response = native();
        response["output"][2]["id"] = json!(18446744073709551616_u128);
        response["output"][2]["status"] = "future_phase".into();
        response["output"][0]["summary"] = json!([
            {"type":"summary_text","text":"First ","native_extension":18446744073709551616_u128},
            {"type":"summary_text","text":"second"}
        ]);
        response["output"].as_array_mut().unwrap().insert(2,json!({"type":"reasoning","id":"rs_two","summary":[{"type":"summary_text","text":"Third"}],"future":true}));
        let mut events = summary_events(&response);
        if interleaved {
            // Add/delta for the later reasoning before earlier output positions.
            let later: Vec<_> = events.drain(11..15).collect();
            events.splice(1..1, later);
        }
        events.insert(3,json!({"type":"response.future_metadata","output_index":2,"item_id":"rs_two","raw":18446744073709551616_u128}));
        for (index, event) in events.iter_mut().enumerate() {
            event["sequence_number"] = index.into();
        }
        let mut fixture = Fixture::start(vec![
            Reply::stream(events_body(&events)),
            Reply::json(native()),
        ])
        .await;
        let (broker, _) = fixture_broker(Some(KEY));
        let provider = fixture
            .provider(
                broker,
                vec![metadata("fixture", "native-fixture")],
                limits(),
            )
            .with_native_history();
        let source = basic(true);
        let mut stream = provider
            .stream_response(request(source.clone()), RequestContext::default())
            .await
            .unwrap();
        let mut shown = Vec::new();
        let mut last = None;
        while let Some(value) = stream.events.next().await {
            if let ProviderStreamEvent::Model(value) = value.unwrap() {
                if value.response.kind() == "response.completed" {
                    last = Some(value.response.wire()["response"].clone());
                }
                shown.push(value.response.wire().clone());
            }
        }
        let last = last.unwrap();
        let output = last["output"].as_array().unwrap();
        assert_eq!(
            output[0]["summary"],
            json!([{"type":"summary_text","text":"First second"},{"type":"summary_text","text":"Third"}])
        );
        let history = capsule(output);
        assert_eq!(history["response"], response);
        assert_eq!(history["chunks"], json!(events));
        assert_eq!(history["source"], "sse");
        let deltas: Vec<_> = shown
            .iter()
            .filter(|v| v["type"] == "response.reasoning_summary_text.delta")
            .collect();
        assert_eq!(deltas.len(), 2);
        for delta in deltas {
            assert_eq!(delta["output_index"], 0);
            let i = delta["summary_index"].as_u64().unwrap() as usize;
            assert_eq!(delta["delta"], output[0]["summary"][i]["text"]);
            assert_eq!(delta["item_id"], output[0]["id"]);
        }
        assert!(
            shown
                .iter()
                .filter(|v| v["type"] == "response.output_text.delta")
                .all(|v| v["output_index"] == 1 && v["content_index"] == 0)
        );
        let carrier_done = shown
            .iter()
            .position(|v| !v["item"]["encrypted_content"].is_null())
            .unwrap();
        assert_eq!(shown[carrier_done]["type"], "response.output_item.done");
        assert!(
            shown[..carrier_done]
                .iter()
                .all(|v| v["item"]["encrypted_content"].is_null())
        );
        assert!(
            shown[..carrier_done]
                .iter()
                .any(|v| v["type"] == "response.output_text.delta")
        );
        let mut parser = caidex_model_core::ResponsesStream::new(limits().frame_bytes).unwrap();
        parser.push(events_body(&shown).as_bytes()).unwrap();
        assert_eq!(
            parser.finish().unwrap(),
            caidex_model_core::StreamState::Completed
        );
        let sent = fixture.captured().await.body.unwrap();
        let mut next = followup(&source, output);
        next["stream"] = false.into();
        provider
            .create_response(request(next), RequestContext::default())
            .await
            .unwrap();
        let restored = fixture.captured().await.body.unwrap();
        let mut expected = vec![json!({"role":"user","content":source["input"]})];
        expected.extend(response["output"].as_array().unwrap().clone());
        expected.push(json!({"role":"user","content":"Follow-up"}));
        assert_eq!(restored["input"], json!(expected));
        assert_eq!(history["request"], sent);
    }
}

#[tokio::test]
async fn summary_history_scope_prefix_and_complete_group_are_checked_before_keys() {
    let mut fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_native_history();
    let source = json!({"model":"fixture","input":[{"role":"developer","content":"Priority"},{"role":"user","content":"Start"}],"instructions":"Original"});
    let first = provider
        .create_response(request(source.clone()), RequestContext::default())
        .await
        .unwrap();
    fixture.captured().await;
    let good = followup(&source, first.response.output());
    let initial_reads = reads.load(Ordering::SeqCst);
    let mut bad = Vec::new();
    let mut v = good.clone();
    v["instructions"] = "Changed".into();
    bad.push(v);
    let mut v = good.clone();
    v["input"][0]["content"] = "Changed".into();
    bad.push(v);
    let mut v = good.clone();
    v["input"][2]["summary"][0]["text"] = "Changed".into();
    bad.push(v);
    let mut v = good.clone();
    v["input"][4]["n"] = 1.into();
    bad.push(v);
    let mut v = good.clone();
    v["input"].as_array_mut().unwrap().remove(3);
    bad.push(v);
    let mut v = good.clone();
    v["input"].as_array_mut().unwrap().swap(3, 4);
    bad.push(v);
    let mut v = good.clone();
    v["input"][2]["status"] = "incomplete".into();
    bad.push(v);
    let mut v = good.clone();
    v["input"][2]["id"] = "\n".into();
    bad.push(v);
    for mutation in [
        "version",
        "scope",
        "native_model",
        "source",
        "response",
        "request",
    ] {
        let mut v = good.clone();
        let mut history = capsule(first.response.output());
        match mutation {
            "version" => history["version"] = 2.into(),
            "scope" => history["scope"]["credential"]["profile"] = "other".into(),
            "native_model" => history["native_model"] = "other".into(),
            "source" => history["source"] = "other".into(),
            "response" => history["response"]["output"][0]["summary"][0]["text"] = "Changed".into(),
            "request" => history["request"]["store"] = true.into(),
            _ => unreachable!(),
        }
        v["input"][2]["encrypted_content"] =
            format!("caidex.qwen.native-history.v1:{history}").into();
        bad.push(v);
    }
    for v in bad {
        let error = provider
            .create_response(request(v), RequestContext::default())
            .await
            .err()
            .unwrap();
        assert_eq!(error.http_status, 400);
        assert_eq!(reads.load(Ordering::SeqCst), initial_reads);
    }
    for (base, r, native) in [
        (
            format!("{}/other", fixture.base),
            reference(),
            "native-fixture",
        ),
        (
            fixture.base.clone(),
            CredentialRef {
                profile: Id::new("other").unwrap(),
                ..reference()
            },
            "native-fixture",
        ),
        (fixture.base.clone(), reference(), "other-native"),
    ] {
        let other = QwenProvider::new(
            QwenConfig::new(&base, r).unwrap(),
            vec![metadata("fixture", native)],
            broker.clone(),
            limits(),
        )
        .unwrap()
        .with_native_history();
        assert_eq!(
            other
                .create_response(request(good.clone()), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            400
        );
        assert_eq!(reads.load(Ordering::SeqCst), initial_reads);
    }
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn native_history_never_remaps_previous_effort_or_drops_compiled_instructions() {
    let mut fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, _) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_runtime_context()
        .with_native_history()
        .with_verbosity_instruction("low".into(), "Be brief".into())
        .unwrap()
        .with_reasoning_effort_mapping("fixture".into(), "high".into(), "xhigh".into())
        .unwrap()
        .with_reasoning_effort_mapping("fixture".into(), "xhigh".into(), "none".into())
        .unwrap();
    let mut source = runtime_request(false);
    source["reasoning"] = json!({"effort":"high"});
    source["text"]["verbosity"] = "low".into();
    let first = provider
        .create_response(request(source.clone()), runtime_context())
        .await
        .unwrap();
    let sent = fixture.captured().await;
    assert_eq!(sent.body.as_ref().unwrap()["reasoning"]["effort"], "xhigh");
    assert_eq!(sent.header("session_id"), None);
    let old = capsule(first.response.output());
    assert_eq!(
        old["request"]["instructions"],
        sent.body.unwrap()["instructions"]
    );
    assert_eq!(old["request"]["reasoning"]["effort"], "xhigh");
    assert!(old["request"].get("client_metadata").is_none());
    let mut second = followup(&source, first.response.output());
    second["reasoning"]["effort"] = "xhigh".into();
    let response = provider
        .create_response(request(second.clone()), runtime_context())
        .await
        .unwrap();
    let sent = fixture.captured().await.body.unwrap();
    assert_eq!(sent["reasoning"]["effort"], "none");
    assert_eq!(sent["instructions"], old["request"]["instructions"]);
    assert_eq!(capsule(response.response.output())["request"], sent);
    let third = followup(&second, response.response.output());
    provider
        .create_response(request(third), runtime_context())
        .await
        .unwrap();
    let third = fixture.captured().await.body.unwrap();
    assert_eq!(
        third["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v["type"] == "reasoning")
            .count(),
        2
    );
    assert!(!third.to_string().contains("caidex.qwen.native-history"));
}

#[tokio::test]
async fn inconsistent_summary_streams_never_publish_carriers_and_release_slots() {
    for case in 0..21 {
        let mut events = summary_events(&native());
        match case {
            0 => events[2]["delta"] = "Wrong".into(),
            1 => events[3]["text"] = "Wrong".into(),
            2 => events[4]["item"]["summary"][0]["text"] = "Wrong".into(),
            3 => events[1]["item"]["id"] = "Wrong".into(),
            4 => events[2]["item_id"] = "Wrong".into(),
            5 => events[2]["output_index"] = 100.into(),
            6 => {
                events.insert(4, events[2].clone());
            }
            7 => events[4]["output_index"] = 1.into(),
            8 => {
                events.remove(0);
            }
            9 => {
                events.insert(1, events[0].clone());
            }
            10 => events[2]["content_index"] = 1.into(),
            11 => events[7]["item_id"] = "rs_one".into(),
            12 => events[9]["part"]["text"] = "Wrong".into(),
            13 => {
                events.insert(7, events[6].clone());
            }
            14 => events.last_mut().unwrap()["response"]["model"] = "other-native".into(),
            15 => {
                events.last_mut().unwrap()["response"]["output"][0]["encrypted_content"] =
                    "opaque".into()
            }
            16 => {
                events.last_mut().unwrap()["response"]["output"].as_array_mut().unwrap().push(json!({"type":"function_call","id":"fc","call_id":"call","name":"shell","arguments":"{}"}));
            }
            17 => {
                events.pop();
            }
            18 => {
                events.insert(4, events[4].clone());
            }
            19 => events[5]["item"]["id"] = "rs_fixture_native".into(),
            20 => events[5]["item"]["id"] = "rs_one".into(),
            _ => unreachable!(),
        }
        for (i, e) in events.iter_mut().enumerate() {
            e["sequence_number"] = i.into();
        }
        let mut broken = Reply::stream(events_body(&events));
        broken.stall = 2;
        let mut fixture = Fixture::start(vec![broken, Reply::json(native())]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        let provider = fixture
            .provider(
                broker,
                vec![metadata("fixture", "native-fixture")],
                limits(),
            )
            .with_native_history();
        let mut response = provider
            .stream_response(request(basic(true)), RequestContext::default())
            .await
            .unwrap();
        let mut failed = false;
        while let Some(e) = response.events.next().await {
            match e {
                Err(error) => {
                    assert!(error.http_status >= 500, "case {case}");
                    failed = true;
                    break;
                }
                Ok(ProviderStreamEvent::Model(e)) => {
                    assert_ne!(e.response.kind(), "response.completed", "case {case}");
                    assert!(
                        e.response.wire()["item"]["encrypted_content"].is_null(),
                        "case {case}"
                    );
                    assert!(
                        e.response.wire()["item"]["type"] != "function_call",
                        "case {case}"
                    );
                }
                _ => (),
            }
        }
        assert!(failed, "case {case}");
        assert!(response.events.next().await.is_none());
        fixture.captured().await;
        fixture.disconnected().await;
        let valid = provider
            .create_response(request(basic(false)), RequestContext::default())
            .await
            .unwrap();
        assert_eq!(valid.response.wire()["status"], "completed");
        fixture.captured().await;
    }
}

#[tokio::test]
async fn incremental_summary_display_remains_cancellable_and_droppable_before_terminal() {
    let events = summary_events(&native());
    let mut stalled = Reply::stream(events_body(&events[..3]));
    stalled.stall = 2;
    let mut fixture = Fixture::start(vec![stalled]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_native_history();
    for action in ["cancel", "deadline", "drop"] {
        let cancellation = CancellationToken::new();
        let context = RequestContext {
            cancellation: cancellation.clone(),
            deadline: (action == "deadline")
                .then(|| std::time::Instant::now() + Duration::from_millis(300)),
            headers: ContextHeaders::default(),
        };
        let mut response = provider
            .stream_response(request(basic(true)), context)
            .await
            .unwrap();
        loop {
            let next = tokio::time::timeout(WAIT, response.events.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            if let ProviderStreamEvent::Model(next) = next {
                assert!(next.response.wire()["item"]["encrypted_content"].is_null());
                if next.response.kind() == "response.reasoning_summary_text.delta" {
                    assert_eq!(next.response.wire()["delta"], "Native thinking");
                    break;
                }
            }
        }
        fixture.captured().await;
        if action == "drop" {
            drop(response);
        } else {
            if action == "cancel" {
                cancellation.cancel();
            } else {
                tokio::time::sleep(Duration::from_millis(310)).await;
            }
            let error = response.events.next().await.unwrap().err().unwrap();
            assert_eq!(
                error.code,
                if action == "cancel" {
                    "provider_cancelled"
                } else {
                    "provider_timeout"
                }
            );
            assert!(response.events.next().await.is_none());
        }
        fixture.disconnected().await;
    }
    assert_eq!(reads.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn history_policy_keeps_unsupported_tools_and_unbound_items_before_authentication() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_native_history();
    for item in [
        json!({"type":"reasoning","id":"rs","summary":[]}),
        json!({"type":"function_call","id":"fc","call_id":"call","name":"shell","arguments":"{}"}),
        json!({"type":"future_item","n":18446744073709551616_u128}),
        json!({"role":"user","content":[{"type":"input_image","image_url":"https://example.com"}]}),
    ] {
        let mut source = basic(false);
        source["input"] = json!([item]);
        assert_eq!(
            provider
                .create_response(request(source), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            400
        );
    }
    for key in [
        "tools",
        "tool_choice",
        "parallel_tool_calls",
        "include",
        "previous_response_id",
    ] {
        let mut source = basic(false);
        source[key] = if key == "tools" {
            json!([])
        } else {
            true.into()
        };
        assert_eq!(
            provider
                .create_response(request(source), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            400
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn oversized_or_noncompleted_native_history_is_never_a_successful_carrier() {
    for streaming in [false, true] {
        for state in ["completed", "failed", "incomplete"] {
            let mut response = native();
            response["status"] = state.into();
            let reply = if streaming {
                Reply::stream(created() + &terminal(response.clone()))
            } else {
                Reply::json(response.clone())
            };
            let mut fixture = Fixture::start(vec![reply]).await;
            let (broker, _) = fixture_broker(Some(KEY));
            let mut limits = limits();
            if state == "completed" {
                limits.request_bytes = 900;
            }
            let provider = fixture
                .provider(broker, vec![metadata("fixture", "native-fixture")], limits)
                .with_native_history();
            if streaming {
                let mut stream = provider
                    .stream_response(request(basic(true)), RequestContext::default())
                    .await
                    .unwrap();
                let mut error = false;
                let mut terminal = None;
                while let Some(value) = stream.events.next().await {
                    match value {
                        Err(e) => {
                            assert_eq!(e.code, "qwen_history_too_large");
                            error = true;
                        }
                        Ok(ProviderStreamEvent::Model(e)) => {
                            assert!(e.response.wire()["item"]["encrypted_content"].is_null());
                            if e.response.terminal().is_some() {
                                terminal = Some(e.response.wire()["response"].clone());
                            }
                        }
                        _ => (),
                    }
                }
                if state == "completed" {
                    assert!(error);
                    assert!(terminal.is_none());
                } else {
                    assert!(!error);
                    assert_eq!(terminal, Some(response));
                }
            } else {
                let result = provider
                    .create_response(request(basic(false)), RequestContext::default())
                    .await;
                if state == "completed" {
                    assert_eq!(result.err().unwrap().code, "qwen_history_too_large");
                } else {
                    assert_eq!(*result.unwrap().response.wire(), response);
                }
            }
            fixture.captured().await;
        }
    }
}

#[tokio::test]
async fn native_history_refuses_role_escalation_identity_collisions_and_bad_summaries() {
    for streaming in [false, true] {
        for case in 0..9 {
            let mut response = native();
            match case {
                0 => response["output"][1]["role"] = "developer".into(),
                1 => response["output"][1]["id"] = "rs_fixture_native".into(),
                2 => response["output"][1]["id"] = "rs_one".into(),
                3 => response["output"][0]["summary"][0]["type"] = "reasoning_text".into(),
                4 => response["output"][0]["summary"][0]["text"] = false.into(),
                5 => response["output"][1]["content"][0]["type"] = "input_text".into(),
                6 => response["id"] = "\n".into(),
                7 => response["output"][1]["status"] = "incomplete".into(),
                8 => {
                    response["output"][0]["content"] =
                        json!([{"type":"reasoning_text","text":"Unbound"}])
                }
                _ => unreachable!(),
            }
            let reply = if streaming {
                Reply::stream(created() + &terminal(response))
            } else {
                Reply::json(response)
            };
            let fixture = Fixture::start(vec![reply]).await;
            let (broker, _) = fixture_broker(Some(KEY));
            let provider = fixture
                .provider(
                    broker,
                    vec![metadata("fixture", "native-fixture")],
                    limits(),
                )
                .with_native_history();
            if streaming {
                let mut stream = provider
                    .stream_response(request(basic(true)), RequestContext::default())
                    .await
                    .unwrap();
                let mut failed = false;
                while let Some(e) = stream.events.next().await {
                    match e {
                        Err(error) => {
                            assert_eq!(error.http_status, 502);
                            failed = true;
                        }
                        Ok(ProviderStreamEvent::Model(e)) => {
                            assert_ne!(e.response.kind(), "response.completed");
                            assert!(e.response.wire()["item"]["encrypted_content"].is_null());
                        }
                        _ => (),
                    }
                }
                assert!(failed, "case {case}");
            } else {
                assert_eq!(
                    provider
                        .create_response(request(basic(false)), RequestContext::default())
                        .await
                        .err()
                        .unwrap()
                        .http_status,
                    502,
                    "case {case}"
                );
            }
        }
    }
}

#[tokio::test]
async fn native_tool_search_and_mcp_outputs_are_not_display_only_extensions() {
    for history in [false, true] {
        for output in [
            json!({"type":"tool_search_output","id":"ts","status":"completed","execution":"server","tools":[]}),
            json!({"type":"mcp_list_tools","id":"mcp","server_label":"unknown","tools":[]}),
        ] {
            let mut wire = native();
            wire["output"].as_array_mut().unwrap().push(output);
            let fixture = Fixture::start(vec![
                Reply::json(wire.clone()),
                Reply::stream(created() + &terminal(wire)),
            ])
            .await;
            let (broker, _) = fixture_broker(Some(KEY));
            let mut provider = fixture.provider(
                broker,
                vec![metadata("fixture", "native-fixture")],
                limits(),
            );
            if history {
                provider = provider.with_native_history();
            }
            assert_eq!(
                provider
                    .create_response(request(basic(false)), RequestContext::default())
                    .await
                    .err()
                    .unwrap()
                    .code,
                "qwen_unexpected_tool"
            );
            let mut stream = provider
                .stream_response(request(basic(true)), RequestContext::default())
                .await
                .unwrap();
            let mut failed = false;
            while let Some(e) = stream.events.next().await {
                match e {
                    Err(error) => {
                        assert_eq!(error.code, "qwen_unexpected_tool");
                        failed = true;
                    }
                    Ok(ProviderStreamEvent::Model(e)) => {
                        assert_ne!(e.response.kind(), "response.completed");
                    }
                    _ => (),
                }
            }
            assert!(failed);
        }
    }
}

#[tokio::test]
async fn gateway_reuses_bound_summary_history_without_forwarding_listener_identity() {
    let events = summary_events(&native());
    let mut fixture = Fixture::start(vec![
        Reply::stream(events_body(&events)),
        Reply::json(native()),
    ])
    .await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_runtime_context()
        .with_native_history();
    let gateway =
        caidex_model_gateway::start_with_provider(Arc::new(provider), broker.redactor(), limits())
            .await
            .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let source = basic(true);
    let response = client
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .header("session_id", "LOCAL_GATEWAY_SESSION")
        .body(source.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let raw = response.text().await.unwrap();
    assert!(!raw.contains(KEY));
    assert!(!raw.contains(gateway.token().expose()));
    let mut parser = caidex_model_core::ResponsesStream::new(limits().frame_bytes).unwrap();
    let parsed = parser.push(raw.as_bytes()).unwrap();
    assert_eq!(
        parser.finish().unwrap(),
        caidex_model_core::StreamState::Completed
    );
    let end = parsed.last().unwrap().response.wire()["response"].clone();
    let first = fixture.captured().await;
    assert!(!first.headers.contains(gateway.token().expose()));
    assert!(!first.headers.contains("LOCAL_GATEWAY_SESSION"));
    let mut next = followup(&source, end["output"].as_array().unwrap());
    next["stream"] = false.into();
    let response = client
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .body(next.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
    assert_eq!(
        capsule(body["output"].as_array().unwrap())["response"],
        native()
    );
    let second = fixture.captured().await;
    assert_eq!(
        second.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert!(
        !second
            .body
            .unwrap()
            .to_string()
            .contains("caidex.qwen.native-history")
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    gateway.shutdown().await.unwrap();
}

fn tool_source(streaming: bool) -> Value {
    json!({"model":"fixture","input":[{"role":"developer","content":"Keep the rules"},{"role":"user","content":"Call tools"}],"stream":streaming,"tools":[
        {"type":"namespace","name":"alpha","description":"Alpha group","tools":[{"type":"function","name":"lookup","description":"Look up","parameters":{"type":"object","properties":{"q":{"type":"string"}},"required":["q"]}}]},
        {"type":"namespace","name":"beta","tools":[{"type":"function","name":"lookup","parameters":{"type":"object"},"strict":false,"defer_loading":false}]}
    ],"tool_choice":"auto","parallel_tool_calls":true})
}
fn tool_native() -> Value {
    let mut value = native();
    value["output"] = json!([
        value["output"][0],
        {"type":"function_call","id":"fc_alpha","call_id":"call_alpha","name":"caidex_ns_0","arguments":"{\"q\":\"中文🙂\"}","status":"completed","future":{"n":18446744073709551616_u128}},
        value["output"][1],
        {"type":"function_call","id":"fc_beta","call_id":"call_beta","name":"caidex_ns_1","arguments":"{}","status":"completed"}
    ]);
    value
}
#[tokio::test]
async fn native_function_namespaces_pair_results_and_replay_bound_tool_history() {
    let original = tool_native();
    let mut fixture =
        Fixture::start(vec![Reply::json(original.clone()), Reply::json(native())]).await;
    let (broker, _) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_native_tools();
    let source = tool_source(false);
    let response = provider
        .create_response(request(source.clone()), RequestContext::default())
        .await
        .unwrap();
    let sent = fixture.captured().await.body.unwrap();
    assert_eq!(sent["tools"][0]["name"], "caidex_ns_0");
    assert_eq!(sent["tools"][1]["name"], "caidex_ns_1");
    assert!(
        sent["tools"][0]["description"]
            .as_str()
            .unwrap()
            .contains("Alpha group")
    );
    assert!(sent.get("parallel_tool_calls").is_none());
    let projected = response.response.output();
    assert_eq!(projected[1]["namespace"], "alpha");
    assert_eq!(projected[1]["name"], "lookup");
    assert_eq!(projected[3]["namespace"], "beta");
    let encoded = projected[0]["encrypted_content"].as_str().unwrap();
    let h: Value = serde_json::from_str(
        encoded
            .strip_prefix("caidex.qwen.native-history.v2:")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(h["response"], original);
    assert_eq!(h["request"], sent);
    assert_eq!(h["tool_mapping"]["tools"], source["tools"]);
    assert!(!encoded.contains(KEY));
    let mut next = source.clone();
    next["input"].as_array_mut().unwrap().extend(
        serde_json::from_str::<Vec<Value>>(&serde_json::to_string(projected).unwrap()).unwrap(),
    );
    next["input"].as_array_mut().unwrap().extend([
        json!({"type":"function_call_output","call_id":"call_beta","output":[{"type":"input_text","text":"Beta"}]}),
        json!({"type":"function_call_output","call_id":"call_alpha","output":"Alpha"}),
        json!({"role":"user","content":"Continue"})]);
    let next_source = next.clone();
    let second_response = provider
        .create_response(request(next), RequestContext::default())
        .await
        .unwrap();
    let next = fixture.captured().await.body.unwrap();
    let input = next["input"].as_array().unwrap();
    assert_eq!(input[2], original["output"][0]);
    assert_eq!(input[3], original["output"][1]);
    assert_eq!(
        input[4],
        json!({"type":"function_call_output","call_id":"call_alpha","output":"Alpha"})
    );
    assert_eq!(input[5], original["output"][3]);
    assert_eq!(
        input[6],
        json!({"type":"function_call_output","call_id":"call_beta","output":"Beta"})
    );
    assert_eq!(input[7], original["output"][2]);
    assert_eq!(input[8]["content"], "Continue");
    let third = tools_next(&next_source, second_response.response.output());
    provider
        .create_response(request(third), RequestContext::default())
        .await
        .unwrap();
    let third = fixture.captured().await.body.unwrap();
    let mut expected = next["input"].as_array().unwrap().clone();
    expected.extend(native()["output"].as_array().unwrap().clone());
    expected.push(json!({"role":"user","content":"Continue"}));
    assert_eq!(third["input"], json!(expected));
}

fn tool_events(response: &Value) -> Vec<Value> {
    let mut events = summary_events(response);
    let terminal = events.pop().unwrap();
    for (index, item) in response["output"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(_, i)| i["type"] == "function_call")
    {
        let mut added = item.clone();
        added["arguments"] = "".into();
        added["status"] = "in_progress".into();
        events.push(json!({"type":"response.output_item.added","output_index":index,"item":added}));
        events.push(json!({"type":"response.function_call_arguments.delta","output_index":index,"item_id":item["id"],"delta":item["arguments"]}));
        events.push(json!({"type":"response.function_call_arguments.done","output_index":index,"item_id":item["id"],"arguments":item["arguments"]}));
        events.push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
    }
    events.push(terminal);
    for (index, event) in events.iter_mut().enumerate() {
        event["sequence_number"] = index.into();
    }
    events
}
fn tools_capsule(output: &[Value]) -> Value {
    serde_json::from_str(
        output[0]["encrypted_content"]
            .as_str()
            .unwrap()
            .strip_prefix("caidex.qwen.native-history.v2:")
            .unwrap(),
    )
    .unwrap()
}
fn tools_next(source: &Value, output: &[Value]) -> Value {
    let mut next = source.clone();
    next["input"]
        .as_array_mut()
        .unwrap()
        .extend(output.iter().cloned());
    for item in output.iter().filter(|i| i["type"] == "function_call") {
        next["input"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"function_call_output","call_id":item["call_id"],"output":"Done"}));
    }
    next["input"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role":"user","content":"Continue"}));
    next
}
#[tokio::test]
async fn native_tool_stream_withholds_calls_until_terminal_and_retains_all_raw_chunks() {
    let original = tool_native();
    let chunks = tool_events(&original);
    let mut fixture = Fixture::start(vec![
        Reply::stream(events_body(&chunks)),
        Reply::json(native()),
    ])
    .await;
    let (broker, _) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_native_tools();
    let source = tool_source(true);
    let mut response = provider
        .stream_response(request(source.clone()), RequestContext::default())
        .await
        .unwrap();
    let mut parser = caidex_model_core::ResponsesStream::new(limits().frame_bytes).unwrap();
    let mut output = None;
    let mut carrier = false;
    let mut calls = 0;
    while let Some(event) = response.events.next().await {
        if let ProviderStreamEvent::Model(event) = event.unwrap() {
            let wire = event.response.wire();
            assert!(
                !event
                    .response
                    .kind()
                    .starts_with("response.function_call_arguments.")
            );
            if wire["item"]["encrypted_content"].is_string() {
                carrier = true;
            }
            if wire["item"]["type"] == "function_call" {
                assert!(carrier);
                calls += 1;
                assert_eq!(wire["item"]["name"], "lookup");
            }
            parser.push(format!("data: {wire}\n\n").as_bytes()).unwrap();
            if event.response.kind() == "response.completed" {
                output = Some(wire["response"]["output"].as_array().unwrap().clone());
            }
        }
    }
    assert_eq!(calls, 4);
    assert_eq!(
        parser.finish().unwrap(),
        caidex_model_core::StreamState::Completed
    );
    let sent = fixture.captured().await.body.unwrap();
    let output = output.unwrap();
    let h = tools_capsule(&output);
    assert_eq!(h["chunks"], json!(chunks));
    assert_eq!(h["response"], original);
    assert_eq!(h["request"], sent);
    let mut next = tools_next(&source, &output);
    next["stream"] = false.into();
    provider
        .create_response(request(next), RequestContext::default())
        .await
        .unwrap();
    let sent = fixture.captured().await.body.unwrap();
    assert_eq!(sent["input"][3], original["output"][1]);
    assert_eq!(sent["input"][4]["call_id"], "call_alpha");
    assert_eq!(sent["input"][5], original["output"][3]);
    assert_eq!(sent["input"][6]["call_id"], "call_beta");
}
#[tokio::test]
async fn named_and_allowed_tool_choices_compile_native_subset_and_enforce_selection() {
    for choice in [
        json!({"type":"function","namespace":"alpha","name":"lookup"}),
        json!({"type":"allowed_tools","mode":"required","tools":[{"type":"function","namespace":"alpha","name":"lookup"}]}),
        json!({"type":"allowed_tools","mode":"auto","tools":[{"type":"function","namespace":"alpha","name":"lookup"}]}),
    ] {
        let mut first = tool_native();
        first["output"].as_array_mut().unwrap().pop();
        let mut fixture = Fixture::start(vec![Reply::json(first)]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        let provider = fixture
            .provider(
                broker,
                vec![metadata("fixture", "native-fixture")],
                limits(),
            )
            .with_native_tools();
        let mut source = tool_source(false);
        source["tool_choice"] = choice;
        provider
            .create_response(request(source.clone()), RequestContext::default())
            .await
            .unwrap();
        let sent = fixture.captured().await.body.unwrap();
        assert_eq!(sent["tools"].as_array().unwrap().len(), 1);
        assert_eq!(sent["tools"][0]["name"], "caidex_ns_0");
        assert_eq!(sent["tool_choice"]["type"], "allowed_tools");
        assert_eq!(
            sent["tool_choice"]["tools"],
            json!([{"type":"function","name":"caidex_ns_0"}])
        );
    }
    for choice in [json!("required"), json!("auto"), Value::Null, json!("none")] {
        let mut fixture = Fixture::start(vec![Reply::json(if choice == "none" {
            native()
        } else {
            let mut r = tool_native();
            r["output"].as_array_mut().unwrap().pop();
            r
        })])
        .await;
        let (broker, _) = fixture_broker(Some(KEY));
        let provider = fixture
            .provider(
                broker,
                vec![metadata("fixture", "native-fixture")],
                limits(),
            )
            .with_native_tools();
        let mut source = tool_source(false);
        source["tools"].as_array_mut().unwrap().pop();
        source["tool_choice"] = choice.clone();
        provider
            .create_response(request(source), RequestContext::default())
            .await
            .unwrap();
        let sent = fixture.captured().await.body.unwrap();
        assert_eq!(sent["tool_choice"], choice);
    }
}
#[tokio::test]
async fn bad_tool_declarations_policies_and_results_refuse_before_key_or_post() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_native_tools();
    for case in 0..21 {
        let mut wire = tool_source(false);
        match case {
            0=>wire["tool_choice"]="required".into(),
            1=>wire["parallel_tool_calls"]=false.into(),
            2=>wire["tools"][0]["tools"][0]["strict"]=true.into(),
            3=>wire["tools"][0]["tools"][0]["defer_loading"]=true.into(),
            4=>wire["tools"][0]["tools"][0]["type"]="custom".into(),
            5=>wire["tools"][0]["tools"][0]["name"]="bad.name".into(),
            6=>wire["tools"][0]["tools"][0]["parameters"]=json!([]),
            7=>wire["tools"][0]["tools"][0]["description"]=false.into(),
            8=>wire["tools"][0]["tools"].as_array_mut().unwrap().clear(),
            9=>wire["tools"][0]["description"]=false.into(),
            10=>{wire["tools"][1]=wire["tools"][0].clone();},
            11=>wire["tool_choice"]=json!({"type":"function","namespace":"absent","name":"lookup"}),
            12=>wire["tool_choice"]=json!({"type":"allowed_tools","mode":"required","tools":[{"type":"function","namespace":"alpha","name":"lookup"},{"type":"function","namespace":"beta","name":"lookup"}]}),
            13=>wire["tool_choice"]=json!({"type":"allowed_tools","mode":"auto","tools":[{"type":"function","namespace":"alpha","name":"lookup"},{"type":"function","namespace":"alpha","name":"lookup"}]}),
            14=>wire["input"].as_array_mut().unwrap().push(json!({"type":"function_call_output","call_id":"orphan","output":"x"})),
            15=>wire["input"].as_array_mut().unwrap().push(json!({"type":"function_call","namespace":"alpha","name":"lookup","call_id":"call","arguments":"{}"})),
            16=>wire["input"].as_array_mut().unwrap().extend([json!({"type":"function_call","namespace":"alpha","name":"lookup","call_id":"call","arguments":"[]"}),json!({"type":"function_call_output","call_id":"call","output":"x"})]),
            17=>wire["input"].as_array_mut().unwrap().extend([json!({"type":"function_call","namespace":"alpha","name":"lookup","call_id":"call","arguments":"{}"}),json!({"type":"function_call_output","call_id":"call","output":[{"type":"input_image","image_url":"remote"}]})]),
            18=>wire["input"].as_array_mut().unwrap().extend([json!({"type":"function_call","namespace":"alpha","name":"lookup","call_id":"call","arguments":"{}"}),json!({"role":"user","content":"Before result"}),json!({"type":"function_call_output","call_id":"call","output":"x"})]),
            19=>wire["tools"].as_array_mut().unwrap().push(json!({"type":"mcp","server_url":"remote"})),
            20=>wire["tools"].as_array_mut().unwrap().push(json!({"type":"function","name":"caidex_ns_0"})),
            _=>unreachable!()
        }
        assert!(
            provider
                .create_response(request(wire), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status
                < 500,
            "case {case}"
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn native_tool_payload_choice_and_terminal_states_are_checked_before_delivery() {
    for case in 0..13 {
        let mut value = tool_native();
        let mut source = tool_source(false);
        match case {
            0 => value["output"][1]["name"] = "undeclared".into(),
            1 => value["output"][1]["arguments"] = "[]".into(),
            2 => value["output"][1]["arguments"] = "{broken".into(),
            3 => value["output"][3]["call_id"] = "call_alpha".into(),
            4 => value["output"][1]["status"] = "incomplete".into(),
            5 => value["status"] = "incomplete".into(),
            6 => value["status"] = "failed".into(),
            7 => value["output"][1]["namespace"] = "alpha".into(),
            8 => source["tool_choice"] = "none".into(),
            9 => {
                source["tool_choice"] =
                    json!({"type":"function","namespace":"alpha","name":"lookup"})
            }
            10 => {
                source["tool_choice"] =
                    json!({"type":"function","namespace":"alpha","name":"lookup"});
                value = native();
            }
            11 => value["output"][3]["id"] = "fc_alpha".into(),
            12 => value["output"][1]["type"] = "custom_tool_call".into(),
            _ => unreachable!(),
        }
        for streaming in [false, true] {
            let fixture = Fixture::start(vec![if streaming {
                Reply::stream(events_body(&tool_events(&value)))
            } else {
                Reply::json(value.clone())
            }])
            .await;
            let (broker, _) = fixture_broker(Some(KEY));
            let provider = fixture
                .provider(
                    broker,
                    vec![metadata("fixture", "native-fixture")],
                    limits(),
                )
                .with_native_tools();
            source["stream"] = streaming.into();
            if !streaming {
                assert!(
                    provider
                        .create_response(request(source.clone()), RequestContext::default())
                        .await
                        .err()
                        .unwrap()
                        .http_status
                        >= 500,
                    "case {case}"
                );
            } else {
                let mut response = provider
                    .stream_response(request(source.clone()), RequestContext::default())
                    .await
                    .unwrap();
                let mut failed = false;
                while let Some(event) = response.events.next().await {
                    match event {
                        Err(_) => {
                            failed = true;
                            break;
                        }
                        Ok(ProviderStreamEvent::Model(e)) => {
                            assert_ne!(e.response.kind(), "response.completed", "case {case}");
                            assert_ne!(
                                e.response.wire()["item"]["type"],
                                "function_call",
                                "case {case}"
                            );
                        }
                        _ => (),
                    }
                }
                assert!(failed, "case {case}");
            }
        }
    }
}
#[tokio::test]
async fn malformed_tool_argument_streams_never_deliver_calls_and_release_socket_slot() {
    for case in 0..13 {
        let mut events = tool_events(&tool_native());
        let index = events
            .iter()
            .position(|e| e["type"] == "response.function_call_arguments.delta")
            .unwrap();
        match case {
            0 => events[index]["delta"] = "{}".into(),
            1 => events[index]["item_id"] = "wrong".into(),
            2 => events[index]["output_index"] = 90.into(),
            3 => events[index + 1]["arguments"] = "{}".into(),
            4 => events[index + 2]["item"]["arguments"] = "{}".into(),
            5 => events[index - 1]["item"]["name"] = "wrong".into(),
            6 => events[index - 1]["item"]["call_id"] = "wrong".into(),
            7 => events[index]["content_index"] = 0.into(),
            8 => {
                events.insert(index + 2, events[index].clone());
            }
            9 => {
                events.insert(index + 2, events[index + 1].clone());
            }
            10 => {
                events.insert(index + 3, events[index].clone());
            }
            11 => {
                events.pop();
            }
            12 => events[index]["type"] = "response.function_call_arguments.future".into(),
            _ => unreachable!(),
        }
        for (i, e) in events.iter_mut().enumerate() {
            e["sequence_number"] = i.into();
        }
        let mut bad = Reply::stream(events_body(&events));
        bad.stall = 2;
        let mut fixture = Fixture::start(vec![bad, Reply::json(native())]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        let provider = fixture
            .provider(
                broker,
                vec![metadata("fixture", "native-fixture")],
                limits(),
            )
            .with_native_tools();
        let mut response = provider
            .stream_response(request(tool_source(true)), RequestContext::default())
            .await
            .unwrap();
        let mut failed = false;
        while let Some(event) = response.events.next().await {
            match event {
                Err(_) => {
                    failed = true;
                    break;
                }
                Ok(ProviderStreamEvent::Model(e)) => {
                    assert_ne!(e.response.kind(), "response.completed", "case {case}");
                    assert_ne!(
                        e.response.wire()["item"]["type"],
                        "function_call",
                        "case {case}"
                    );
                    assert!(
                        e.response.wire()["item"]["encrypted_content"].is_null(),
                        "case {case}"
                    );
                }
                _ => (),
            }
        }
        assert!(failed, "case {case}");
        assert!(response.events.next().await.is_none());
        fixture.captured().await;
        fixture.disconnected().await;
        provider
            .create_response(request(tool_source(false)), RequestContext::default())
            .await
            .unwrap();
        fixture.captured().await;
    }
}

#[tokio::test]
async fn tool_history_binds_policy_namespace_and_full_serialized_group_before_credentials() {
    let mut fixture = Fixture::start(vec![Reply::json(tool_native()), Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_native_tools();
    let source = tool_source(false);
    let response = provider
        .create_response(request(source.clone()), RequestContext::default())
        .await
        .unwrap();
    fixture.captured().await;
    let good = tools_next(&source, response.response.output());
    for case in 0..14 {
        let mut wire = good.clone();
        match case {
            0 => wire["tools"][0]["description"] = "Changed".into(),
            1 => wire["tools"][0]["tools"][0]["parameters"]["required"] = json!([]),
            2 => wire["tool_choice"] = "none".into(),
            3 => wire["parallel_tool_calls"] = Value::Null,
            4 => wire["input"][3]["namespace"] = "beta".into(),
            5 => wire["input"][3]["arguments"] = "{}".into(),
            6 => {
                wire["input"].as_array_mut().unwrap().remove(4);
            }
            7 => {
                wire["input"].as_array_mut().unwrap().swap(3, 5);
            }
            8 => wire["input"][2]["summary"][0]["text"] = "Changed".into(),
            9 => {
                let h = tools_capsule(response.response.output());
                wire["input"][2]["encrypted_content"] =
                    format!("caidex.qwen.native-history.v1:{h}").into();
            }
            10 => {
                let mut h = tools_capsule(response.response.output());
                h["tool_mapping"]["tools"][0]["description"] = "Changed".into();
                wire["input"][2]["encrypted_content"] =
                    format!("caidex.qwen.native-history.v2:{h}").into();
            }
            11 => {
                let mut h = tools_capsule(response.response.output());
                h["request"]["tools"][0]["parameters"] = json!({});
                wire["input"][2]["encrypted_content"] =
                    format!("caidex.qwen.native-history.v2:{h}").into();
            }
            12 => wire["input"].as_array_mut().unwrap().push(
                json!({"type":"function_call_output","call_id":"call_alpha","output":"Duplicate"}),
            ),
            13 => {
                let mut h = tools_capsule(response.response.output());
                h["scope"]["credential"]["profile"] = "other".into();
                wire["input"][2]["encrypted_content"] =
                    format!("caidex.qwen.native-history.v2:{h}").into();
            }
            _ => unreachable!(),
        }
        assert!(
            provider
                .create_response(request(wire), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status
                < 500,
            "case {case}"
        );
    }
    let legacy = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_native_history();
    let mut no_tools = good;
    for key in ["tools", "tool_choice", "parallel_tool_calls"] {
        no_tools.as_object_mut().unwrap().remove(key);
    }
    assert!(
        legacy
            .create_response(request(no_tools), RequestContext::default())
            .await
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn caller_supplied_function_history_is_paired_and_invalid_results_are_rejected() {
    let mut fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_native_tools();
    let mut source = tool_source(false);
    source["input"].as_array_mut().unwrap().extend([
        json!({"type":"function_call","namespace":"alpha","name":"lookup","call_id":"direct","arguments":"{\"q\":\"x\"}"}),
        json!({"type":"function_call_output","call_id":"direct","output":[{"type":"input_text","text":"First"},{"type":"output_text","text":"Second"}]}),
        json!({"role":"user","content":"Continue"})]);
    provider
        .create_response(request(source.clone()), RequestContext::default())
        .await
        .unwrap();
    let sent = fixture.captured().await.body.unwrap();
    assert_eq!(sent["input"][2]["name"], "caidex_ns_0");
    assert!(sent["input"][2].get("namespace").is_none());
    assert_eq!(sent["input"][3]["output"], "First\nSecond");
    for case in 0..4 {
        let mut wire = source.clone();
        match case {
            0 => wire["input"][3]["status"] = "in_progress".into(),
            1 => wire["input"][3]["call_id"] = "orphan".into(),
            2 => {
                let extra = [wire["input"][2].clone(), wire["input"][3].clone()];
                wire["input"].as_array_mut().unwrap().extend(extra);
            }
            3 => wire["input"][2]["name"] = "undeclared".into(),
            _ => unreachable!(),
        }
        assert!(
            provider
                .create_response(request(wire), RequestContext::default())
                .await
                .is_err()
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn partial_tool_stream_cancel_drop_and_deadline_never_publish_calls_or_carrier() {
    let events = tool_events(&tool_native());
    let pos = events
        .iter()
        .position(|e| e["type"] == "response.function_call_arguments.delta")
        .unwrap();
    let mut reply = Reply::stream(events_body(&events[..=pos]));
    reply.stall = 2;
    let mut fixture = Fixture::start(vec![reply]).await;
    let (broker, _) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_native_tools();
    for action in ["cancel", "drop", "deadline"] {
        let cancellation = CancellationToken::new();
        let context = RequestContext {
            cancellation: cancellation.clone(),
            deadline: (action == "deadline")
                .then(|| std::time::Instant::now() + Duration::from_millis(300)),
            headers: ContextHeaders::default(),
        };
        let mut response = provider
            .stream_response(request(tool_source(true)), context)
            .await
            .unwrap();
        // The earlier withheld reasoning/message events also emit heartbeats.
        // Wait until delayed message parts flush, then consume the tool delta.
        let mut message_flushed = false;
        loop {
            let e = response.events.next().await.unwrap().unwrap();
            match e {
                ProviderStreamEvent::Heartbeat if message_flushed => break,
                ProviderStreamEvent::Heartbeat => (),
                ProviderStreamEvent::Model(e) => {
                    assert_ne!(e.response.wire()["item"]["type"], "function_call");
                    assert!(e.response.wire()["item"]["encrypted_content"].is_null());
                    message_flushed |= e.response.kind() == "response.content_part.done";
                }
            }
        }
        fixture.captured().await;
        if action == "drop" {
            drop(response);
        } else {
            if action == "cancel" {
                cancellation.cancel();
            } else {
                tokio::time::sleep(Duration::from_millis(310)).await;
            }
            let error = response.events.next().await.unwrap().err().unwrap();
            assert_eq!(
                error.code,
                if action == "cancel" {
                    "provider_cancelled"
                } else {
                    "provider_timeout"
                }
            );
            assert!(response.events.next().await.is_none());
        }
        fixture.disconnected().await;
    }
}

#[tokio::test]
async fn tool_opt_in_keeps_capability_budgets_default_guards_and_real_error_semantics() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let mut m = metadata("fixture", "native-fixture");
    m.capabilities.native_tools = CapabilitySupport::Unsupported;
    let p = fixture
        .provider(broker.clone(), vec![m], limits())
        .with_native_tools();
    assert_eq!(
        p.create_response(request(tool_source(false)), RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "unsupported_tools"
    );
    for history in [false, true] {
        let p = fixture.provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        );
        let p = if history { p.with_native_history() } else { p };
        assert!(
            p.create_response(request(tool_source(false)), RequestContext::default())
                .await
                .is_err()
        );
    }
    let mut expanded = tool_source(false);
    expanded["tools"] = json!([{"type":"namespace","name":"group","description":"g".repeat(1024),"tools":(0..6).map(|i|json!({"type":"function","name":format!("member_{i}"),"description":"d","parameters":{"type":"object"}})).collect::<Vec<_>>()}]);
    let mut expanded_limits = limits();
    expanded_limits.request_bytes = expanded.to_string().len() + 100;
    let p = fixture
        .provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            expanded_limits,
        )
        .with_native_tools();
    assert_eq!(
        p.create_response(request(expanded), RequestContext::default())
            .await
            .err()
            .unwrap()
            .http_status,
        413
    );
    let source = tool_source(false);
    let mut small = limits();
    small.request_bytes = source.to_string().len() - 1;
    let p = fixture
        .provider(broker, vec![metadata("fixture", "native-fixture")], small)
        .with_native_tools();
    assert_eq!(
        p.create_response(request(source), RequestContext::default())
            .await
            .err()
            .unwrap()
            .http_status,
        413
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
    let mut fixture = Fixture::start(vec![Reply::stream(format!(
        "{}{}",
        created(),
        event(json!({"type":"error","code":"native_failure","message":"Synthetic failure"}))
    ))])
    .await;
    let (broker, _) = fixture_broker(Some(KEY));
    let p = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_native_tools();
    let mut r = p
        .stream_response(request(tool_source(true)), RequestContext::default())
        .await
        .unwrap();
    let mut error = false;
    while let Some(e) = r.events.next().await {
        if let ProviderStreamEvent::Model(e) = e.unwrap() {
            if e.response.kind() == "error" {
                assert_eq!(e.response.wire()["code"], "native_failure");
                error = true;
            }
            assert!(e.response.wire()["item"]["encrypted_content"].is_null());
        }
    }
    assert!(error);
    fixture.captured().await;
}

#[tokio::test]
async fn gateway_bound_tool_history_keeps_authentication_and_runtime_attribution_separate() {
    let mut fixture = Fixture::start(vec![
        Reply::stream(events_body(&tool_events(&tool_native()))),
        Reply::json(native()),
    ])
    .await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_runtime_context()
        .with_native_tools();
    let gateway =
        caidex_model_gateway::start_with_provider(Arc::new(provider), broker.redactor(), limits())
            .await
            .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let source = tool_source(true);
    let response = client
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .header("session_id", "LOCAL_TOOL_SESSION")
        .body(source.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let raw = response.text().await.unwrap();
    assert!(!raw.contains(KEY));
    assert!(!raw.contains(gateway.token().expose()));
    let mut parser = caidex_model_core::ResponsesStream::new(limits().frame_bytes).unwrap();
    let parsed = parser.push(raw.as_bytes()).unwrap();
    assert_eq!(
        parser.finish().unwrap(),
        caidex_model_core::StreamState::Completed
    );
    let end = parsed.last().unwrap().response.wire()["response"].clone();
    let first = fixture.captured().await;
    assert!(!first.headers.contains(gateway.token().expose()));
    assert!(!first.headers.contains("LOCAL_TOOL_SESSION"));
    let mut next = tools_next(&source, end["output"].as_array().unwrap());
    next["stream"] = false.into();
    let response = client
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .body(next.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
    assert_eq!(
        tools_capsule(body["output"].as_array().unwrap())["response"],
        native()
    );
    let second = fixture.captured().await;
    assert_eq!(
        second.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert!(
        !second
            .body
            .unwrap()
            .to_string()
            .contains("caidex.qwen.native-history")
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    gateway.shutdown().await.unwrap();
}

fn custom_source(streaming: bool) -> Value {
    let mut source = tool_source(streaming);
    source["tools"][0]["tools"][0] = json!({"type":"custom","name":"lookup","description":"Freeform patch","format":{"type":"grammar","syntax":"lark","definition":"start: /.+/"},"defer_loading":false});
    source
}
fn custom_native() -> Value {
    let mut value = tool_native();
    value["output"][1]["arguments"] = json!({"input":"*** Begin Patch\n中文🙂\n*** End Patch"})
        .to_string()
        .into();
    value
}
fn custom_next(source: &Value, output: &[Value]) -> Value {
    let mut next = source.clone();
    next["input"]
        .as_array_mut()
        .unwrap()
        .extend(output.iter().cloned());
    for item in output.iter().filter(|i| {
        matches!(
            i["type"].as_str(),
            Some("function_call" | "custom_tool_call")
        )
    }) {
        next["input"].as_array_mut().unwrap().push(json!({"type":if item["type"] == "custom_tool_call" {"custom_tool_call_output"} else {"function_call_output"},"call_id":item["call_id"],"output":"Done"}));
    }
    next["input"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role":"user","content":"Continue"}));
    next
}
fn custom_capsule(output: &[Value]) -> Value {
    serde_json::from_str(
        output[0]["encrypted_content"]
            .as_str()
            .unwrap()
            .strip_prefix("caidex.qwen.native-history.v3:")
            .unwrap(),
    )
    .unwrap()
}
#[tokio::test]
async fn custom_mapping_preserves_freeform_identity_native_payload_and_three_round_replay() {
    let original = custom_native();
    let mut fixture =
        Fixture::start(vec![Reply::json(original.clone()), Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_custom_tool_mapping();
    let source = custom_source(false);
    let response = provider
        .create_response(request(source.clone()), RequestContext::default())
        .await
        .unwrap();
    let sent = fixture.captured().await.body.unwrap();
    assert_eq!(sent["tools"][0]["type"], "function");
    assert_eq!(
        sent["tools"][0]["parameters"],
        json!({"type":"object","properties":{"input":{"type":"string"}},"required":["input"],"additionalProperties":false})
    );
    assert!(
        sent["tools"][0]["description"]
            .as_str()
            .unwrap()
            .contains("guidance only")
    );
    assert!(sent["tools"][0].get("format").is_none());
    let output = response.response.output();
    assert_eq!(output[1]["type"], "custom_tool_call");
    assert_eq!(output[1]["input"], "*** Begin Patch\n中文🙂\n*** End Patch");
    assert!(output[1].get("arguments").is_none());
    assert_eq!(output[1]["namespace"], "alpha");
    assert_eq!(output[1]["name"], "lookup");
    assert_eq!(output[1]["future"], original["output"][1]["future"]);
    assert_eq!(output[3]["type"], "function_call");
    let h = custom_capsule(output);
    assert_eq!(h["version"], 3);
    assert_eq!(h["tool_mapping"]["custom_as_function"], true);
    assert_eq!(h["tool_mapping"]["tools"], source["tools"]);
    assert_eq!(h["response"], original);
    assert_eq!(h["request"], sent);
    assert!(!h.to_string().contains(KEY));
    let encoded: Vec<Value> =
        serde_json::from_str(&serde_json::to_string(output).unwrap()).unwrap();
    let next = custom_next(&source, &encoded);
    let response = provider
        .create_response(request(next.clone()), RequestContext::default())
        .await
        .unwrap();
    let sent = fixture.captured().await.body.unwrap();
    assert_eq!(sent["input"][3], original["output"][1]);
    assert_eq!(
        sent["input"][4],
        json!({"type":"function_call_output","call_id":"call_alpha","output":"Done"})
    );
    provider
        .create_response(
            request(custom_next(&next, response.response.output())),
            RequestContext::default(),
        )
        .await
        .unwrap();
    let third = fixture.captured().await.body.unwrap();
    let mut expected = sent["input"].as_array().unwrap().clone();
    expected.extend(native()["output"].as_array().unwrap().clone());
    expected.push(json!({"role":"user","content":"Continue"}));
    assert_eq!(third["input"], json!(expected));
    assert_eq!(reads.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn custom_stream_preserves_raw_chunks_and_withholds_freeform_calls_until_terminal() {
    let original = custom_native();
    let chunks = tool_events(&original);
    let mut fixture = Fixture::start(vec![
        Reply::stream(events_body(&chunks)),
        Reply::json(native()),
    ])
    .await;
    let (broker, _) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_custom_tool_mapping();
    let source = custom_source(true);
    let mut response = provider
        .stream_response(request(source.clone()), RequestContext::default())
        .await
        .unwrap();
    let mut parser = caidex_model_core::ResponsesStream::new(limits().frame_bytes).unwrap();
    let mut output = None;
    let mut carrier = false;
    let mut calls = 0;
    while let Some(event) = response.events.next().await {
        if let ProviderStreamEvent::Model(event) = event.unwrap() {
            let wire = event.response.wire();
            assert!(
                !event
                    .response
                    .kind()
                    .starts_with("response.function_call_arguments.")
            );
            if wire["item"]["encrypted_content"].is_string() {
                carrier = true;
            }
            if matches!(
                wire["item"]["type"].as_str(),
                Some("function_call" | "custom_tool_call")
            ) {
                assert!(carrier);
                calls += 1;
                assert_eq!(wire["item"]["name"], "lookup");
            }
            parser.push(format!("data: {wire}\n\n").as_bytes()).unwrap();
            if event.response.kind() == "response.completed" {
                output = Some(wire["response"]["output"].as_array().unwrap().clone());
            }
        }
    }
    assert_eq!(calls, 4);
    assert_eq!(
        parser.finish().unwrap(),
        caidex_model_core::StreamState::Completed
    );
    let sent = fixture.captured().await.body.unwrap();
    let output = output.unwrap();
    let h = custom_capsule(&output);
    assert_eq!(h["chunks"], json!(chunks));
    assert_eq!(output[1]["type"], "custom_tool_call");
    assert_eq!(output[1]["input"], "*** Begin Patch\n中文🙂\n*** End Patch");
    assert_eq!(h["response"], original);
    assert_eq!(h["request"], sent);
    let mut next = custom_next(&source, &output);
    next["stream"] = false.into();
    provider
        .create_response(request(next), RequestContext::default())
        .await
        .unwrap();
    let sent = fixture.captured().await.body.unwrap();
    assert_eq!(sent["input"][3], original["output"][1]);
    assert_eq!(sent["input"][4]["call_id"], "call_alpha");
    assert_eq!(sent["input"][5], original["output"][3]);
    assert_eq!(sent["input"][6]["call_id"], "call_beta");
}

#[tokio::test]
async fn custom_partial_stream_cancel_drop_and_deadline_release_socket_without_delivering_calls() {
    let events = tool_events(&custom_native());
    let pos = events
        .iter()
        .position(|e| e["type"] == "response.function_call_arguments.delta")
        .unwrap();
    let mut reply = Reply::stream(events_body(&events[..=pos]));
    reply.stall = 2;
    let mut fixture = Fixture::start(vec![reply]).await;
    let (broker, _) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_custom_tool_mapping();
    for action in ["cancel", "drop", "deadline"] {
        let cancellation = CancellationToken::new();
        let context = RequestContext {
            cancellation: cancellation.clone(),
            deadline: (action == "deadline")
                .then(|| std::time::Instant::now() + Duration::from_millis(300)),
            headers: ContextHeaders::default(),
        };
        let mut response = provider
            .stream_response(request(custom_source(true)), context)
            .await
            .unwrap();
        // The earlier withheld reasoning/message events also emit heartbeats.
        // Wait until delayed message parts flush, then consume the tool delta.
        let mut message_flushed = false;
        loop {
            let e = response.events.next().await.unwrap().unwrap();
            match e {
                ProviderStreamEvent::Heartbeat if message_flushed => break,
                ProviderStreamEvent::Heartbeat => (),
                ProviderStreamEvent::Model(e) => {
                    assert!(!matches!(
                        e.response.wire()["item"]["type"].as_str(),
                        Some("function_call" | "custom_tool_call")
                    ));
                    assert!(e.response.wire()["item"]["encrypted_content"].is_null());
                    message_flushed |= e.response.kind() == "response.content_part.done";
                }
            }
        }
        fixture.captured().await;
        if action == "drop" {
            drop(response);
        } else {
            if action == "cancel" {
                cancellation.cancel();
            } else {
                tokio::time::sleep(Duration::from_millis(310)).await;
            }
            let error = response.events.next().await.unwrap().err().unwrap();
            assert_eq!(
                error.code,
                if action == "cancel" {
                    "provider_cancelled"
                } else {
                    "provider_timeout"
                }
            );
            assert!(response.events.next().await.is_none());
        }
        fixture.disconnected().await;
    }
}

#[tokio::test]
async fn gateway_custom_history_keeps_listener_token_and_runtime_attribution_executor_local() {
    let mut fixture = Fixture::start(vec![
        Reply::stream(events_body(&tool_events(&custom_native()))),
        Reply::json(native()),
    ])
    .await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_runtime_context()
        .with_custom_tool_mapping();
    let gateway =
        caidex_model_gateway::start_with_provider(Arc::new(provider), broker.redactor(), limits())
            .await
            .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let source = custom_source(true);
    let response = client
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .header("session_id", "LOCAL_TOOL_SESSION")
        .body(source.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let raw = response.text().await.unwrap();
    assert!(!raw.contains(KEY));
    assert!(!raw.contains(gateway.token().expose()));
    let mut parser = caidex_model_core::ResponsesStream::new(limits().frame_bytes).unwrap();
    let parsed = parser.push(raw.as_bytes()).unwrap();
    assert_eq!(
        parser.finish().unwrap(),
        caidex_model_core::StreamState::Completed
    );
    let end = parsed.last().unwrap().response.wire()["response"].clone();
    assert_eq!(end["output"][1]["type"], "custom_tool_call");
    let first = fixture.captured().await;
    assert!(!first.headers.contains(gateway.token().expose()));
    assert!(!first.headers.contains("LOCAL_TOOL_SESSION"));
    let mut next = custom_next(&source, end["output"].as_array().unwrap());
    next["stream"] = false.into();
    let response = client
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .body(next.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
    assert_eq!(
        custom_capsule(body["output"].as_array().unwrap())["response"],
        native()
    );
    let second = fixture.captured().await;
    assert_eq!(
        second.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert!(
        !second
            .body
            .unwrap()
            .to_string()
            .contains("caidex.qwen.native-history")
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn custom_formats_namespaces_and_named_choices_compile_explicit_function_guidance() {
    for (index, format) in [
        None,
        Some(json!({"type":"text"})),
        Some(json!({"type":"grammar","syntax":"lark","definition":"start: /.+/"})),
        Some(json!({"type":"grammar","syntax":"regex","definition":"^chosen$"})),
    ]
    .into_iter()
    .enumerate()
    {
        for namespaced in [false, true] {
            let mut source = custom_source(false);
            let mut custom = source["tools"][0]["tools"][0].clone();
            custom.as_object_mut().unwrap().remove("format");
            if let Some(format) = &format {
                custom["format"] = format.clone();
            }
            source["tools"] = if namespaced {
                json!([{"type":"namespace","name":"alpha","tools":[custom]}])
            } else {
                json!([custom])
            };
            let mut selector = json!({"type":"custom","name":"lookup"});
            if namespaced {
                selector["namespace"] = "alpha".into();
            }
            source["tool_choice"] = match index {
                0 => Value::Null,
                1 => "required".into(),
                2 => selector.clone(),
                _ => json!({"type":"allowed_tools","mode":"required","tools":[selector]}),
            };
            let mut original = custom_native();
            original["output"].as_array_mut().unwrap().pop();
            let name = if namespaced { "caidex_ns_0" } else { "lookup" };
            original["output"][1]["name"] = name.into();
            let mut fixture = Fixture::start(vec![Reply::json(original)]).await;
            let (broker, _) = fixture_broker(Some(KEY));
            let provider = fixture
                .provider(
                    broker,
                    vec![metadata("fixture", "native-fixture")],
                    limits(),
                )
                .with_custom_tool_mapping();
            let response = provider
                .create_response(request(source), RequestContext::default())
                .await
                .unwrap();
            let sent = fixture.captured().await.body.unwrap();
            assert_eq!(sent["tools"][0]["type"], "function");
            assert_eq!(sent["tools"][0]["name"], name);
            assert!(sent["tools"][0].get("format").is_none());
            if index >= 2 {
                assert_eq!(
                    sent["tool_choice"],
                    json!({"type":"allowed_tools","mode":"required","tools":[{"type":"function","name":name}]})
                );
            }
            assert_eq!(response.response.output()[1]["type"], "custom_tool_call");
            assert_eq!(
                response.response.output()[1]["input"],
                "*** Begin Patch\n中文🙂\n*** End Patch"
            );
            assert_eq!(
                response.response.output()[1].get("namespace").cloned(),
                if namespaced {
                    Some(json!("alpha"))
                } else {
                    None
                }
            );
        }
    }
}

#[tokio::test]
async fn custom_invalid_declarations_selectors_and_unenabled_policies_refuse_before_authentication()
{
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_custom_tool_mapping();
    for case in 0..18 {
        let mut wire = custom_source(false);
        match case {
            0 => wire["tools"][0]["tools"][0]["parameters"] = json!({}),
            1 => wire["tools"][0]["tools"][0]["strict"] = false.into(),
            2 => wire["tools"][0]["tools"][0]["defer_loading"] = true.into(),
            3 => wire["tools"][0]["tools"][0]["format"] = Value::Null,
            4 => wire["tools"][0]["tools"][0]["format"] = json!({"type":"text","syntax":"lark"}),
            5 => wire["tools"][0]["tools"][0]["format"]["syntax"] = "unknown".into(),
            6 => wire["tools"][0]["tools"][0]["format"]["definition"] = " \n".into(),
            7 => wire["tools"][0]["tools"][0]["format"]["definition"] = 9.into(),
            8 => wire["tools"][0]["tools"][0]["format"]["future"] = true.into(),
            9 => wire["tools"][0]["tools"][0]["description"] = json!({}),
            10 => wire["tools"][0]["tools"][0]["name"] = "invalid/name".into(),
            11 => wire["tools"][0]["tools"][0]["name"] = "x".repeat(65).into(),
            12 => wire["tools"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"custom","name":"caidex_ns_0"})),
            13 => wire["tools"][1]["name"] = "alpha".into(),
            14 => {
                wire["tool_choice"] = json!({"type":"function","namespace":"alpha","name":"lookup"})
            }
            15 => wire["tool_choice"] = json!({"type":"custom","namespace":"beta","name":"lookup"}),
            16 => {
                wire["tool_choice"] = json!({"type":"allowed_tools","mode":"auto","tools":[{"type":"function","namespace":"alpha","name":"lookup"}]})
            }
            17 => {
                wire["tool_choice"] =
                    json!({"type":"custom","namespace":"alpha","name":"lookup","input":"forged"})
            }
            _ => unreachable!(),
        }
        assert_eq!(
            provider
                .create_response(request(wire), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            400,
            "case {case}"
        );
    }
    for policy in [0, 1, 2] {
        let p = fixture.provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        );
        let p = match policy {
            1 => p.with_native_history(),
            2 => p.with_native_tools(),
            _ => p,
        };
        assert_eq!(
            p.create_response(request(custom_source(false)), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            400
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn custom_native_arguments_identity_and_terminal_failures_never_deliver_executable_calls() {
    for case in 0..15 {
        let mut value = custom_native();
        match case {
            0 => value["output"][1]["arguments"] = "{}".into(),
            1 => value["output"][1]["arguments"] = "{\"input\":null}".into(),
            2 => value["output"][1]["arguments"] = "{\"input\":1}".into(),
            3 => value["output"][1]["arguments"] = "{\"input\":{},\"extra\":true}".into(),
            4 => value["output"][1]["arguments"] = "{\"input\":\"ok\",\"extra\":true}".into(),
            5 => value["output"][1]["arguments"] = "[\"text\"]".into(),
            6 => value["output"][1]["arguments"] = "invalid".into(),
            7 => value["output"][1]["name"] = "lookup".into(),
            8 => value["output"][1]["call_id"] = "call_beta".into(),
            9 => value["output"][1]["id"] = "msg_one".into(),
            10 => value["output"][1]["status"] = "in_progress".into(),
            11 => value["status"] = "failed".into(),
            12 => value["status"] = "incomplete".into(),
            13 => {
                value["output"][1]["type"] = "custom_tool_call".into();
                value["output"][1]["input"] = "native custom".into();
                value["output"][1]
                    .as_object_mut()
                    .unwrap()
                    .remove("arguments");
            }
            14 => value["output"][1]["namespace"] = "alpha".into(),
            _ => unreachable!(),
        }
        for streaming in [false, true] {
            let mut fixture = Fixture::start(vec![if streaming {
                Reply::stream(format!("{}{}", created(), terminal(value.clone())))
            } else {
                Reply::json(value.clone())
            }])
            .await;
            let (broker, _) = fixture_broker(Some(KEY));
            let provider = fixture
                .provider(
                    broker,
                    vec![metadata("fixture", "native-fixture")],
                    limits(),
                )
                .with_custom_tool_mapping();
            if !streaming {
                assert_eq!(
                    provider
                        .create_response(request(custom_source(false)), RequestContext::default())
                        .await
                        .err()
                        .unwrap()
                        .http_status,
                    502,
                    "case {case}"
                );
            } else {
                let mut r = provider
                    .stream_response(request(custom_source(true)), RequestContext::default())
                    .await
                    .unwrap();
                let mut failed = false;
                while let Some(e) = r.events.next().await {
                    match e {
                        Err(_) => {
                            failed = true;
                            break;
                        }
                        Ok(ProviderStreamEvent::Model(e)) => {
                            assert_ne!(e.response.kind(), "response.completed");
                            assert!(!matches!(
                                e.response.wire()["item"]["type"].as_str(),
                                Some("function_call" | "custom_tool_call")
                            ));
                            assert!(e.response.wire()["item"]["encrypted_content"].is_null());
                        }
                        _ => (),
                    }
                }
                assert!(failed, "case {case}");
                assert!(r.events.next().await.is_none());
            }
            fixture.captured().await;
            fixture.disconnected().await;
        }
    }
}

#[tokio::test]
async fn custom_history_version_policy_format_and_serialized_display_tampering_refuse_before_keys()
{
    let mut fixture =
        Fixture::start(vec![Reply::json(custom_native()), Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let p = fixture
        .provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_custom_tool_mapping();
    let source = custom_source(false);
    let r = p
        .create_response(request(source.clone()), RequestContext::default())
        .await
        .unwrap();
    fixture.captured().await;
    let encoded: Vec<Value> =
        serde_json::from_str(&serde_json::to_string(r.response.output()).unwrap()).unwrap();
    let good = custom_next(&source, &encoded);
    for case in 0..18 {
        let mut wire = good.clone();
        let mut h = custom_capsule(&encoded);
        let mut version = 3;
        match case {
            0 => version = 1,
            1 => version = 2,
            2 => h["version"] = 2.into(),
            3 => h["tool_mapping"]["custom_as_function"] = false.into(),
            4 => h["tool_mapping"]["custom_as_function"] = Value::Null,
            5 => {
                h["tool_mapping"]
                    .as_object_mut()
                    .unwrap()
                    .remove("custom_as_function");
            }
            6 => {
                h["tool_mapping"]["tools"][0]["tools"][0]["format"]["definition"] = "changed".into()
            }
            7 => h["request"]["tools"][0]["parameters"]["additionalProperties"] = true.into(),
            8 => h["response"]["output"][1]["arguments"] = "{\"input\":1}".into(),
            9 => h["scope"]["credential"]["profile"] = "other".into(),
            10 => h["request"]["input"][0]["content"] = "changed".into(),
            11 => wire["input"][3]["input"] = "changed".into(),
            12 => wire["input"][3]["type"] = "function_call".into(),
            13 => wire["input"][3]["namespace"] = "beta".into(),
            14 => wire["input"][3]["status"] = "incomplete".into(),
            15 => wire["tools"][0]["tools"][0]["format"]["definition"] = "changed".into(),
            16 => wire["tool_choice"] = "none".into(),
            17 => {
                wire["input"].as_array_mut().unwrap().swap(3, 4);
            }
            _ => unreachable!(),
        }
        wire["input"][2]["encrypted_content"] =
            format!("caidex.qwen.native-history.v{version}:{h}").into();
        assert_eq!(
            p.create_response(request(wire), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            400,
            "case {case}"
        );
    }
    let legacy = fixture
        .provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_native_tools();
    let mut converted = good.clone();
    converted["tools"][0]["tools"][0] = tool_source(false)["tools"][0]["tools"][0].clone();
    assert!(
        legacy
            .create_response(request(converted), RequestContext::default())
            .await
            .is_err()
    );
    let mut no_tools = good;
    for k in ["tools", "tool_choice", "parallel_tool_calls"] {
        no_tools.as_object_mut().unwrap().remove(k);
    }
    let legacy = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_native_history();
    assert!(
        legacy
            .create_response(request(no_tools), RequestContext::default())
            .await
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
    for tools in [false, true] {
        let mut fixture = Fixture::start(vec![Reply::json(native())]).await;
        let (broker, reads) = fixture_broker(Some(KEY));
        let legacy = fixture.provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        );
        let legacy = if tools {
            legacy.with_native_tools()
        } else {
            legacy.with_native_history()
        };
        let source = if tools {
            tool_source(false)
        } else {
            basic(false)
        };
        let r = legacy
            .create_response(request(source.clone()), RequestContext::default())
            .await
            .unwrap();
        fixture.captured().await;
        let mut source = source;
        if source["input"].is_string() {
            source["input"] = json!([{"role":"user","content":source["input"]}]);
        }
        let p = fixture
            .provider(
                broker,
                vec![metadata("fixture", "native-fixture")],
                limits(),
            )
            .with_custom_tool_mapping();
        assert_eq!(
            p.create_response(
                request(custom_next(&source, r.response.output())),
                RequestContext::default()
            )
            .await
            .err()
            .unwrap()
            .code,
            "qwen_invalid_history"
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn custom_direct_history_preserves_freeform_text_and_rejects_mismatched_results() {
    let mut fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let p = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_custom_tool_mapping();
    let mut source = custom_source(false);
    let input = "  中文🙂\n\"quoted\"\\ path  \n";
    source["input"].as_array_mut().unwrap().extend([
        json!({"type":"custom_tool_call","namespace":"alpha","name":"lookup","call_id":"direct","input":input}),
        json!({"type":"custom_tool_call_output","call_id":"direct","output":[{"type":"input_text","text":"First"},{"type":"output_text","text":"Second"}]}),
        json!({"role":"user","content":"Continue"})]);
    p.create_response(request(source.clone()), RequestContext::default())
        .await
        .unwrap();
    let sent = fixture.captured().await.body.unwrap();
    assert_eq!(sent["input"][2]["type"], "function_call");
    assert_eq!(sent["input"][2]["name"], "caidex_ns_0");
    assert_eq!(
        serde_json::from_str::<Value>(sent["input"][2]["arguments"].as_str().unwrap()).unwrap(),
        json!({"input":input})
    );
    assert_eq!(
        sent["input"][3],
        json!({"type":"function_call_output","call_id":"direct","output":"First\nSecond"})
    );
    for case in 0..8 {
        let mut wire = source.clone();
        match case {
            0 => wire["input"][2]["input"] = Value::Null,
            1 => wire["input"][2]["arguments"] = "{}".into(),
            2 => wire["input"][2]["namespace"] = "beta".into(),
            3 => wire["input"][3]["type"] = "function_call_output".into(),
            4 => wire["input"][3]["call_id"] = "orphan".into(),
            5 => wire["input"][3]["output"] = json!([{"type":"input_image","image_url":"x"}]),
            6 => {
                let items = [wire["input"][2].clone(), wire["input"][3].clone()];
                wire["input"].as_array_mut().unwrap().extend(items);
            }
            7 => wire["input"][3]["status"] = "in_progress".into(),
            _ => unreachable!(),
        }
        assert_eq!(
            p.create_response(request(wire), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            400,
            "case {case}"
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn custom_policy_retains_capability_source_compiled_budgets_and_native_errors() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let mut m = metadata("fixture", "native-fixture");
    m.capabilities.native_tools = CapabilitySupport::Unsupported;
    let p = fixture
        .provider(broker.clone(), vec![m], limits())
        .with_custom_tool_mapping();
    assert_eq!(
        p.create_response(request(custom_source(false)), RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "unsupported_tools"
    );
    for compiled in [false, true] {
        let mut source = custom_source(false);
        if compiled {
            source["tools"][0]["description"] = "Group".repeat(256).into();
            source["tools"][0]["tools"][0]["format"]["definition"] = "\\".repeat(2048).into();
        }
        let mut l = limits();
        l.request_bytes = source.to_string().len() + if compiled { 10 } else { 0 };
        if !compiled {
            l.request_bytes -= 1;
        }
        let p = fixture
            .provider(
                broker.clone(),
                vec![metadata("fixture", "native-fixture")],
                l,
            )
            .with_custom_tool_mapping();
        assert_eq!(
            p.create_response(request(source), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            413
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
    let mut fixture = Fixture::start(vec![Reply::stream(format!(
        "{}{}",
        created(),
        event(json!({"type":"error","code":"native_failure","message":"Synthetic failure"}))
    ))])
    .await;
    let (broker, _) = fixture_broker(Some(KEY));
    let p = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_custom_tool_mapping();
    let mut r = p
        .stream_response(request(custom_source(true)), RequestContext::default())
        .await
        .unwrap();
    let mut failed = false;
    while let Some(e) = r.events.next().await {
        if let ProviderStreamEvent::Model(e) = e.unwrap() {
            assert!(e.response.wire()["item"]["encrypted_content"].is_null());
            if e.response.kind() == "error" {
                assert_eq!(e.response.wire()["code"], "native_failure");
                failed = true;
            }
        }
    }
    assert!(failed);
    fixture.captured().await;
}

#[tokio::test]
async fn custom_argument_stream_corruption_and_raw_custom_events_release_native_slots() {
    for case in 0..15 {
        let mut events = tool_events(&custom_native());
        let index = events
            .iter()
            .position(|e| e["type"] == "response.function_call_arguments.delta")
            .unwrap();
        match case {
            0 => events[index]["delta"] = "{}".into(),
            1 => events[index]["item_id"] = "wrong".into(),
            2 => events[index]["output_index"] = 90.into(),
            3 => events[index + 1]["arguments"] = "{}".into(),
            4 => events[index + 2]["item"]["arguments"] = "{}".into(),
            5 => events[index - 1]["item"]["name"] = "wrong".into(),
            6 => events[index - 1]["item"]["call_id"] = "wrong".into(),
            7 => events[index]["content_index"] = 0.into(),
            8 => {
                events.insert(index + 2, events[index].clone());
            }
            9 => {
                events.insert(index + 2, events[index + 1].clone());
            }
            10 => {
                events.insert(index + 3, events[index].clone());
            }
            11 => {
                events.pop();
            }
            12 => events[index]["type"] = "response.function_call_arguments.future".into(),
            13 => events[index]["type"] = "response.custom_tool_call_input.delta".into(),
            14 => {
                events[index + 1]["type"] = "response.custom_tool_call_input.done".into();
                events[index + 1]["input"] = "raw custom".into();
            }
            _ => unreachable!(),
        }
        for (i, e) in events.iter_mut().enumerate() {
            e["sequence_number"] = i.into();
        }
        let mut bad = Reply::stream(events_body(&events));
        bad.stall = 2;
        let mut fixture = Fixture::start(vec![bad, Reply::json(native())]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        let provider = fixture
            .provider(
                broker,
                vec![metadata("fixture", "native-fixture")],
                limits(),
            )
            .with_custom_tool_mapping();
        let mut response = provider
            .stream_response(request(custom_source(true)), RequestContext::default())
            .await
            .unwrap();
        let mut failed = false;
        while let Some(event) = response.events.next().await {
            match event {
                Err(_) => {
                    failed = true;
                    break;
                }
                Ok(ProviderStreamEvent::Model(e)) => {
                    assert_ne!(e.response.kind(), "response.completed", "case {case}");
                    assert!(
                        !matches!(
                            e.response.wire()["item"]["type"].as_str(),
                            Some("function_call" | "custom_tool_call")
                        ),
                        "case {case}"
                    );
                    assert!(
                        e.response.wire()["item"]["encrypted_content"].is_null(),
                        "case {case}"
                    );
                }
                _ => (),
            }
        }
        assert!(failed, "case {case}");
        assert!(response.events.next().await.is_none());
        fixture.captured().await;
        fixture.disconnected().await;
        provider
            .create_response(request(custom_source(false)), RequestContext::default())
            .await
            .unwrap();
        fixture.captured().await;
    }
}

#[tokio::test]
async fn runtime_history_controls_replay_json_sse_all_versions_and_three_rounds() {
    for version in 1..=3 {
        for streaming in [false, true] {
            let mut first_native = match version {
                1 => native(),
                2 => tool_native(),
                _ => custom_native(),
            };
            first_native["output"][0]["summary"] = json!([
                {"type":"summary_text","text":"First ","future":{"n":18446744073709551616_u128}},
                {"type":"summary_text","text":"second"}
            ]);
            let events = if version == 1 {
                summary_events(&first_native)
            } else {
                tool_events(&first_native)
            };
            let reply = if streaming {
                Reply::stream(events_body(&events))
            } else {
                Reply::json(first_native.clone())
            };
            let mut fixture = Fixture::start(vec![reply, Reply::json(native())]).await;
            let (broker, reads) = fixture_broker(Some(KEY));
            let mut provider = fixture
                .provider(
                    broker,
                    vec![metadata("fixture", "native-fixture")],
                    limits(),
                )
                .with_runtime_context()
                .with_native_history()
                .with_verbosity_instruction("low".into(), "Be brief".into())
                .unwrap()
                .with_reasoning_effort_mapping("fixture".into(), "high".into(), "xhigh".into())
                .unwrap()
                .with_reasoning_effort_mapping("fixture".into(), "xhigh".into(), "none".into())
                .unwrap();
            provider = match version {
                1 => provider,
                2 => provider.with_native_tools(),
                _ => provider.with_custom_tool_mapping(),
            };
            let mut source = match version {
                1 => runtime_request(streaming),
                2 => tool_source(streaming),
                _ => custom_source(streaming),
            };
            source["instructions"] = "Original instructions".into();
            source["text"] = json!({"format":{"type":"text"},"verbosity":"low"});
            source["client_metadata"] = json!({"attribution":"LOCAL_CONTROL_BODY"});
            source["prompt_cache_key"] = "LOCAL_CONTROL_CACHE".into();
            source["reasoning"] = json!({"effort":"high","summary":"auto","context":"all_turns"});
            source["include"] = json!(["reasoning.encrypted_content"]);
            let first = if streaming {
                let mut response = provider
                    .stream_response(request(source.clone()), runtime_context())
                    .await
                    .unwrap();
                let mut terminal = None;
                while let Some(event) = response.events.next().await {
                    if let ProviderStreamEvent::Model(event) = event.unwrap()
                        && event.response.kind() == "response.completed"
                    {
                        terminal = Some(event.response.wire()["response"].clone());
                    }
                }
                caidex_model_core::CanonicalResponse::new(terminal.unwrap()).unwrap()
            } else {
                provider
                    .create_response(request(source.clone()), runtime_context())
                    .await
                    .unwrap()
                    .response
            };
            let captured = fixture.captured().await;
            assert!(!captured.headers.contains("LOCAL_"));
            let sent = captured.body.unwrap();
            assert_eq!(sent["reasoning"], json!({"effort":"xhigh"}));
            assert_eq!(sent["instructions"], "Original instructions\nBe brief");
            assert_eq!(sent["input"][0]["role"], "developer");
            assert_eq!(sent["store"], false);
            for key in ["include", "text", "client_metadata", "prompt_cache_key"] {
                assert!(sent.get(key).is_none());
            }
            let recorded = match version {
                1 => capsule(first.output()),
                2 => tools_capsule(first.output()),
                _ => custom_capsule(first.output()),
            };
            assert_eq!(recorded["request"], sent);
            assert_eq!(recorded["response"], first_native);
            assert_eq!(first.output()[0]["summary"][0]["text"], "First second");
            assert!(!first.wire().to_string().contains(KEY));
            let mut next = if version == 1 {
                followup(&source, first.output())
            } else {
                custom_next(&source, first.output())
            };
            // Reparse the actual projected carrier as after a client restart.
            next = serde_json::from_str(&next.to_string()).unwrap();
            next["stream"] = false.into();
            next["reasoning"]["effort"] = "xhigh".into();
            let second = provider
                .create_response(request(next.clone()), runtime_context())
                .await
                .unwrap()
                .response;
            let second_sent = fixture.captured().await.body.unwrap();
            assert_eq!(second_sent["reasoning"], json!({"effort":"none"}));
            assert_eq!(second_sent["instructions"], sent["instructions"]);
            let input = second_sent["input"].as_array().unwrap();
            assert_eq!(
                &input[..sent["input"].as_array().unwrap().len()],
                sent["input"].as_array().unwrap()
            );
            assert_eq!(input.iter().filter(|v| v["type"] == "reasoning").count(), 1);
            for (index, item) in input
                .iter()
                .enumerate()
                .filter(|(_, v)| v["type"] == "function_call")
            {
                assert_eq!(input[index + 1]["type"], "function_call_output");
                assert_eq!(input[index + 1]["call_id"], item["call_id"]);
            }
            let third = if version == 1 {
                followup(&next, second.output())
            } else {
                custom_next(&next, second.output())
            };
            provider
                .create_response(request(third), runtime_context())
                .await
                .unwrap();
            let third_sent = fixture.captured().await.body.unwrap();
            let mut expected = second_sent["input"].as_array().unwrap().clone();
            expected.extend(native()["output"].as_array().unwrap().clone());
            expected.push(
                json!({"role":"user","content":if version==1 {"Follow-up"} else {"Continue"}}),
            );
            assert_eq!(third_sent["input"], json!(expected));
            assert_eq!(third_sent["reasoning"], json!({"effort":"none"}));
            assert!(
                !third_sent
                    .to_string()
                    .contains("caidex.qwen.native-history")
            );
            assert_eq!(reads.load(Ordering::SeqCst), 3);
        }
    }
}

#[tokio::test]
async fn runtime_history_controls_nullable_and_missing_effort_do_not_invent_native_controls() {
    let mut fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_runtime_context()
        .with_native_history();
    let mut count = 0;
    for reasoning in [
        json!({"summary":"auto"}),
        json!({"context":"all_turns"}),
        json!({"summary":null,"context":null}),
    ] {
        for include in [
            None,
            Some(Value::Null),
            Some(json!([])),
            Some(json!(["reasoning.encrypted_content"])),
        ] {
            let mut source = basic(false);
            source["reasoning"] = reasoning.clone();
            if let Some(include) = include {
                source["include"] = include;
            }
            let response = provider
                .create_response(request(source), RequestContext::default())
                .await
                .unwrap()
                .response;
            let sent = fixture.captured().await.body.unwrap();
            assert!(sent.get("reasoning").is_none() && sent.get("include").is_none());
            assert_eq!(sent["store"], false);
            assert_eq!(capsule(response.output())["request"], sent);
            count += 1;
        }
    }
    assert_eq!(reads.load(Ordering::SeqCst), count);
}

#[tokio::test]
async fn runtime_history_controls_invalid_and_partial_policies_refuse_before_key_or_post() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let make = || {
        fixture.provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
    };
    let provider = make()
        .with_runtime_context()
        .with_native_history()
        .with_reasoning_effort_mapping("fixture".into(), "high".into(), "xhigh".into())
        .unwrap();
    let mut cases = Vec::new();
    for reasoning in [
        Value::Null,
        json!({}),
        json!([]),
        json!({"summary":"concise"}),
        json!({"summary":"detailed"}),
        json!({"summary":true}),
        json!({"summary":[]}),
        json!({"context":"last_turn"}),
        json!({"context":true}),
        json!({"context":[]}),
        json!({"summary":"auto","unknown":1}),
        json!({"effort":null,"summary":"auto"}),
        json!({"effort":"low","summary":"auto"}),
        json!({"effort":"high","summary":"auto","context":"current_turn"}),
    ] {
        let mut wire = basic(false);
        wire["reasoning"] = reasoning;
        cases.push(wire);
    }
    for include in [
        json!({}),
        json!("reasoning.encrypted_content"),
        json!([null]),
        json!(["message.output_text.logprobs"]),
        json!(["reasoning.encrypted_content", "reasoning.encrypted_content"]),
        json!(["reasoning.encrypted_content", "future"]),
    ] {
        let mut wire = basic(false);
        wire["include"] = include;
        cases.push(wire);
    }
    for (key, value) in [
        ("previous_response_id", json!("remote")),
        ("conversation", json!("remote")),
        ("context_management", json!([{"type":"compaction"}])),
    ] {
        let mut wire = basic(false);
        wire[key] = value;
        cases.push(wire);
    }
    let mut unbound = basic(false);
    unbound["input"] = json!([{"type":"reasoning","id":"unbound","summary":[]}]);
    cases.push(unbound);
    for mut wire in cases {
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
    for partial in [
        make(),
        make().with_runtime_context(),
        make().with_native_history(),
        make().with_native_tools(),
        make().with_custom_tool_mapping(),
    ] {
        // Each individual field also requires both policies, even null/empty values.
        for (key, value) in [
            ("reasoning", json!({"summary":"auto"})),
            ("reasoning", json!({"context":"all_turns"})),
            ("include", json!(["reasoning.encrypted_content"])),
            ("include", Value::Null),
            ("include", json!([])),
        ] {
            for streaming in [false, true] {
                let mut wire = basic(streaming);
                wire[key] = value.clone();
                let error = if streaming {
                    partial
                        .stream_response(request(wire), RequestContext::default())
                        .await
                        .err()
                        .unwrap()
                } else {
                    partial
                        .create_response(request(wire), RequestContext::default())
                        .await
                        .err()
                        .unwrap()
                };
                assert_eq!(error.http_status, 400);
            }
        }
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn runtime_history_controls_reasoning_capability_and_route_mapping_remain_explicit() {
    let mut response = native();
    response["output"] = json!([response["output"][1]]);
    let mut fixture = Fixture::start(vec![Reply::json(response)]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let mut model = metadata("fixture", "native-fixture");
    model.capabilities.reasoning = CapabilitySupport::Unsupported;
    let provider = fixture
        .provider(
            broker,
            vec![model, metadata("other", "native-other")],
            limits(),
        )
        .with_runtime_context()
        .with_native_history()
        .with_reasoning_effort_mapping("fixture".into(), "high".into(), "xhigh".into())
        .unwrap()
        .with_reasoning_effort_mapping("fixture".into(), "none".into(), "none".into())
        .unwrap();
    for streaming in [false, true] {
        for reasoning in [
            json!({"summary":"auto"}),
            json!({"summary":"auto","effort":"none"}),
            json!({"effort":"high","context":"all_turns"}),
        ] {
            let mut wire = basic(streaming);
            wire["reasoning"] = reasoning;
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
            assert_eq!(error.code, "unsupported_reasoning");
        }
        let mut wire = basic(streaming);
        wire["model"] = "other".into();
        wire["reasoning"] = json!({"effort":"high","summary":"auto","context":"all_turns"});
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
        assert_eq!(error.code, "qwen_unsupported_request");
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
    for reasoning in [
        json!({"summary":null,"context":"all_turns"}),
        json!({"effort":"none","context":"all_turns"}),
    ] {
        let mut wire = basic(false);
        wire["reasoning"] = reasoning.clone();
        wire["include"] = json!(["reasoning.encrypted_content"]);
        provider
            .create_response(request(wire), RequestContext::default())
            .await
            .unwrap();
        let sent = fixture.captured().await.body.unwrap();
        assert_eq!(
            sent.get("reasoning").cloned(),
            reasoning.get("effort").map(|v| json!({"effort":v}))
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn runtime_history_controls_source_compiled_budget_and_pre_cancel_deadline_remain_enforced() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let make = |budget| {
        fixture
            .provider(
                broker.clone(),
                vec![metadata("fixture", "native-fixture")],
                budget,
            )
            .with_runtime_context()
            .with_native_history()
    };
    let mut wire = basic(false);
    wire["reasoning"] = json!({"summary":"auto","context":"all_turns"});
    wire["include"] = json!(["reasoning.encrypted_content"]);
    let mut small = limits();
    small.request_bytes = wire.to_string().len() - 1;
    let error = make(small)
        .create_response(request(wire.clone()), RequestContext::default())
        .await
        .err()
        .unwrap();
    assert_eq!(error.http_status, 413);
    let mut small = limits();
    small.request_bytes = 500;
    let expanded = make(small)
        .with_verbosity_instruction("low".into(), "x".repeat(600))
        .unwrap();
    let mut text = wire.clone();
    text["text"] = json!({"verbosity":"low"});
    assert_eq!(
        expanded
            .create_response(request(text), RequestContext::default())
            .await
            .err()
            .unwrap()
            .http_status,
        413
    );
    let provider = make(limits());
    for streaming in [false, true] {
        wire["stream"] = streaming.into();
        for cancelled in [false, true] {
            let mut context = RequestContext::default();
            if cancelled {
                context.cancellation.cancel();
            } else {
                context.deadline = Some(std::time::Instant::now());
            }
            let error = if streaming {
                provider
                    .stream_response(request(wire.clone()), context)
                    .await
                    .err()
                    .unwrap()
            } else {
                provider
                    .create_response(request(wire.clone()), context)
                    .await
                    .err()
                    .unwrap()
            };
            assert_eq!(
                error.code,
                if cancelled {
                    "provider_cancelled"
                } else {
                    "provider_timeout"
                }
            );
        }
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn gateway_runtime_history_controls_keep_authorization_priority_and_serialized_replay_local()
{
    let mut fixture = Fixture::start(vec![
        Reply::stream(events_body(&summary_events(&native()))),
        Reply::json(native()),
    ])
    .await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let provider = fixture
        .provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_runtime_context()
        .with_native_history()
        .with_verbosity_instruction("low".into(), "Be brief".into())
        .unwrap();
    let gateway =
        caidex_model_gateway::start_with_provider(Arc::new(provider), broker.redactor(), limits())
            .await
            .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut source = runtime_request(true);
    source["reasoning"] = json!({"summary":"auto","context":"all_turns"});
    source["include"] = json!(["reasoning.encrypted_content"]);
    let address = format!("http://{}/v1/responses", gateway.address());
    let mut outgoing = client
        .post(&address)
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .body(source.to_string());
    for (name, value) in runtime_context().headers.iter() {
        outgoing = outgoing.header(name, value);
    }
    let response = outgoing.send().await.unwrap();
    assert_eq!(response.status(), 200);
    let raw = response.text().await.unwrap();
    assert!(
        !raw.contains(KEY) && !raw.contains(gateway.token().expose()) && !raw.contains("LOCAL_")
    );
    let mut parser = caidex_model_core::ResponsesStream::new(limits().frame_bytes).unwrap();
    let parsed = parser.push(raw.as_bytes()).unwrap();
    assert_eq!(
        parser.finish().unwrap(),
        caidex_model_core::StreamState::Completed
    );
    let projected = parsed.last().unwrap().response.wire()["response"].clone();
    let sent = fixture.captured().await;
    assert!(!sent.headers.contains("LOCAL_") && !sent.headers.contains(gateway.token().expose()));
    assert_eq!(
        sent.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    let sent = sent.body.unwrap();
    assert_eq!(sent["input"][0]["role"], "developer");
    assert_eq!(sent["input"][2]["role"], "developer");
    assert_eq!(sent["instructions"], "Original instructions\nBe brief");
    assert!(sent.get("include").is_none() && sent.get("reasoning").is_none());
    assert_eq!(
        capsule(projected["output"].as_array().unwrap())["request"],
        sent
    );
    let mut next = followup(&source, projected["output"].as_array().unwrap());
    next["stream"] = false.into();
    let response = client
        .post(&address)
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .body(next.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
    assert_eq!(
        capsule(body["output"].as_array().unwrap())["response"],
        native()
    );
    let replayed = fixture.captured().await.body.unwrap();
    let mut expected = sent["input"].as_array().unwrap().clone();
    expected.extend(native()["output"].as_array().unwrap().clone());
    expected.push(json!({"role":"user","content":"Follow-up"}));
    assert_eq!(replayed["input"], json!(expected));
    assert!(!replayed.to_string().contains("caidex.qwen.native-history"));
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn runtime_history_controls_stream_cancel_drop_and_deadline_never_publish_tools_or_carriers()
{
    for custom in [false, true] {
        let events = if custom {
            tool_events(&custom_native())
        } else {
            summary_events(&native())
        };
        let end = events
            .iter()
            .position(|e| e["type"] == "response.reasoning_text.delta")
            .unwrap()
            + 1;
        let mut stalled = Reply::stream(events_body(&events[..end]));
        stalled.stall = 2;
        let mut fixture = Fixture::start(
            (0..3)
                .flat_map(|_| [stalled.clone(), Reply::json(native())])
                .collect(),
        )
        .await;
        let (broker, reads) = fixture_broker(Some(KEY));
        let mut provider = fixture
            .provider(
                broker,
                vec![metadata("fixture", "native-fixture")],
                limits(),
            )
            .with_runtime_context()
            .with_native_history();
        if custom {
            provider = provider.with_custom_tool_mapping();
        }
        let mut source = if custom {
            custom_source(true)
        } else {
            basic(true)
        };
        source["reasoning"] = json!({"summary":"auto","context":"all_turns"});
        source["include"] = json!(["reasoning.encrypted_content"]);
        for action in ["cancel", "deadline", "drop"] {
            let cancellation = CancellationToken::new();
            let context = RequestContext {
                cancellation: cancellation.clone(),
                deadline: (action == "deadline")
                    .then(|| std::time::Instant::now() + Duration::from_millis(300)),
                headers: ContextHeaders::default(),
            };
            let mut stream = provider
                .stream_response(request(source.clone()), context)
                .await
                .unwrap();
            loop {
                let event = tokio::time::timeout(WAIT, stream.events.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                if let ProviderStreamEvent::Model(event) = event {
                    assert!(event.response.wire()["item"]["encrypted_content"].is_null());
                    assert!(!matches!(
                        event.response.wire()["item"]["type"].as_str(),
                        Some("custom_tool_call" | "function_call")
                    ));
                    if event.response.kind() == "response.reasoning_summary_text.delta" {
                        break;
                    }
                }
            }
            fixture.captured().await;
            if action == "drop" {
                drop(stream);
            } else {
                if action == "cancel" {
                    cancellation.cancel();
                } else {
                    tokio::time::sleep(Duration::from_millis(310)).await;
                }
                let error = stream.events.next().await.unwrap().err().unwrap();
                assert_eq!(
                    error.code,
                    if action == "cancel" {
                        "provider_cancelled"
                    } else {
                        "provider_timeout"
                    }
                );
                assert!(stream.events.next().await.is_none());
            }
            fixture.disconnected().await;
            let mut next = source.clone();
            next["stream"] = false.into();
            let response = provider
                .create_response(request(next), RequestContext::default())
                .await
                .unwrap();
            assert_eq!(response.response.wire()["status"], "completed");
            fixture.captured().await;
        }
        assert_eq!(reads.load(Ordering::SeqCst), 6);
    }
}

#[tokio::test]
async fn runtime_history_controls_cannot_weaken_carrier_scope_prefix_version_or_tool_policy() {
    for version in 1..=3 {
        let first_native = match version {
            1 => native(),
            2 => tool_native(),
            _ => custom_native(),
        };
        let mut fixture =
            Fixture::start(vec![Reply::json(first_native), Reply::json(native())]).await;
        let (broker, reads) = fixture_broker(Some(KEY));
        let mut provider = fixture
            .provider(
                broker,
                vec![metadata("fixture", "native-fixture")],
                limits(),
            )
            .with_runtime_context()
            .with_native_history();
        provider = match version {
            1 => provider,
            2 => provider.with_native_tools(),
            _ => provider.with_custom_tool_mapping(),
        };
        let mut source = match version {
            1 => {
                json!({"model":"fixture","input":[{"role":"developer","content":"Priority"},{"role":"user","content":"Start"}]})
            }
            2 => tool_source(false),
            _ => custom_source(false),
        };
        source["reasoning"] = json!({"summary":"auto","context":"all_turns"});
        source["include"] = json!(["reasoning.encrypted_content"]);
        let first = provider
            .create_response(request(source.clone()), RequestContext::default())
            .await
            .unwrap()
            .response;
        fixture.captured().await;
        let good = if version == 1 {
            followup(&source, first.output())
        } else {
            custom_next(&source, first.output())
        };
        let h = match version {
            1 => capsule(first.output()),
            2 => tools_capsule(first.output()),
            _ => custom_capsule(first.output()),
        };
        for case in 0..15 {
            let mut wire = good.clone();
            let mut history = h.clone();
            match case {
                0 => history["scope"]["credential"]["profile"] = "other".into(),
                1 => history["native_model"] = "other".into(),
                2 => history["version"] = 4.into(),
                3 => wire["input"][0]["content"] = "Changed".into(),
                4 => wire["input"][2]["summary"][0]["text"] = "Changed".into(),
                5 => history["request"]["include"] = json!(["reasoning.encrypted_content"]),
                6 => {
                    history["request"]["reasoning"] =
                        json!({"summary":"auto","context":"all_turns"})
                }
                7 => {
                    wire["input"].as_array_mut().unwrap().remove(3);
                }
                8 => history["request"]["reasoning"] = Value::Null,
                9 => history["request"]["reasoning"] = json!({}),
                10 => history["request"]["reasoning"] = "high".into(),
                11 => history["request"]["reasoning"] = json!({"effort":null}),
                12 => history["request"]["reasoning"] = json!({"effort":3}),
                13 => history["request"]["reasoning"] = json!({"effort":"future"}),
                14 => {
                    history["request"]["reasoning"] = json!({"effort":"high","context":"all_turns"})
                }
                _ => unreachable!(),
            }
            wire["input"][2]["encrypted_content"] =
                format!("caidex.qwen.native-history.v{version}:{history}").into();
            for streaming in [false, true] {
                wire["stream"] = streaming.into();
                let error = if streaming {
                    provider
                        .stream_response(request(wire.clone()), RequestContext::default())
                        .await
                        .err()
                        .unwrap_or_else(|| {
                            panic!(
                                "v{version} case {case} streaming={streaming} accepted bad carrier"
                            )
                        })
                } else {
                    provider
                        .create_response(request(wire.clone()), RequestContext::default())
                        .await
                        .err()
                        .unwrap_or_else(|| {
                            panic!(
                                "v{version} case {case} streaming={streaming} accepted bad carrier"
                            )
                        })
                };
                assert_eq!(error.http_status, 400, "v{version} case {case}");
                assert_eq!(reads.load(Ordering::SeqCst), 1);
            }
        }
        provider
            .create_response(request(good), RequestContext::default())
            .await
            .unwrap();
        fixture.captured().await;
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}

mod lite;

mod runtime_ids;
