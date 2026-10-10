use caidex_credentials::{Broker, CredentialRef, Id, Secret, SecretKind, SecretStore};
use caidex_model_core::{
    CanonicalRequest, ModelMetadata, ModelProvider, ProviderStreamEvent, RequestContext,
    ResponsesDialect, ResponsesStream, StreamState,
};
use caidex_provider_chat_completions::{ChatCompletionsConfig, ChatCompletionsProvider, Limits};
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
const KEY: &str = "CAIDEX_SYNTHETIC_CHAT_ONLY";
const WAIT: Duration = Duration::from_secs(5);
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
fn bind_alias(value: &mut Value, alias: &Value) {
    match value {
        Value::String(text) if text == "FROM_REQUEST" => *value = alias.clone(),
        Value::String(text) if text == "FROM_REQUEST_PREFIX" => {
            *value = alias.as_str().unwrap()[..20].into()
        }
        Value::String(text) if text == "FROM_REQUEST_SUFFIX" => {
            *value = alias.as_str().unwrap()[20..].into()
        }
        Value::Array(values) => {
            for value in values {
                bind_alias(value, alias);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                bind_alias(value, alias);
            }
        }
        _ => (),
    }
}
fn reference() -> CredentialRef {
    CredentialRef {
        owner: Id::new("executor").unwrap(),
        provider: Id::new("custom-chat").unwrap(),
        profile: Id::new("fixture").unwrap(),
        kind: SecretKind::ApiKey,
    }
}
struct Fixture {
    endpoint: String,
    requests: mpsc::UnboundedReceiver<Value>,
    closed: mpsc::UnboundedReceiver<()>,
    task: JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn start(reply: Value, streaming: bool, suffix: &str, status: u16, stall: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let (tx, requests) = mpsc::unbounded_channel();
        let (ct, closed) = mpsc::unbounded_channel();
        let suffix = suffix.to_owned();
        let task = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (tx, ct, reply, suffix) =
                    (tx.clone(), ct.clone(), reply.clone(), suffix.clone());
                tasks.spawn(async move {
                    let mut headers=Vec::new();
                    while !headers.ends_with(b"\r\n\r\n") { headers.push(socket.read_u8().await.unwrap()); assert!(headers.len()<32768); }
                    let headers=String::from_utf8(headers).unwrap();
                    assert!(headers.starts_with("POST /v1/chat/completions "));
                    assert!(headers.contains(&format!("Bearer {KEY}")));
                    assert!(!headers.to_lowercase().contains("session_id:"));
                    assert!(!headers.to_lowercase().contains("x-openai-internal-codex-responses-lite"));
                    let len=headers.lines().find_map(|l| {let (k,v)=l.split_once(':')?; k.eq_ignore_ascii_case("content-length").then(||v.trim().parse::<usize>().unwrap())}).unwrap();
                    let mut bytes=vec![0;len];socket.read_exact(&mut bytes).await.unwrap();
                    let body:Value=serde_json::from_slice(&bytes).unwrap();
                    tx.send(body.clone()).unwrap();
                    let mut reply=reply;
                    bind_alias(&mut reply, &body["tools"][0]["function"]["name"]);
                    let data=if streaming {
                        if reply.is_array() {reply.as_array().unwrap().iter().map(|v|format!("data: {v}\n\n")).collect::<String>()+&suffix}
                        else {reply.as_str().unwrap().to_owned()+&suffix}
                    } else {reply.to_string()};
                    let content_type=if streaming {"text/event-stream"} else {"application/json"};
                    let header=if stall {format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {content_type}\r\nRetry-After: 2\r\nTransfer-Encoding: chunked\r\nx-request-id: chat-fixture\r\n\r\n")} else {format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {content_type}\r\nRetry-After: 2\r\nContent-Length: {}\r\nx-request-id: chat-fixture\r\n\r\n",data.len())};
                    socket.write_all(header.as_bytes()).await.unwrap();
                    if stall {
                        if !data.is_empty() { let frame=format!("{:x}\r\n{data}\r\n", data.len());let _=socket.write_all(frame.as_bytes()).await; }
                        let _=socket.read(&mut[0]).await;
                    } else {let _=socket.write_all(data.as_bytes()).await;}
                    let _=ct.send(());
                });
                while tasks.try_join_next().is_some() {}
            }
        });
        Self {
            endpoint,
            requests,
            closed,
            task,
        }
    }
    async fn request(&mut self) -> Value {
        tokio::time::timeout(WAIT, self.requests.recv())
            .await
            .unwrap()
            .unwrap()
    }
}
fn provider(
    endpoint: &str,
    lite: bool,
    limits: Limits,
) -> (ChatCompletionsProvider<Store>, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let broker = Arc::new(Broker::new(
        Id::new("executor").unwrap(),
        Store(reads.clone()),
    ));
    let mut config = ChatCompletionsConfig::new(endpoint, Some(reference()))
        .unwrap()
        .with_local_runtime_context();
    let mut dialects = vec![ResponsesDialect::Classic];
    if lite {
        config = config.with_lite();
        dialects.push(ResponsesDialect::Lite);
    }
    let model = ModelMetadata::configured("public".into(), "native".into(), dialects);
    (
        ChatCompletionsProvider::new(config, vec![model], broker, limits).unwrap(),
        reads,
    )
}
fn request(wire: Value, lite: bool) -> CanonicalRequest {
    CanonicalRequest::new(
        wire,
        if lite {
            ResponsesDialect::Lite
        } else {
            ResponsesDialect::Classic
        },
    )
    .unwrap()
}
fn text_reply(finish: &str) -> Value {
    json!({"id":"chat-1","object":"chat.completion","created":1,"model":"native","choices":[{"index":0,"message":{"role":"assistant","content":"中文🙂"},"finish_reason":finish}],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8,"prompt_tokens_details":{"cached_tokens":2},"completion_tokens_details":{"reasoning_tokens":1}}})
}
fn chunk(delta: Value, finish: Value) -> Value {
    json!({"id":"chat-1","object":"chat.completion.chunk","created":1,"model":"native","choices":[{"index":0,"delta":delta,"finish_reason":finish}],"usage":null})
}
#[tokio::test]
async fn six_methods_stateless_json_text_usage_and_incomplete_are_explicit() {
    for finish in ["stop", "length", "content_filter"] {
        let mut fixture = Fixture::start(text_reply(finish), false, "", 200, false).await;
        let (provider, reads) = provider(&fixture.endpoint, false, Limits::default());
        assert_eq!(provider.list_models().await.unwrap().len(), 1);
        assert!(
            provider
                .metadata("public")
                .unwrap()
                .codex_compatibility
                .is_none()
        );
        assert!(
            provider
                .capabilities("public")
                .unwrap()
                .context_window
                .is_none()
        );
        assert!(matches!(
            provider.credential_requirements("public").unwrap(),
            caidex_model_core::CredentialRequirement::Bearer { .. }
        ));
        let response=provider.create_response(request(json!({"model":"public","input":"你好","instructions":"system","max_output_tokens":100,"store":false}),false),RequestContext::default()).await.unwrap().response;
        assert_eq!(response.output_text().collect::<Vec<_>>(), vec!["中文🙂"]);
        assert_eq!(
            response.wire()["usage"]["input_tokens_details"]["cached_tokens"],
            2
        );
        assert_eq!(
            response.wire()["usage"]["output_tokens_details"]["reasoning_tokens"],
            1
        );
        assert_eq!(
            response.state(),
            if finish == "stop" {
                StreamState::Completed
            } else {
                StreamState::Incomplete
            }
        );
        let wire = fixture.request().await;
        assert_eq!(wire["model"], "native");
        assert_eq!(wire["max_completion_tokens"], 100);
        assert_eq!(wire["store"], false);
        assert_eq!(wire["messages"][0]["role"], "system");
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn unsupported_reasoning_state_media_and_controls_never_resolve_keys() {
    let (provider, reads) = provider(
        "http://127.0.0.1:1/v1/chat/completions",
        false,
        Limits::default(),
    );
    let mutations = vec![
        json!({"previous_response_id":"foreign"}),
        json!({"reasoning":{"effort":"high"}}),
        json!({"reasoning":{"effort":"none","summary":"auto"}}),
        json!({"include":["reasoning.encrypted_content"]}),
        json!({"store":true}),
        json!({"service_tier":"priority"}),
        json!({"text":{"format":{"type":"json_schema"}}}),
        json!({"input":[{"type":"reasoning","encrypted_content":"foreign"}]}),
        json!({"input":[{"role":"user","content":[{"type":"input_image","image_url":"https://example.com"}]}]}),
        json!({"tools":[{"type":"web_search"}]}),
        json!({"tools":[{"type":"custom","name":"x","format":{"type":"grammar","definition":"..."}}]}),
        json!({"tools":[{"type":"function","name":"x","parameters":{},"defer_loading":true}]}),
        json!({"input":[{"type":"function_call_output","call_id":"orphan","output":"result"}]}),
        json!({"temperature":3}),
        json!({"parallel_tool_calls":"false"}),
    ];
    for mutation in mutations {
        let mut wire = json!({"model":"public","input":"hello"});
        wire.as_object_mut()
            .unwrap()
            .extend(mutation.as_object().unwrap().clone());
        assert_eq!(
            provider
                .create_response(request(wire, false), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            400
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn json_tools_roundtrip_namespace_custom_arguments_and_result_text() {
    for custom in [false, true] {
        let tool = if custom {
            json!({"type":"custom","name":"exec","format":{"type":"text"}})
        } else {
            json!({"type":"function","name":"exec","parameters":{"type":"object"}})
        };
        let mut reply = text_reply("tool_calls");
        reply["choices"][0]["message"] = json!({"role":"assistant","content":null,"tool_calls":[{"id":"call-1","type":"function","function":{"name":"FROM_REQUEST","arguments":if custom {"{\"input\":\"print(1)\"}"} else {" { \"n\" : 1.00 } "}}}]});
        let mut fixture = Fixture::start(reply, false, "", 200, false).await;
        let (provider, reads) = provider(&fixture.endpoint, true, Limits::default());
        let declarations = json!([{"type":"namespace","name":"functions","tools":[tool]}]);
        let first = json!({"model":"public","input":[{"type":"additional_tools","id":"at_stable","role":"developer","tools":declarations},{"role":"user","content":"run"}],"parallel_tool_calls":false});
        let response = provider
            .create_response(request(first, true), RequestContext::default())
            .await
            .unwrap()
            .response;
        let item = response.output()[0].clone();
        assert_eq!(item["namespace"], "functions");
        assert_eq!(item["name"], "exec");
        let first_wire = fixture.request().await;
        let result = json!({"type":if custom {"custom_tool_call_output"} else {"function_call_output"},"call_id":"call-1","output":"exact result\n🙂"});
        let history = json!({"model":"public","input":[{"role":"user","content":"run"},item,result],"tools":declarations});
        provider
            .create_response(request(history, false), RequestContext::default())
            .await
            .unwrap();
        let next = fixture.request().await;
        assert_eq!(next["tools"], first_wire["tools"]);
        assert_eq!(
            next["messages"][1]["tool_calls"][0]["function"]["arguments"],
            if custom {
                "{\"input\":\"print(1)\"}"
            } else {
                " { \"n\" : 1.00 } "
            }
        );
        assert_eq!(next["messages"][2]["content"], "exact result\n🙂");
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}
#[tokio::test]
async fn sse_terminal_usage_and_canonical_lifecycle_validate() {
    let chunks = json!([chunk(json!({"role":"assistant","content":"中"}),Value::Null),chunk(json!({"content":"文🙂"}),json!("stop")),{"id":"chat-1","object":"chat.completion.chunk","created":1,"model":"native","choices":[],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}}]);
    let mut fixture = Fixture::start(chunks, true, "data: [DONE]\n\n", 200, false).await;
    let (provider, reads) = provider(&fixture.endpoint, false, Limits::default());
    let mut response = provider
        .stream_response(
            request(json!({"model":"public","input":"hi","stream":true}), false),
            RequestContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers.get("x-request-id"), Some("chat-fixture"));
    let mut parser = ResponsesStream::new(1024 * 1024).unwrap();
    let mut terminal = None;
    while let Some(event) = response.events.next().await {
        if let ProviderStreamEvent::Model(event) = event.unwrap() {
            parser
                .push(
                    format!(
                        "event: {}\ndata: {}\n\n",
                        event.frame.event, event.frame.data
                    )
                    .as_bytes(),
                )
                .unwrap();
            if event.response.kind() == "response.completed" {
                terminal = Some(event.response.wire()["response"].clone());
            }
        }
    }
    assert_eq!(parser.finish().unwrap(), StreamState::Completed);
    assert_eq!(terminal.unwrap()["usage"]["total_tokens"], 3);
    assert_eq!(
        fixture.request().await["stream_options"],
        json!({"include_usage":true})
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn sse_truncation_identity_drift_reasoning_and_finish_errors_deliver_no_calls() {
    let cases = vec![
        (
            json!([chunk(
                json!({"role":"assistant","content":"partial"}),
                Value::Null
            )]),
            "",
        ),
        (
            json!([chunk(json!({"role":"assistant"}), Value::Null)]),
            "data: [DONE]\n\n",
        ),
        (
            json!([chunk(
                json!({"role":"assistant","reasoning_content":"private"}),
                json!("stop")
            )]),
            "data: [DONE]\n\n",
        ),
        (
            json!([chunk(json!({"role":"assistant"}), json!("function_call"))]),
            "data: [DONE]\n\n",
        ),
        (
            json!([chunk(json!({"role":"assistant"}),Value::Null),{"id":"different","object":"chat.completion.chunk","created":1,"model":"native","choices":[]}]),
            "data: [DONE]\n\n",
        ),
    ];
    for (chunks, suffix) in cases {
        let mut fixture = Fixture::start(chunks, true, suffix, 200, false).await;
        let (provider, _) = provider(&fixture.endpoint, false, Limits::default());
        let mut events = provider
            .stream_response(
                request(json!({"model":"public","input":"hi","stream":true}), false),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .events;
        assert_eq!(events.next().await.unwrap().err().unwrap().http_status, 502);
        assert!(events.next().await.is_none());
        fixture.request().await;
    }
}
#[tokio::test]
async fn http_errors_rate_limit_and_no_retry_do_not_expose_native_diagnostics() {
    for status in [401, 403, 429, 302, 500] {
        let mut fixture =
            Fixture::start(json!({"error":{"message":KEY}}), false, "", status, false).await;
        let (provider, reads) = provider(&fixture.endpoint, false, Limits::default());
        let error = provider
            .create_response(
                request(json!({"model":"public","input":"hi"}), false),
                RequestContext::default(),
            )
            .await
            .err()
            .unwrap();
        assert!(!format!("{error:?}").contains(KEY));
        assert_eq!(
            error.retry_after_seconds,
            if status == 429 { Some(2) } else { None }
        );
        fixture.request().await;
        assert!(fixture.requests.try_recv().is_err());
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn cancellation_idle_deadline_drop_close_socket_and_release_shared_slot() {
    for mode in ["cancel", "deadline", "idle", "drop"] {
        let mut fixture = Fixture::start(json!(""), true, "", 200, true).await;
        let limits = Limits {
            in_flight: 1,
            idle_timeout: Duration::from_millis(50),
            total_timeout: Duration::from_secs(1),
            ..Limits::default()
        };
        let (provider, reads) = provider(&fixture.endpoint, false, limits);
        let mut context = RequestContext::default();
        let cancel = context.cancellation.clone();
        if mode == "deadline" {
            context.deadline = Some(std::time::Instant::now() + Duration::from_millis(50));
        }
        let mut events = provider
            .stream_response(
                request(json!({"model":"public","input":"hi","stream":true}), false),
                context,
            )
            .await
            .unwrap()
            .events;
        fixture.request().await;
        if mode == "cancel" {
            cancel.cancel();
        }
        if mode != "drop" {
            let error = events.next().await.unwrap().err().unwrap();
            assert_eq!(
                error.code,
                if mode == "cancel" {
                    "provider_cancelled"
                } else {
                    "provider_timeout"
                }
            );
        }
        drop(events);
        tokio::time::timeout(WAIT, fixture.closed.recv())
            .await
            .unwrap()
            .unwrap();
        let response = provider
            .stream_response(
                request(
                    json!({"model":"public","input":"recover","stream":true}),
                    false,
                ),
                RequestContext::default(),
            )
            .await
            .unwrap();
        drop(response);
        fixture.request().await;
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn sse_tool_arguments_are_fragmented_but_delivered_only_after_valid_done() {
    for (finish, suffix, succeeds) in [
        ("tool_calls", "data: [DONE]\n\n", true),
        ("tool_calls", "", false),
        ("length", "data: [DONE]\n\n", false),
    ] {
        let chunks = json!([
            chunk(
                json!({"role":"assistant","tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"FROM_REQUEST_PREFIX","arguments":" { \"n\" : "}}]}),
                Value::Null
            ),
            chunk(
                json!({"tool_calls":[{"index":0,"id":null,"type":null,"function":{"name":"FROM_REQUEST_SUFFIX","arguments":"1.00 } "}}]}),
                json!(finish)
            )
        ]);
        let mut fixture = Fixture::start(chunks, true, suffix, 200, false).await;
        let (provider, _) = provider(&fixture.endpoint, false, Limits::default());
        let mut events=provider.stream_response(request(json!({"model":"public","input":"hi","stream":true,"parallel_tool_calls":false,"tools":[{"type":"function","name":"exec","parameters":{"type":"object"}}]}),false),RequestContext::default()).await.unwrap().events;
        let mut calls = Vec::new();
        let mut failed = false;
        while let Some(event) = events.next().await {
            match event {
                Err(_) => failed = true,
                Ok(ProviderStreamEvent::Model(event))
                    if event.response.kind() == "response.output_item.done" =>
                {
                    calls.push(event.response.wire()["item"].clone())
                }
                _ => (),
            }
        }
        assert_eq!(failed, !succeeds);
        assert_eq!(calls.len(), usize::from(succeeds));
        if succeeds {
            assert_eq!(calls[0]["arguments"], " { \"n\" : 1.00 } ");
        }
        fixture.request().await;
    }
}
#[tokio::test]
async fn malformed_json_usage_tool_aliases_and_parallel_violation_fail_closed() {
    let mut cases = Vec::new();
    for mutation in [
        json!({"usage":{"prompt_tokens":-1}}),
        json!({"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":6}}),
        json!({"usage":{"prompt_tokens":2,"prompt_tokens_details":{"cached_tokens":3}}}),
        json!({"model":"different"}),
        json!({"choices":[]}),
        json!({"created":"wrong"}),
        json!({"error":{"message":KEY}}),
    ] {
        let mut reply = text_reply("stop");
        reply
            .as_object_mut()
            .unwrap()
            .extend(mutation.as_object().unwrap().clone());
        cases.push(reply);
    }
    for message in [
        json!({"role":"assistant","content":null,"reasoning_content":"private"}),
        json!({"role":"assistant","content":null,"tool_calls":[{"id":"c","type":"function","function":{"name":"unknown","arguments":"{}"}}]}),
    ] {
        let mut reply = text_reply("tool_calls");
        reply["choices"][0]["message"] = message;
        cases.push(reply);
    }
    let mut parallel = text_reply("tool_calls");
    parallel["choices"][0]["message"] = json!({"role":"assistant","content":null,"tool_calls":[{"id":"c1","type":"function","function":{"name":"FROM_REQUEST","arguments":"{}"}},{"id":"c2","type":"function","function":{"name":"FROM_REQUEST","arguments":"{}"}}]});
    cases.push(parallel);
    for reply in cases {
        let mut fixture = Fixture::start(reply, false, "", 200, false).await;
        let (provider, _) = provider(&fixture.endpoint, false, Limits::default());
        let error=provider.create_response(request(json!({"model":"public","input":"hi","parallel_tool_calls":false,"tools":[{"type":"function","name":"exec","parameters":{}}]}),false),RequestContext::default()).await.err().unwrap();
        assert_eq!(error.http_status, 502);
        assert!(!format!("{error:?}").contains(KEY));
        fixture.request().await;
    }
}
#[tokio::test]
async fn history_duplicate_orphan_wrong_kind_and_opaque_are_rejected_before_keys() {
    let (provider, reads) = provider(
        "http://127.0.0.1:1/v1/chat/completions",
        false,
        Limits::default(),
    );
    let call = json!({"type":"function_call","name":"exec","call_id":"c","arguments":"{}"});
    let result = json!({"type":"function_call_output","call_id":"c","output":"done"});
    let cases = vec![
        json!([call]),
        json!([call, call, result]),
        json!([call,{"role":"user","content":"interleaved"},result]),
        json!([call,{"type":"custom_tool_call_output","call_id":"c","output":"wrong"}]),
        json!([{"type":"compaction","encrypted_content":"foreign"}]),
        json!([{"type":"message","role":"assistant","content":[{"type":"output_text","text":"visible","signature":"foreign"}]}]),
    ];
    for input in cases {
        assert_eq!(provider.create_response(request(json!({"model":"public","input":input,"tools":[{"type":"function","name":"exec","parameters":{}}]}),false),RequestContext::default()).await.err().unwrap().http_status,400);
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn budgets_unknown_routes_and_precancelled_deadlines_are_local() {
    let (provider, reads) = provider(
        "http://127.0.0.1:1/v1/chat/completions",
        false,
        Limits {
            request_bytes: 64,
            ..Limits::default()
        },
    );
    assert_eq!(
        provider
            .create_response(
                request(json!({"model":"public","input":"x".repeat(100)}), false),
                RequestContext::default()
            )
            .await
            .err()
            .unwrap()
            .http_status,
        413
    );
    assert_eq!(
        provider
            .create_response(
                request(json!({"model":"unknown","input":"hi"}), false),
                RequestContext::default()
            )
            .await
            .err()
            .unwrap()
            .http_status,
        404
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let (provider, reads) = self::provider(
        "http://127.0.0.1:1/v1/chat/completions",
        false,
        Limits::default(),
    );
    for cancelled in [true, false] {
        let mut context = RequestContext::default();
        if cancelled {
            context.cancellation.cancel();
        } else {
            context.deadline = Some(std::time::Instant::now());
        }
        assert_eq!(
            provider
                .create_response(
                    request(json!({"model":"public","input":"hi"}), false),
                    context
                )
                .await
                .err()
                .unwrap()
                .code,
            if cancelled {
                "provider_cancelled"
            } else {
                "provider_timeout"
            }
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    for streaming in [true, false] {
        let mut fixture = Fixture::start(
            if streaming {
                json!([chunk(
                    json!({"role":"assistant","content":"x".repeat(1000)}),
                    json!("stop")
                )])
            } else {
                text_reply("stop")
            },
            streaming,
            "data: [DONE]\n\n",
            200,
            false,
        )
        .await;
        let (provider, _) = self::provider(
            &fixture.endpoint,
            false,
            Limits {
                response_bytes: 64,
                ..Limits::default()
            },
        );
        if streaming {
            let mut events = provider
                .stream_response(
                    request(json!({"model":"public","input":"hi","stream":true}), false),
                    RequestContext::default(),
                )
                .await
                .unwrap()
                .events;
            assert!(events.next().await.unwrap().is_err());
        } else {
            assert!(
                provider
                    .create_response(
                        request(json!({"model":"public","input":"hi"}), false),
                        RequestContext::default()
                    )
                    .await
                    .is_err()
            );
        }
        fixture.request().await;
    }
}
#[test]
fn endpoint_and_dialect_configuration_are_executor_owned() {
    for endpoint in [
        "http://example.com/v1/chat/completions",
        "http://localhost/v1/chat/completions",
        "https://example.com/v1/responses",
        "https://user:secret@example.com/v1/chat/completions",
        "https://example.com/v1/chat/completions?key=secret",
        "https://example.com/v1/chat/completions#secret",
    ] {
        assert!(ChatCompletionsConfig::new(endpoint, None).is_err());
    }
    let reads = Arc::new(AtomicUsize::new(0));
    let broker = Arc::new(Broker::new(Id::new("executor").unwrap(), Store(reads)));
    let config =
        ChatCompletionsConfig::new("https://example.com/v1/chat/completions", None).unwrap();
    assert!(!format!("{config:?}").contains("example"));
    assert!(
        ChatCompletionsProvider::new(
            config,
            vec![ModelMetadata::configured(
                "public".into(),
                "native".into(),
                vec![ResponsesDialect::Lite]
            )],
            broker,
            Limits::default()
        )
        .is_err()
    );
}

#[tokio::test]
async fn router_and_default_runtime_policies_do_not_infer_or_fallback() {
    let (provider, reads) = provider(
        "http://127.0.0.1:1/v1/chat/completions",
        false,
        Limits::default(),
    );
    let router =
        caidex_model_core::ModelRouter::new(vec![("public".into(), Arc::new(provider))]).unwrap();
    assert_eq!(router.list_models().await.unwrap()[0].id, "public");
    assert_eq!(
        router
            .create_response(
                request(json!({"model":"missing","input":"hi"}), false),
                RequestContext::default()
            )
            .await
            .err()
            .unwrap()
            .http_status,
        404
    );
    assert_eq!(router.create_response(request(json!({"model":"public","input":"hi","reasoning":{"effort":"none"},"include":["reasoning.encrypted_content"]}),false),RequestContext::default()).await.err().unwrap().http_status,400);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn explicit_no_reasoning_runtime_and_grammar_prompt_profile_has_clear_limits() {
    let mut fixture = Fixture::start(text_reply("stop"), false, "", 200, false).await;
    let reads = Arc::new(AtomicUsize::new(0));
    let broker = Arc::new(Broker::new(
        Id::new("executor").unwrap(),
        Store(reads.clone()),
    ));
    let config = ChatCompletionsConfig::new(&fixture.endpoint, Some(reference()))
        .unwrap()
        .with_no_reasoning_runtime()
        .with_grammar_prompt_mapping();
    let provider = ChatCompletionsProvider::new(
        config,
        vec![ModelMetadata::configured(
            "public".into(),
            "native".into(),
            vec![ResponsesDialect::Classic],
        )],
        broker,
        Limits::default(),
    )
    .unwrap();
    let wire = json!({"model":"public","input":"hi","reasoning":{"effort":"none"},"include":["reasoning.encrypted_content"],"tools":[{"type":"custom","name":"exec","format":{"type":"grammar","syntax":"regex","definition":"[a-z]+"}}]});
    provider
        .create_response(request(wire.clone(), false), RequestContext::default())
        .await
        .unwrap();
    let native = fixture.request().await;
    assert!(native.get("reasoning").is_none() && native.get("include").is_none());
    assert!(
        native["tools"][0]["function"]["description"]
            .as_str()
            .unwrap()
            .contains("not native enforcement")
    );
    assert!(
        native["tools"][0]["function"]["description"]
            .as_str()
            .unwrap()
            .contains("[a-z]+")
    );
    assert_eq!(
        native["tools"][0]["function"]["parameters"]["properties"]["input"]["type"],
        "string"
    );
    for reasoning in [
        json!({"effort":"high"}),
        json!({"effort":"none","summary":"auto"}),
    ] {
        let mut wire = wire.clone();
        wire["reasoning"] = reasoning;
        assert_eq!(
            provider
                .create_response(request(wire, false), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            400
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn tool_choice_is_enforced_and_unknown_usage_is_preserved() {
    for choice in ["none", "required"] {
        let mut reply = text_reply(if choice == "none" {
            "tool_calls"
        } else {
            "stop"
        });
        if choice == "none" {
            reply["choices"][0]["message"] = json!({"role":"assistant","content":null,"tool_calls":[{"id":"c","type":"function","function":{"name":"FROM_REQUEST","arguments":"{}"}}]});
        }
        let mut fixture = Fixture::start(reply, false, "", 200, false).await;
        let (provider, _) = provider(&fixture.endpoint, false, Limits::default());
        assert_eq!(provider.create_response(request(json!({"model":"public","input":"hi","tool_choice":choice,"tools":[{"type":"function","name":"exec","parameters":{}}]}),false),RequestContext::default()).await.err().unwrap().code,"chat_tool_choice_violation");
        fixture.request().await;
    }
    let mut reply = text_reply("stop");
    reply["usage"]["future_accounting"] = json!({"large":18446744073709551616_u128});
    let mut fixture = Fixture::start(reply, false, "", 200, false).await;
    let (provider, _) = provider(&fixture.endpoint, false, Limits::default());
    let response = provider
        .create_response(
            request(json!({"model":"public","input":"hi"}), false),
            RequestContext::default(),
        )
        .await
        .unwrap()
        .response;
    assert_eq!(
        response.wire()["usage"]["native_chat_usage"]["future_accounting"],
        json!({"large":18446744073709551616_u128})
    );
    fixture.request().await;
}
