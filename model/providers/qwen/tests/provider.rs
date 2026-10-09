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
