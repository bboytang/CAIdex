use super::*;
use caidex_model_core::{
    CanonicalRequest, CapabilitySupport, CredentialRequirement, EvidenceSource, ModelMetadata,
    ModelProvider, ModelRegistry, ProviderStreamEvent, ResponsesDialect,
};
use caidex_provider_google::{
    GeminiModel, GeminiProvider, NativeHistory, ReasoningMapping, ServiceTierMapping,
    SummaryMapping, ToolMap, VerbosityMapping,
};

const MODEL: &str = "models/fixture-provider";
const LIMIT: usize = 256 * 1024;
fn profile() -> GeminiModel {
    GeminiModel::new(
        ModelMetadata::configured(
            "alias".into(),
            MODEL.into(),
            vec![ResponsesDialect::Classic, ResponsesDialect::Lite],
        ),
        128,
        8,
    )
}
fn canonical(
    dialect: ResponsesDialect,
    input: Vec<Value>,
    declarations: &Value,
    stream: bool,
) -> CanonicalRequest {
    let mut source = json!({"model":"alias","input":input,"stream":stream,"store":false,"parallel_tool_calls":true,"include":["reasoning.encrypted_content"]});
    if dialect == ResponsesDialect::Lite {
        source["input"].as_array_mut().unwrap().insert(
            0,
            json!({"type":"additional_tools","role":"developer","tools":declarations}),
        );
    } else {
        source["tools"] = declarations.clone();
    }
    CanonicalRequest::new(source, dialect).unwrap()
}

// Catches config/metadata touching credentials, accepting unusable routes or
// conflicting declarations, or granting compatibility from a model catalog.
#[tokio::test]
async fn six_method_profiles_and_catalog_preserve_evidence_and_api_key_ownership() {
    let mut fixture = Fixture::start(vec![Reply::json(json!({"models":[model(MODEL)]}))]).await;
    let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
    let mut absent = profile();
    absent.metadata.id = "absent".into();
    absent.metadata.native_model = "models/unavailable".into();
    let provider = GeminiProvider::new(client, vec![profile(), absent], 8).unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let metadata = provider.metadata("alias").unwrap();
    assert_eq!(metadata.source, EvidenceSource::Configured);
    assert!(metadata.codex_compatibility.is_none());
    assert_eq!(
        provider.capabilities("alias").unwrap().vision,
        CapabilitySupport::Unknown
    );
    assert!(
        matches!(provider.credential_requirements("alias").unwrap(),CredentialRequirement::ApiKey{reference:r} if r==reference())
    );
    for missing in ["unknown", "models/fixture-provider"] {
        assert!(provider.metadata(missing).is_err());
        assert!(provider.capabilities(missing).is_err());
        assert!(provider.credential_requirements(missing).is_err());
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let listed = provider.list_models().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "alias");
    assert_eq!(listed[0].source, EvidenceSource::ProviderCatalog);
    assert!(listed[0].codex_compatibility.is_none());
    assert_eq!(listed[0].capabilities.vision, CapabilitySupport::Unknown);
    assert_eq!(ModelRegistry::new(listed).unwrap().models().count(), 1);
    assert!(
        fixture
            .request()
            .await
            .starts_with("GET /proxy/v1beta/models?pageSize=1000 HTTP/1.1\r\n")
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    for case in 0..9 {
        let (client, reads) = super::client(&fixture.base, Some(KEY), Limits::default());
        let mut p = profile();
        let mut profiles = Vec::new();
        let mut maximum = 8;
        match case {
            0 => {}
            1 => {
                profiles.push(profile());
                profiles.push(profile());
            }
            2 => {
                maximum = 0;
                profiles.push(profile());
            }
            3 => {
                p.metadata.native_model = "models/../private".into();
                profiles.push(p);
            }
            4 => {
                p.max_tokens = 0;
                profiles.push(p);
            }
            5 => {
                p.metadata.capabilities.output_limit = Some(64);
                profiles.push(p);
            }
            6 => {
                p.metadata.dialects = vec![];
                profiles.push(p);
            }
            7 => {
                p.verbosity_mappings = vec![
                    VerbosityMapping::new("low".into(), "one".into()).unwrap(),
                    VerbosityMapping::new("low".into(), "two".into()).unwrap(),
                ];
                profiles.push(p);
            }
            _ => {
                p.image_mime_types = vec!["application/pdf".into()];
                profiles.push(p);
            }
        }
        assert!(
            GeminiProvider::new(client, profiles, maximum).is_err(),
            "case{case}"
        );
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }
}

// Catches bypassing preflight compiler/dialect/stream mode guards, unsolicited
// context forwarding, retries on rejection, or credentials read for invalid input.
#[tokio::test]
async fn provider_invalid_requests_fail_before_credentials_or_native_post() {
    let fixture = Fixture::start(vec![]).await;
    let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
    let mut p = profile();
    p.metadata.dialects = vec![ResponsesDialect::Classic];
    p.retain_runtime_metadata = true;
    let provider = GeminiProvider::new(client, vec![p], 8).unwrap();
    let valid = canonical(
        ResponsesDialect::Classic,
        vec![json!({"role":"user","content":"q"})],
        &json!([]),
        false,
    );
    assert_eq!(
        provider
            .create_response(
                canonical(
                    ResponsesDialect::Lite,
                    vec![json!({"role":"user","content":"q"})],
                    &json!([]),
                    false
                ),
                RequestContext::default()
            )
            .await
            .err()
            .unwrap()
            .code,
        "unsupported_dialect"
    );
    assert_eq!(
        provider
            .create_response(
                canonical(
                    ResponsesDialect::Classic,
                    vec![json!({"role":"user","content":"q"})],
                    &json!([]),
                    true
                ),
                RequestContext::default()
            )
            .await
            .err()
            .unwrap()
            .code,
        "invalid_model_request"
    );
    assert_eq!(
        provider
            .stream_response(valid.clone(), RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "invalid_model_request"
    );
    for (key, value, code) in [
        ("model", json!("missing"), "unknown_model"),
        (
            "stream_options",
            json!({"reasoning_summary_delivery":"sequential_cutoff"}),
            "unsupported_google_runtime_parameter",
        ),
        (
            "text",
            json!({"verbosity":"low"}),
            "unsupported_google_output_format",
        ),
    ] {
        let mut wire = valid.wire().clone();
        wire[key] = value;
        assert_eq!(
            provider
                .create_response(
                    CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap(),
                    RequestContext::default()
                )
                .await
                .err()
                .unwrap()
                .code,
            code
        );
    }
    let mut context = RequestContext::default();
    context
        .headers
        .insert("session_id", "private session".into(), REQUEST_HEADERS)
        .unwrap();
    assert_eq!(
        provider
            .create_response(valid.clone(), context)
            .await
            .err()
            .unwrap()
            .code,
        "unsupported_native_context_header"
    );
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert_eq!(
        provider
            .create_response(
                valid,
                RequestContext {
                    cancellation,
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
}

// Catches JSON/SSE routing divergence, loss of exact compiled profile controls,
// signed history/reversed results, source ownership, or IDs reused across turns.
#[tokio::test]
async fn provider_json_sse_and_persisted_three_turn_replay_keep_profiles_and_signed_parts() {
    let declarations = json!([{ "type":"function","name":"echo","parameters":{"type":"object"}},{"type":"custom","name":"raw","format":{"type":"text"}}]);
    let tools = ToolMap::new(declarations.as_array().unwrap(), 8).unwrap();
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let signed = json!({"role":"model","parts":[
            {"thought":true,"text":"summary","thoughtSignature":"PRIVATE_THOUGHT"},
            {"functionCall":{"name":tools.native_tools()[0]["name"],"args":{"n":1}},"thoughtSignature":"PRIVATE_CALL"},
            {"functionCall":{"name":tools.native_tools()[1]["name"],"id":"raw-call","args":{"input":"  原文🙂\n  "}},"future":{"keep":true}}]});
        let first_reply = json!({"responseId":"reused-native-id","modelVersion":"serving-version","candidates":[{"finishReason":"STOP","content":signed}]});
        let second_chunks = vec![
            json!({"candidates":[{"content":{"parts":[{"text":"streamed ","thoughtSignature":"STREAM_A"}]}}]}),
            json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":"answer","thoughtSignature":"STREAM_B"}]}}]}),
            json!({"responseId":"reused-native-id","usageMetadata":{"promptTokenCount":8,"candidatesTokenCount":2,"totalTokenCount":10}}),
        ];
        let third_reply =
            json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":"final"}]}}]});
        let mut r = Reply::json(first_reply.clone());
        r.headers = "x-request-id: first-http\r\nx-codex-turn-state: must-not-forward\r\n".into();
        let mut s = Reply::sse(&second_chunks);
        s.headers = "x-request-id: stream-http\r\n".into();
        let mut fixture = Fixture::start(vec![r, s, Reply::json(third_reply.clone())]).await;
        let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
        let mut p = profile();
        p.retain_runtime_metadata = true;
        p.reasoning_mappings =
            vec![ReasoningMapping::new("high".into(), json!({"thinkingBudget":1024})).unwrap()];
        p.summary_mappings = vec![SummaryMapping::new("auto".into(), true).unwrap()];
        p.verbosity_mappings =
            vec![VerbosityMapping::new("low".into(), "Be brief.".into()).unwrap()];
        p.service_tier_mappings =
            vec![ServiceTierMapping::new("default".into(), "standard".into()).unwrap()];
        let provider = GeminiProvider::new(client, vec![p], 8).unwrap();
        let make = |input: Vec<Value>, stream| {
            let mut source = canonical(dialect, input, &declarations, stream)
                .wire()
                .clone();
            source["reasoning"] = json!({"effort":"high","summary":"auto"});
            source["text"] = json!({"verbosity":"low"});
            source["service_tier"] = "default".into();
            source["client_metadata"] = json!({"local":"retained"});
            source["prompt_cache_key"] = "local hint".into();
            CanonicalRequest::new(source, dialect).unwrap()
        };
        let mut input = vec![
            json!({"role":"developer","content":"fixed"}),
            json!({"role":"user","content":"start"}),
        ];
        let first = provider
            .create_response(make(input.clone(), false), RequestContext::default())
            .await
            .unwrap();
        assert_eq!(first.headers.get("x-request-id"), Some("first-http"));
        assert!(first.headers.get("x-codex-turn-state").is_none());
        let first_body = request_body(&fixture.request().await);
        assert_eq!(
            first_body["generationConfig"],
            json!({"maxOutputTokens":128,"thinkingConfig":{"thinkingBudget":1024,"includeThoughts":true}})
        );
        assert_eq!(
            first_body["systemInstruction"],
            json!({"parts":[{"text":"fixed"},{"text":"Be brief."}]})
        );
        assert_eq!(first_body["serviceTier"], "standard");
        for key in ["client_metadata", "prompt_cache_key", "model", "text"] {
            assert!(first_body.get(key).is_none());
        }
        let history = NativeHistory::from_responses_output(
            first.response.output(),
            MODEL,
            &first_body,
            LIMIT,
        )
        .unwrap();
        assert_eq!(history.native_response(), &first_reply);
        let first_id = first.response.id().to_owned();
        assert_ne!(first_id, "reused-native-id");
        let call_id = first.response.output()[1]["call_id"]
            .as_str()
            .unwrap()
            .to_owned();
        input.extend(first.response.output().to_vec());
        input.extend([
            json!({"type":"custom_tool_call_output","call_id":"raw-call","output":"  原文🙂\n  "}),
            json!({"type":"function_call_output","call_id":call_id,"output":"result"}),
        ]);
        let mut stream = provider
            .stream_response(make(input.clone(), true), RequestContext::default())
            .await
            .unwrap();
        assert_eq!(stream.headers.get("x-request-id"), Some("stream-http"));
        let mut events = Vec::new();
        while let Some(event) = stream.events.next().await {
            if let ProviderStreamEvent::Model(event) = event.unwrap() {
                events.push(event);
            }
        }
        let second_body = request_body(&fixture.request().await);
        assert_eq!(second_body["contents"][1], signed);
        assert_eq!(
            second_body["contents"][2]["parts"][0]["functionResponse"]["response"],
            json!({"output":"result"})
        );
        let final_wire = events.last().unwrap().response.wire();
        assert_eq!(final_wire["type"], "response.completed");
        assert_eq!(
            events
                .iter()
                .filter_map(|e| e.response.text_delta())
                .collect::<String>(),
            "streamed answer"
        );
        assert_ne!(final_wire["response"]["id"], first_id);
        let output = final_wire["response"]["output"].as_array().unwrap();
        let history =
            NativeHistory::from_responses_output(output, MODEL, &second_body, LIMIT).unwrap();
        assert_eq!(history.chunks().unwrap(), second_chunks);
        input.extend(output.iter().cloned());
        input.push(json!({"role":"user","content":"third"}));
        let bytes = serde_json::to_vec(&make(input, false)).unwrap();
        let restored =
            CanonicalRequest::new(serde_json::from_slice(&bytes).unwrap(), dialect).unwrap();
        let third = provider
            .create_response(restored, RequestContext::default())
            .await
            .unwrap();
        let third_body = request_body(&fixture.request().await);
        assert_eq!(third_body["contents"][1], signed);
        assert_eq!(
            third_body["contents"][3]["parts"],
            json!([{"text":"streamed ","thoughtSignature":"STREAM_A"},{"text":"answer","thoughtSignature":"STREAM_B"}])
        );
        assert_eq!(
            NativeHistory::from_responses_output(
                third.response.output(),
                MODEL,
                &third_body,
                LIMIT
            )
            .unwrap()
            .native_response(),
            &third_reply
        );
        assert_eq!(reads.load(Ordering::SeqCst), 3);
    }
}

// Catches accepting duplicate, empty, overlong or non-ASCII native context,
// leaking raw headers, and retrying a rejected response.
#[tokio::test]
async fn provider_rejects_invalid_native_response_headers_without_retry() {
    for stream in [false, true] {
        for headers in [
            "x-request-id: \r\n".to_owned(),
            "x-request-id: one\r\nx-request-id: two\r\n".to_owned(),
            format!("x-request-id: {}\r\n", "x".repeat(8193)),
            "x-request-id: 私密\r\n".to_owned(),
        ] {
            let mut reply = if stream {
                Reply::sse(&[native_reply("STOP")])
            } else {
                Reply::json(native_reply("STOP"))
            };
            reply.headers = headers;
            let mut fixture = Fixture::start(vec![reply]).await;
            let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
            let provider = GeminiProvider::new(client, vec![profile()], 8).unwrap();
            let request = canonical(
                ResponsesDialect::Classic,
                vec![json!({"role":"user","content":"q"})],
                &json!([]),
                stream,
            );
            let error = if stream {
                provider
                    .stream_response(request, RequestContext::default())
                    .await
                    .err()
                    .unwrap()
            } else {
                provider
                    .create_response(request, RequestContext::default())
                    .await
                    .err()
                    .unwrap()
            };
            assert_eq!(error.code, "provider_invalid_response_header");
            assert!(!format!("{error:?}").contains("私密"));
            fixture.request().await;
            assert_eq!(reads.load(Ordering::SeqCst), 1);
        }
    }
}

// Catches wrapper queues hiding cancellation/Drop, leaking a native permit,
// publishing tools/history before EOF, or fabricating completion after errors.
#[tokio::test]
async fn projected_stream_cancellation_drop_and_truncation_never_publish_terminal_history() {
    for cancel in [false, true] {
        let mut reply = Reply::sse(&[native_reply("STOP")]);
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
        let provider = GeminiProvider::new(client, vec![profile()], 8).unwrap();
        let cancellation = CancellationToken::new();
        let mut stream = provider
            .stream_response(
                canonical(
                    ResponsesDialect::Classic,
                    vec![json!({"role":"user","content":"q"})],
                    &json!([]),
                    true,
                ),
                RequestContext {
                    cancellation: cancellation.clone(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        fixture.request().await;
        let mut progress = Vec::new();
        while let Some(Ok(event)) = stream.events.next().await {
            if let ProviderStreamEvent::Model(event) = event {
                let visible = event.response.text_delta().is_some();
                progress.push(event);
                if visible {
                    break;
                }
            }
        }
        assert!(!progress.is_empty());
        assert!(progress.iter().all(|e| e.response.terminal().is_none()
            && !e.frame.data.contains("encrypted_content")
            && e.response.kind() != "response.output_item.done"));
        assert_eq!(
            provider.list_models().await.err().unwrap().code,
            "provider_busy"
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        if cancel {
            cancellation.cancel();
            assert_eq!(
                tokio::time::timeout(WAIT, stream.events.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .err()
                    .unwrap()
                    .code,
                "provider_cancelled"
            );
            assert!(stream.events.next().await.is_none());
        }
        drop(stream);
        fixture.disconnected().await;
        assert!(provider.list_models().await.unwrap().is_empty());
        fixture.request().await;
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
    for body in [
        format!(
            "data: {}\n\n",
            json!({"candidates":[{"content":{"parts":[{"text":"partial"}]}}]})
        ),
        format!("data: {}\n\ndata: {{\n\n", native_reply("STOP")),
    ] {
        let mut reply = Reply::sse(&[]);
        reply.body = body;
        let mut fixture = Fixture::start(vec![reply]).await;
        let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
        let provider = GeminiProvider::new(client, vec![profile()], 8).unwrap();
        let mut stream = provider
            .stream_response(
                canonical(
                    ResponsesDialect::Classic,
                    vec![json!({"role":"user","content":"q"})],
                    &json!([]),
                    true,
                ),
                RequestContext::default(),
            )
            .await
            .unwrap();
        let mut error = None;
        while let Some(event) = stream.events.next().await {
            match event {
                Ok(ProviderStreamEvent::Model(event)) => assert!(
                    event.response.terminal().is_none()
                        && !event.frame.data.contains("encrypted_content")
                        && event.response.kind() != "response.output_item.done"
                ),
                Err(e) => error = Some(e),
                _ => {}
            }
        }
        assert!(error.is_some());
        fixture.request().await;
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
}

// Catches bypassing the real Gateway token/dialect contract, sharing the
// listener token with Google, losing SSE lifecycle, or forwarding context.
#[tokio::test]
async fn gateway_with_gemini_preserves_auth_dialect_headers_and_sse_history() {
    use caidex_credentials::Redactor;
    use caidex_model_core::{ResponsesStream, StreamState};
    use caidex_model_gateway::start_with_provider;
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let mut fixture = Fixture::start(vec![
            Reply::json(native_reply("STOP")),
            Reply::sse(&[native_reply("STOP")]),
        ])
        .await;
        let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
        let provider = Arc::new(GeminiProvider::new(client, vec![profile()], 8).unwrap());
        let gateway = start_with_provider(provider, &Redactor::default(), Limits::default())
            .await
            .unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let make = |stream, authorized: bool, context: bool| {
            let request = canonical(
                dialect,
                vec![json!({"role":"user","content":"q"})],
                &json!([]),
                stream,
            );
            let mut post = http
                .post(format!("http://{}/v1/responses", gateway.address()))
                .header("content-type", "application/json")
                .body(request.wire().to_string());
            if authorized {
                post = post.bearer_auth(gateway.token().expose());
            }
            if context {
                post = post.header("session_id", "private");
            }
            if dialect == ResponsesDialect::Lite {
                post = post.header("x-openai-internal-codex-responses-lite", "true");
            }
            post
        };
        assert_eq!(
            make(false, false, false).send().await.unwrap().status(),
            401
        );
        let rejected = make(false, true, true).send().await.unwrap();
        assert_eq!(rejected.status(), 400);
        let error: Value = serde_json::from_slice(&rejected.bytes().await.unwrap()).unwrap();
        assert_eq!(error["error"]["code"], "unsupported_native_context_header");
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        for stream in [false, true] {
            let reply = make(stream, true, false).send().await.unwrap();
            assert_eq!(reply.status(), 200);
            let bytes = reply.bytes().await.unwrap();
            let output = if stream {
                let mut parser = ResponsesStream::new(LIMIT).unwrap();
                let events = parser.push(&bytes).unwrap();
                assert_eq!(parser.finish().unwrap(), StreamState::Completed);
                assert_eq!(
                    events
                        .iter()
                        .filter_map(|e| e.response.text_delta())
                        .collect::<String>(),
                    "回复"
                );
                events.last().unwrap().response.wire()["response"]["output"]
                    .as_array()
                    .unwrap()
                    .clone()
            } else {
                let wire: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(wire["status"], "completed");
                wire["output"].as_array().unwrap().clone()
            };
            let upstream = fixture.request().await;
            assert!(upstream.starts_with(if stream {
                "POST /proxy/v1beta/models/fixture-provider:streamGenerateContent?alt=sse "
            } else {
                "POST /proxy/v1beta/models/fixture-provider:generateContent "
            }));
            assert!(!upstream.contains(gateway.token().expose()));
            assert!(!upstream.to_ascii_lowercase().contains("authorization:"));
            let history = NativeHistory::from_responses_output(
                &output,
                MODEL,
                &request_body(&upstream),
                LIMIT,
            )
            .unwrap();
            assert_eq!(history.native_response(), &native_reply("STOP"));
        }
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        gateway.shutdown().await.unwrap();
    }
}

// Catches opt-in context being sent to Google, default/turn-state acceptance,
// or context handling bypassing cancellation/deadline before credentials.
#[tokio::test]
async fn explicit_runtime_context_stays_local_and_rejects_native_routing_state() {
    let mut fixture = Fixture::start(vec![
        Reply::json(json!({"models":[]})),
        Reply::json(native_reply("STOP")),
        Reply::sse(&[native_reply("STOP")]),
    ])
    .await;
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
        .with_base_url(&fixture.base)
        .unwrap()
        .with_local_runtime_context();
    let client = GeminiClient::new(config, broker, Limits::default()).unwrap();
    let context = || {
        let mut context = RequestContext::default();
        for (key, value) in [
            ("session_id", "local-session"),
            ("x-client-request-id", "local-request"),
            ("x-codex-turn-metadata", "local-turn"),
        ] {
            context
                .headers
                .insert(key, value.into(), REQUEST_HEADERS)
                .unwrap();
        }
        context
    };
    assert!(
        client
            .discover_models(8, context())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        client
            .generate_content(MODEL, native_input(), context())
            .await
            .unwrap()
            .wire(),
        &native_reply("STOP")
    );
    let mut stream = client
        .stream_content(MODEL, native_input(), context())
        .await
        .unwrap();
    let mut completed = false;
    while let Some(event) = stream.next().await {
        completed |= matches!(event.unwrap(), NativeStreamEvent::Completed(_));
    }
    assert!(completed);
    for _ in 0..3 {
        let request = fixture.request().await.to_ascii_lowercase();
        assert!(request.contains("x-goog-api-key: caidex_synthetic_google_key"));
        for key in [
            "session_id:",
            "x-client-request-id:",
            "x-codex-turn-metadata:",
            "x-codex-turn-state:",
            "authorization:",
        ] {
            assert!(!request.contains(key));
        }
        for value in ["local-session", "local-request", "local-turn"] {
            assert!(!request.contains(value));
        }
    }
    for state in ["x-codex-turn-state", "x-request-id"] {
        let mut context = context();
        context
            .headers
            .insert(
                state,
                "private-state".into(),
                caidex_model_core::RESPONSE_HEADERS,
            )
            .unwrap();
        assert_eq!(
            client
                .generate_content(MODEL, native_input(), context)
                .await
                .err()
                .unwrap()
                .code,
            "unsupported_native_context_header"
        );
    }
    let cancelled = context();
    cancelled.cancellation.cancel();
    assert_eq!(
        client
            .generate_content(MODEL, native_input(), cancelled)
            .await
            .err()
            .unwrap()
            .code,
        "provider_cancelled"
    );
    let mut expired = context();
    expired.deadline = Some(std::time::Instant::now() - Duration::from_secs(1));
    assert_eq!(
        client
            .generate_content(MODEL, native_input(), expired)
            .await
            .err()
            .unwrap()
            .code,
        "provider_timeout"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 3);
}

// Catches treating parallel=false as a prompt, delivering multiple executable
// calls/opaque history before cardinality validation, or accepting calls for none.
#[tokio::test]
async fn explicit_single_call_policy_validates_json_and_sse_before_tool_delivery() {
    let declarations = json!([{ "type":"function","name":"echo","parameters":{"type":"object"}}]);
    let tools = ToolMap::new(declarations.as_array().unwrap(), 8).unwrap();
    for stream in [false, true] {
        for (count, choice, reason, want) in [
            (1, "auto", "STOP", "completed"),
            (2, "auto", "STOP", "google_tool_call_limit_exceeded"),
            (1, "none", "STOP", "google_tool_call_limit_exceeded"),
            (2, "auto", "MAX_TOKENS", "incomplete"),
        ] {
            let mut native = native_reply(reason);
            native["candidates"][0]["content"]["parts"]=Value::Array((0..count).map(|i|json!({"functionCall":{"name":tools.native_tools()[0]["name"],"id":format!("call-{i}"),"args":{"n":i}},"thoughtSignature":"PRIVATE_SIGNED_CALL"})).collect());
            let mut fixture = Fixture::start(vec![if stream {
                Reply::sse(std::slice::from_ref(&native))
            } else {
                Reply::json(native.clone())
            }])
            .await;
            let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
            let mut p = profile();
            p.enforce_single_tool_call = true;
            let provider = GeminiProvider::new(client, vec![p], 8).unwrap();
            let mut wire = canonical(
                ResponsesDialect::Classic,
                vec![json!({"role":"user","content":"q"})],
                &declarations,
                stream,
            )
            .wire()
            .clone();
            wire["parallel_tool_calls"] = false.into();
            wire["tool_choice"] = choice.into();
            let request = CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap();
            if stream {
                let mut stream = provider
                    .stream_response(request, RequestContext::default())
                    .await
                    .unwrap();
                let mut events = Vec::new();
                let mut error = None;
                while let Some(event) = stream.events.next().await {
                    match event {
                        Ok(ProviderStreamEvent::Model(event)) => events.push(event),
                        Err(e) => error = Some(e),
                        _ => {}
                    }
                }
                if want.starts_with("google_") {
                    let error = error.unwrap();
                    assert_eq!(error.http_status, 502);
                    assert_eq!(error.code, want);
                    assert!(!format!("{error:?}").contains("PRIVATE_SIGNED_CALL"));
                    assert!(events.iter().all(|e| e.response.terminal().is_none()
                        && e.response.kind() != "response.output_item.done"
                        && !e.frame.data.contains("encrypted_content")
                        && !e.frame.data.contains("function_call")));
                } else {
                    assert!(error.is_none());
                    let final_response = &events.last().unwrap().response.wire()["response"];
                    assert_eq!(final_response["status"], want);
                    assert_eq!(
                        final_response["output"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter(|i| i["type"] == "function_call")
                            .count(),
                        usize::from(want == "completed")
                    );
                    assert_eq!(
                        events
                            .iter()
                            .filter(|e| e.response.kind() == "response.output_item.done"
                                && e.response.wire()["item"]["type"] == "function_call")
                            .count(),
                        usize::from(want == "completed")
                    );
                    assert_eq!(
                        NativeHistory::from_responses_output(
                            final_response["output"].as_array().unwrap(),
                            MODEL,
                            &request_body(&fixture.request().await),
                            LIMIT
                        )
                        .unwrap()
                        .native_response(),
                        &native
                    );
                }
            } else {
                let response = provider
                    .create_response(request, RequestContext::default())
                    .await;
                if want.starts_with("google_") {
                    let error = response.err().unwrap();
                    assert_eq!(error.http_status, 502);
                    assert_eq!(error.code, want);
                    assert!(!format!("{error:?}").contains("PRIVATE_SIGNED_CALL"));
                } else {
                    let response = response.unwrap();
                    assert_eq!(response.response.wire()["status"], want);
                    assert_eq!(
                        response
                            .response
                            .output()
                            .iter()
                            .filter(|i| i["type"] == "function_call")
                            .count(),
                        usize::from(want == "completed")
                    );
                    assert_eq!(
                        NativeHistory::from_responses_output(
                            response.response.output(),
                            MODEL,
                            &request_body(&fixture.request().await),
                            LIMIT
                        )
                        .unwrap()
                        .native_response(),
                        &native
                    );
                }
            }
            if want.starts_with("google_") {
                fixture.request().await;
            }
            assert_eq!(reads.load(Ordering::SeqCst), 1);
        }
    }
}
