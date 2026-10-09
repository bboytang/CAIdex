use super::*;
use caidex_model_core::CanonicalResponse;

fn provider(fixture: &Fixture, broker: Arc<Broker<Store>>) -> QwenProvider<Store> {
    let mut model = metadata("fixture", "native-fixture");
    model.dialects = vec![ResponsesDialect::Classic, ResponsesDialect::Lite];
    QwenProvider::with_lite_options(
        QwenConfig::new(&fixture.base, reference()).unwrap(),
        vec![model],
        broker,
        limits(),
        Default::default(),
    )
    .unwrap()
    .with_runtime_context()
}
fn source(streaming: bool) -> Value {
    let mut wire = custom_source(streaming);
    let tools = wire.as_object_mut().unwrap().remove("tools").unwrap();
    wire["input"].as_array_mut().unwrap().insert(
        0,
        json!({"type":"additional_tools","id":"at_stable","role":"developer","tools":tools}),
    );
    wire["input"][1] = json!({"type":"message","id":"msg_stable","role":"developer","content":[{"type":"input_text","text":"Keep the rules"}]});
    wire["parallel_tool_calls"] = false.into();
    wire["reasoning"] = json!({"summary":"auto","context":"all_turns"});
    wire["include"] = json!(["reasoning.encrypted_content"]);
    wire
}
fn lite(wire: Value) -> CanonicalRequest {
    CanonicalRequest::new(wire, ResponsesDialect::Lite).unwrap()
}
fn single_native() -> Value {
    let mut response = custom_native();
    response["output"].as_array_mut().unwrap().pop();
    response
}
fn reply(response: &Value, streaming: bool) -> Reply {
    if streaming {
        {
            let mut events = tool_events(response);
            events[0]["response"]["id"] = response["id"].clone();
            Reply::stream(events_body(&events))
        }
    } else {
        Reply::json(response.clone())
    }
}
fn stored(output: &[Value]) -> Value {
    serde_json::from_str(
        output[0]["encrypted_content"]
            .as_str()
            .unwrap()
            .strip_prefix("caidex.qwen.native-history.v4:")
            .unwrap(),
    )
    .unwrap()
}
async fn deliver(provider: &QwenProvider<Store>, wire: Value) -> CanonicalResponse {
    if wire["stream"] != true {
        return provider
            .create_response(lite(wire), runtime_context())
            .await
            .unwrap()
            .response;
    }
    let mut events = provider
        .stream_response(lite(wire), runtime_context())
        .await
        .unwrap()
        .events;
    let mut terminal = None;
    let mut carrier = false;
    while let Some(event) = events.next().await {
        if let ProviderStreamEvent::Model(event) = event.unwrap() {
            let wire = event.response.wire();
            assert!(
                !event.response.kind().contains("call_arguments")
                    && !event.response.kind().contains("tool_call_input")
            );
            if wire["item"]["encrypted_content"].is_string() {
                carrier = true;
            }
            if matches!(
                wire["item"]["type"].as_str(),
                Some("function_call" | "custom_tool_call")
            ) {
                assert!(carrier);
            }
            if event.response.kind() == "response.completed" {
                terminal = Some(wire["response"].clone());
            }
        }
    }
    CanonicalResponse::new(terminal.unwrap()).unwrap()
}
async fn refused(provider: &QwenProvider<Store>, wire: Value) -> caidex_model_core::ProviderError {
    if wire["stream"] != true {
        return provider
            .create_response(lite(wire), runtime_context())
            .await
            .err()
            .expect("must refuse");
    }
    let mut events = match provider
        .stream_response(lite(wire), runtime_context())
        .await
    {
        Ok(response) => response.events,
        Err(error) => return error,
    };
    while let Some(event) = events.next().await {
        match event {
            Err(error) => return error,
            Ok(ProviderStreamEvent::Model(event)) => {
                assert!(!matches!(
                    event.response.wire()["item"]["type"].as_str(),
                    Some("function_call" | "custom_tool_call")
                ));
                assert!(
                    event.response.wire()["item"]
                        .get("encrypted_content")
                        .is_none()
                );
                assert!(
                    !event.response.kind().contains("call_arguments")
                        && !event.response.kind().contains("tool_call_input")
                );
                assert!(event.response.terminal().is_none());
            }
            _ => (),
        }
    }
    panic!("bad response did not fail")
}

#[tokio::test]
async fn json_sse_three_rounds_bind_stable_prefix_and_replay_exact_native_wire() {
    for streaming in [false, true] {
        let first = single_native();
        let mut second = tool_native();
        second["id"] = "fixture-two".into();
        second["output"].as_array_mut().unwrap().remove(1);
        second["output"][2]["call_id"] = "call_two".into();
        let mut fixture = Fixture::start(vec![
            reply(&first, streaming),
            reply(&second, streaming),
            reply(&native(), streaming),
            Reply::json(catalog()),
        ])
        .await;
        let (broker, reads) = fixture_broker(Some(KEY));
        let enabled = provider(&fixture, broker)
            .with_reasoning_effort_mapping("fixture".into(), "high".into(), "xhigh".into())
            .unwrap()
            .with_reasoning_effort_mapping("fixture".into(), "xhigh".into(), "none".into())
            .unwrap();
        assert_eq!(
            enabled.metadata("fixture").unwrap().dialects,
            [ResponsesDialect::Classic, ResponsesDialect::Lite]
        );
        let mut original = source(streaming);
        original["reasoning"]["effort"] = "high".into();
        let response = deliver(&enabled, original.clone()).await;
        let captured = fixture.captured().await;
        assert!(
            captured
                .header("x-openai-internal-codex-responses-lite")
                .is_none()
        );
        assert!(!captured.headers.contains("LOCAL_"));
        let sent = captured.body.unwrap();
        assert_eq!(sent["model"], "native-fixture");
        assert_eq!(sent["input"][0], original["input"][1]);
        for key in ["parallel_tool_calls", "include", "instructions"] {
            assert!(sent.get(key).is_none());
        }
        assert_eq!(sent["store"], false);
        assert_eq!(sent["reasoning"], json!({"effort":"xhigh"}));
        assert_eq!(sent["tools"][0]["type"], "function");
        assert_eq!(response.output()[1]["type"], "custom_tool_call");
        assert_eq!(response.output()[1]["namespace"], "alpha");
        assert_eq!(
            response.output()[1]["input"],
            "*** Begin Patch\n中文🙂\n*** End Patch"
        );
        let history = stored(response.output());
        assert_eq!(history["version"], 4);
        assert_eq!(history["tool_mapping"]["lite_single_tool_call"], true);
        assert_eq!(history["tool_mapping"]["additional_tools_id"], "at_stable");
        assert_eq!(history["request"], sent);
        assert_eq!(history["response"], first);
        assert!(!history.to_string().contains(KEY));
        if streaming {
            assert_eq!(history["chunks"], json!(tool_events(&first)));
        }
        let output: Vec<Value> =
            serde_json::from_str(&serde_json::to_string(response.output()).unwrap()).unwrap();
        let mut next = custom_next(&original, &output);
        let response = deliver(&enabled, next.clone()).await;
        let sent = fixture.captured().await.body.unwrap();
        assert_eq!(sent["input"][3], first["output"][1]);
        assert_eq!(sent["input"][4]["type"], "function_call_output");
        assert_eq!(sent["input"][4]["output"], "Done");
        assert_eq!(response.output()[2]["type"], "function_call");
        assert_eq!(response.output()[2]["namespace"], "beta");
        assert_eq!(stored(response.output())["response"], second);
        assert_eq!(
            stored(response.output())["request"]["reasoning"],
            json!({"effort":"xhigh"})
        );
        next = custom_next(&next, response.output());
        deliver(&enabled, next).await;
        let sent = fixture.captured().await.body.unwrap();
        assert_eq!(sent["input"][3], first["output"][1]);
        assert_eq!(sent["input"][9], second["output"][2]);
        assert!(!sent.to_string().contains("caidex.qwen.native-history"));
        assert_eq!(
            enabled.list_models().await.unwrap()[0].dialects,
            [ResponsesDialect::Classic, ResponsesDialect::Lite]
        );
        fixture.captured().await;
        assert_eq!(reads.load(Ordering::SeqCst), 4);
    }
}

#[tokio::test]
async fn invalid_prefix_tools_controls_and_default_route_refuse_before_credentials() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let enabled = provider(&fixture, broker.clone());
    let classic = fixture
        .provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_custom_tool_mapping()
        .with_runtime_context();
    assert_eq!(
        refused(&classic, source(false)).await.code,
        "unsupported_dialect"
    );
    for corrupt in 0..18 {
        let mut wire = source(false);
        match corrupt {
            0 => wire["parallel_tool_calls"] = Value::Null,
            1 => wire["parallel_tool_calls"] = "false".into(),
            2 => wire["input"][0]["role"] = "user".into(),
            3 => wire["input"][0]["id"] = "".into(),
            4 => wire["input"][0]["id"] = "bad\n".into(),
            5 => wire["input"][0]["future"] = true.into(),
            6 => wire["input"][0]["tools"] = json!({}),
            7 => wire["input"].as_array_mut().unwrap().swap(0, 1),
            8 => {
                let extra = wire["input"][0].clone();
                wire["input"].as_array_mut().unwrap().push(extra);
            }
            9 => wire["input"][1]["id"] = "bad\n".into(),
            10 => wire["input"][1]["status"] = "completed".into(),
            11 => wire["input"][1]["role"] = "user".into(),
            12 => wire["input"][0]["tools"][0]["tools"][0]["defer_loading"] = true.into(),
            13 => {
                wire["input"][0]["tools"][0]["tools"][0]["format"] =
                    json!({"type":"grammar","syntax":"bad","definition":"x"})
            }
            14 => wire["input"][0]["tools"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"web_search"})),
            15 => wire["reasoning"]["context"] = "current_turn".into(),
            16 => wire["include"] = json!(["unknown"]),
            _ => {
                wire["tool_choice"] = json!({"type":"function","namespace":"alpha","name":"lookup"})
            }
        }
        for streaming in [false, true] {
            wire["stream"] = streaming.into();
            assert_eq!(
                refused(&enabled, wire.clone()).await.http_status,
                400,
                "case {corrupt}"
            );
        }
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn local_single_call_rejects_multiple_native_calls_before_json_or_sse_delivery() {
    for streaming in [false, true] {
        let mut fixture = Fixture::start(vec![
            reply(&custom_native(), streaming),
            reply(&single_native(), streaming),
        ])
        .await;
        let (broker, _) = fixture_broker(Some(KEY));
        let enabled = provider(&fixture, broker);
        assert_eq!(
            refused(&enabled, source(streaming)).await.code,
            "qwen_invalid_native_tools"
        );
        fixture.captured().await;
        fixture.disconnected().await;
        deliver(&enabled, source(streaming)).await;
        fixture.captured().await;
    }
}

#[tokio::test]
async fn optional_parallel_policy_still_binds_lite_and_default_classic_remains_unchanged() {
    for parallel in [None, Some(true)] {
        let mut fixture = Fixture::start(vec![
            Reply::json(custom_native()),
            Reply::json(custom_native()),
            Reply::json(native()),
        ])
        .await;
        let (broker, reads) = fixture_broker(Some(KEY));
        let enabled = provider(&fixture, broker);
        let mut wire = source(false);
        if let Some(value) = parallel {
            wire["parallel_tool_calls"] = value.into();
        } else {
            wire.as_object_mut().unwrap().remove("parallel_tool_calls");
        }
        let response = deliver(&enabled, wire.clone()).await;
        fixture.captured().await;
        assert_eq!(
            stored(response.output())["tool_mapping"]["lite_single_tool_call"],
            false
        );
        assert_eq!(
            response
                .output()
                .iter()
                .filter(|i| matches!(
                    i["type"].as_str(),
                    Some("function_call" | "custom_tool_call")
                ))
                .count(),
            2
        );
        let mut next = custom_next(&wire, response.output());
        next["parallel_tool_calls"] = false.into();
        assert_eq!(refused(&enabled, next).await.http_status, 400);
        enabled
            .create_response(request(custom_source(false)), runtime_context())
            .await
            .unwrap();
        fixture.captured().await;
        let mut empty = source(false);
        empty["input"][0]["tools"] = json!([]);
        deliver(&enabled, empty).await;
        fixture.captured().await;
        assert_eq!(reads.load(Ordering::SeqCst), 3);
    }
}

#[tokio::test]
async fn v4_history_rejects_policy_version_scope_and_stable_identity_tampering_before_keys() {
    let mut fixture = Fixture::start(vec![Reply::json(single_native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let enabled = provider(&fixture, broker.clone());
    let original = source(false);
    let response = deliver(&enabled, original.clone()).await;
    fixture.captured().await;
    let next = custom_next(&original, response.output());
    for corrupt in 0..13 {
        let mut wire = next.clone();
        let mut history = stored(response.output());
        match corrupt {
            0 => wire["input"][0]["id"] = "at_other".into(),
            1 => wire["input"][1]["id"] = "msg_other".into(),
            2 => wire["input"][1]["content"][0]["text"] = "changed rules".into(),
            3 => {
                wire["input"][0]["tools"][0]["tools"][0]["format"]["definition"] = "changed".into()
            }
            4 => history["version"] = 3.into(),
            5 => history["scope"]["profile"] = "other".into(),
            6 => history["tool_mapping"]
                .as_object_mut()
                .unwrap()
                .remove("lite_single_tool_call")
                .map(|_| ())
                .unwrap(),
            7 => history["tool_mapping"]["lite_single_tool_call"] = "true".into(),
            8 => history["tool_mapping"]["custom_as_function"] = false.into(),
            9 => history["tool_mapping"]["additional_tools_id"] = "bad\n".into(),
            10 => history["request"]["reasoning"] = json!({"summary":"auto"}),
            11 => history["tool_mapping"]["parallel_tool_calls"] = true.into(),
            _ => history["response"]["output"]
                .as_array_mut()
                .unwrap()
                .push(custom_native()["output"][3].clone()),
        }
        if corrupt >= 4 {
            wire["input"][3]["encrypted_content"] =
                format!("caidex.qwen.native-history.v4:{history}").into();
        }
        assert_eq!(
            refused(&enabled, wire).await.http_status,
            400,
            "case {corrupt}"
        );
    }
    let classic = fixture
        .provider(
            broker,
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_custom_tool_mapping()
        .with_runtime_context();
    let mut classic_next = next.clone();
    let declarations = classic_next["input"].as_array_mut().unwrap().remove(0);
    classic_next["tools"] = declarations["tools"].clone();
    classic_next["parallel_tool_calls"] = true.into();
    assert_eq!(
        classic
            .create_response(request(classic_next), runtime_context())
            .await
            .err()
            .unwrap()
            .http_status,
        400
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn explicit_routes_capabilities_and_source_compiled_budgets_keep_pre_auth_guards() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let config = || QwenConfig::new(&fixture.base, reference()).unwrap();
    let mut model = metadata("fixture", "native-fixture");
    model.dialects = vec![ResponsesDialect::Lite];
    assert!(QwenProvider::new(config(), vec![model.clone()], broker.clone(), limits()).is_err());
    let lite_only = QwenProvider::with_lite_options(
        config(),
        vec![model.clone()],
        broker.clone(),
        limits(),
        Default::default(),
    )
    .unwrap()
    .with_runtime_context();
    assert_eq!(
        lite_only
            .create_response(request(basic(false)), RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "unsupported_dialect"
    );
    let mut duplicate = model.clone();
    duplicate.dialects.push(ResponsesDialect::Lite);
    assert!(
        QwenProvider::with_lite_options(
            config(),
            vec![duplicate],
            broker.clone(),
            limits(),
            Default::default()
        )
        .is_err()
    );
    for capability in ["text", "tools", "reasoning"] {
        let mut model = model.clone();
        match capability {
            "text" => model.capabilities.text = CapabilitySupport::Unsupported,
            "tools" => model.capabilities.native_tools = CapabilitySupport::Unsupported,
            _ => model.capabilities.reasoning = CapabilitySupport::Unsupported,
        }
        let enabled = QwenProvider::with_lite_options(
            config(),
            vec![model],
            broker.clone(),
            limits(),
            Default::default(),
        )
        .unwrap()
        .with_runtime_context();
        assert_eq!(refused(&enabled, source(false)).await.http_status, 400);
    }
    for compiled in [false, true] {
        let mut wire = source(false);
        wire["text"] = json!({"verbosity":"low"});
        let mut small = limits();
        small.request_bytes = if compiled {
            wire.to_string().len() + 1
        } else {
            200
        };
        let bounded = QwenProvider::with_lite_options(
            config(),
            vec![model.clone()],
            broker.clone(),
            small,
            Default::default(),
        )
        .unwrap()
        .with_runtime_context();
        let bounded = bounded
            .with_verbosity_instruction("low".into(), "guidance".repeat(300))
            .unwrap();
        assert_eq!(refused(&bounded, wire).await.http_status, 413);
    }
    for cancelled in [false, true] {
        let mut context = runtime_context();
        if cancelled {
            context.cancellation.cancel();
        } else {
            context.deadline = Some(std::time::Instant::now());
        }
        assert_eq!(
            lite_only
                .create_response(lite(source(false)), context)
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
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn partial_stream_cancel_drop_and_deadline_release_native_socket_and_slot() {
    let chunks = tool_events(&single_native());
    let mut stalled = Reply::stream(events_body(&chunks[..chunks.len() - 1]));
    stalled.stall = 2;
    let mut fixture = Fixture::start(
        (0..3)
            .flat_map(|_| [stalled.clone(), Reply::json(native())])
            .collect(),
    )
    .await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let enabled = provider(&fixture, broker);
    for action in ["cancel", "deadline", "drop"] {
        let cancellation = CancellationToken::new();
        let context = RequestContext {
            cancellation: cancellation.clone(),
            deadline: (action == "deadline")
                .then(|| std::time::Instant::now() + Duration::from_millis(300)),
            headers: ContextHeaders::default(),
        };
        let mut stream = enabled
            .stream_response(lite(source(true)), context)
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
                if event.response.kind() == "response.output_text.done" {
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
            assert_eq!(
                stream.events.next().await.unwrap().err().unwrap().code,
                if action == "cancel" {
                    "provider_cancelled"
                } else {
                    "provider_timeout"
                }
            );
            assert!(stream.events.next().await.is_none());
        }
        fixture.disconnected().await;
        deliver(&enabled, source(false)).await;
        fixture.captured().await;
    }
    assert_eq!(reads.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn gateway_lite_two_turns_keeps_tokens_attribution_and_native_history_local() {
    let first = single_native();
    let mut fixture = Fixture::start(vec![reply(&first, true), Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let gateway = caidex_model_gateway::start_with_provider(
        Arc::new(provider(&fixture, broker.clone())),
        broker.redactor(),
        limits(),
    )
    .await
    .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let address = format!("http://{}/v1/responses", gateway.address());
    let original = source(true);
    let denied = client
        .post(&address)
        .header("content-type", "application/json")
        .header("x-openai-internal-codex-responses-lite", "true")
        .body(original.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let mut outgoing = client
        .post(&address)
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .header("x-openai-internal-codex-responses-lite", "true")
        .body(original.to_string());
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
    let events = parser.push(raw.as_bytes()).unwrap();
    assert_eq!(
        parser.finish().unwrap(),
        caidex_model_core::StreamState::Completed
    );
    let output = events.last().unwrap().response.wire()["response"]["output"]
        .as_array()
        .unwrap();
    let sent = fixture.captured().await;
    assert_eq!(
        sent.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert!(
        sent.header("x-openai-internal-codex-responses-lite")
            .is_none()
    );
    assert!(!sent.headers.contains("LOCAL_") && !sent.headers.contains(gateway.token().expose()));
    assert_eq!(stored(output)["request"], sent.body.unwrap());
    let mut next = custom_next(&original, output);
    next["stream"] = false.into();
    let response = client
        .post(&address)
        .bearer_auth(gateway.token().expose())
        .header("content-type", "application/json")
        .header("x-openai-internal-codex-responses-lite", "true")
        .body(next.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let response: Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
    assert_eq!(
        stored(response["output"].as_array().unwrap())["response"],
        native()
    );
    let replayed = fixture.captured().await.body.unwrap();
    assert_eq!(replayed["input"][0], original["input"][1]);
    assert_eq!(replayed["input"][3], first["output"][1]);
    assert_eq!(replayed["input"][4]["type"], "function_call_output");
    assert!(!replayed.to_string().contains("caidex.qwen.native-history"));
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    gateway.shutdown().await.unwrap();
}
