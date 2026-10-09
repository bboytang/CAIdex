use super::*;

fn provider(fixture: &Fixture, broker: Arc<Broker<Store>>) -> DeepSeekProvider<Store> {
    let mut model = metadata("fixture", "native-fixture");
    model.dialects = vec![ResponsesDialect::Classic, ResponsesDialect::Lite];
    DeepSeekProvider::with_lite_options(
        DeepSeekConfig::new(reference())
            .unwrap()
            .with_base_url(&fixture.base)
            .unwrap(),
        vec![model],
        broker,
        limits(),
        Default::default(),
    )
    .unwrap()
    .with_runtime_context()
}
fn source(stream: bool) -> Value {
    json!({"model":"fixture","stream":stream,"parallel_tool_calls":false,"input":[
        {"type":"additional_tools","id":"at_fixture","role":"developer","tools":[
            {"type":"namespace","name":"functions","description":"Scoped tools","tools":[
                {"type":"custom","name":"exec","format":{"type":"text"}},
                {"type":"function","name":"read","parameters":{"type":"object"}}]}]},
        {"role":"developer","content":"Original priority"},
        {"role":"user","content":"Run the exact input"}],
        "include":["reasoning.encrypted_content"],"reasoning":{"summary":"auto","context":"all_turns"}})
}
fn lite(wire: Value) -> CanonicalRequest {
    CanonicalRequest::new(wire, ResponsesDialect::Lite).unwrap()
}
fn custom_native() -> Value {
    let mut wire = history_text();
    wire["output"].as_array_mut().unwrap().push(json!({"type":"function_call","id":"fc_custom","status":"completed","call_id":"call-one","name":"caidex_ns_0","arguments":" { \"input\" : \"line 1\\n你好🙂\\n\" } "}));
    wire
}
fn reply(wire: Value, stream: bool) -> Reply {
    if stream {
        Reply::stream(native_chunks(&wire).into_iter().map(event).collect())
    } else {
        Reply::json(wire)
    }
}
async fn deliver(
    provider: &DeepSeekProvider<Store>,
    wire: Value,
) -> caidex_model_core::CanonicalResponse {
    if !wire["stream"].as_bool().unwrap() {
        return provider
            .create_response(lite(wire), local_context())
            .await
            .unwrap()
            .response;
    }
    let mut events = provider
        .stream_response(lite(wire), local_context())
        .await
        .unwrap()
        .events;
    let mut response = None;
    while let Some(event) = events.next().await {
        if let ProviderStreamEvent::Model(event) = event.unwrap()
            && event.response.terminal().is_some()
        {
            response = Some(
                caidex_model_core::CanonicalResponse::new(
                    event.response.wire()["response"].clone(),
                )
                .unwrap(),
            );
        }
    }
    response.unwrap()
}
fn stored(item: &Value) -> Value {
    serde_json::from_str(
        item["encrypted_content"]
            .as_str()
            .unwrap()
            .strip_prefix("caidex.deepseek.native-history.v3:")
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn lite_json_sse_maps_custom_input_and_replays_exact_native_three_turns() {
    for stream in [false, true] {
        let first = custom_native();
        let mut second = tool_native("caidex_ns_1");
        second["id"] = "fixture-two".into();
        second["output"][2]["call_id"] = "call-two".into();
        let mut fixture = Fixture::start(vec![
            reply(first.clone(), stream),
            reply(second.clone(), stream),
            reply(history_text(), stream),
            Reply::json(catalog()),
        ])
        .await;
        let (broker, reads) = fixture_broker(Some(KEY));
        let enabled = provider(&fixture, broker);
        assert_eq!(
            enabled.metadata("fixture").unwrap().dialects,
            [ResponsesDialect::Classic, ResponsesDialect::Lite]
        );
        let original = source(stream);
        let response = deliver(&enabled, original.clone()).await;
        let sent = fixture.captured().await;
        assert!(
            sent.header("x-openai-internal-codex-responses-lite")
                .is_none()
        );
        let sent = sent.body.unwrap();
        assert_eq!(sent["model"], "native-fixture");
        assert_eq!(sent["input"][0]["role"], "system");
        assert_eq!(sent["input"][0]["content"], "Original priority");
        assert_eq!(sent["tools"][0]["type"], "function");
        assert_eq!(
            sent["tools"][0]["parameters"]["properties"]["input"]["type"],
            "string"
        );
        assert!(sent.get("parallel_tool_calls").is_none());
        assert!(sent.get("reasoning").is_none());
        assert_eq!(response.output()[2]["type"], "custom_tool_call");
        assert_eq!(response.output()[2]["namespace"], "functions");
        assert_eq!(response.output()[2]["name"], "exec");
        assert_eq!(response.output()[2]["input"], "line 1\n你好🙂\n");
        let carrier = stored(&response.output()[0]);
        assert_eq!(carrier["version"], 3);
        assert_eq!(carrier["request"], sent);
        assert_eq!(carrier["response"], first);
        assert_eq!(carrier["tool_mapping"]["lite_single_tool_call"], true);
        assert_eq!(
            carrier["tool_mapping"]["tools"],
            original["input"][0]["tools"]
        );
        assert!(!carrier.to_string().contains(KEY));
        let mut next = original.clone();
        let input = next["input"].as_array_mut().unwrap();
        // Exercise actual serialization before native restoration.
        input.extend(
            serde_json::from_str::<Vec<Value>>(&serde_json::to_string(response.output()).unwrap())
                .unwrap(),
        );
        input.push(json!({"type":"custom_tool_call_output","call_id":"call-one","output":[{"type":"input_text","text":"actual exec result\n🙂"}]}));
        input.push(json!({"role":"user","content":"Continue"}));
        let response = deliver(&enabled, next.clone()).await;
        let sent = fixture.captured().await.body.unwrap();
        assert_eq!(
            &sent["input"].as_array().unwrap()[2..5],
            first["output"].as_array().unwrap()
        );
        assert_eq!(sent["input"][5]["type"], "function_call_output");
        assert_eq!(
            sent["input"][5]["output"][0]["text"],
            "actual exec result\n🙂"
        );
        assert_eq!(response.output()[2]["type"], "function_call");
        assert_eq!(response.output()[2]["name"], "read");
        assert_eq!(stored(&response.output()[0])["response"], second);
        let input = next["input"].as_array_mut().unwrap();
        input.extend(response.output().iter().cloned());
        input.push(
            json!({"type":"function_call_output","call_id":"call-two","output":"read result"}),
        );
        input.push(json!({"role":"user","content":"Finish"}));
        deliver(&enabled, next).await;
        let last = fixture.captured().await.body.unwrap();
        assert_eq!(last["input"][4], first["output"][2]);
        assert_eq!(last["input"][9], second["output"][2]);
        assert!(!last.to_string().contains("caidex.deepseek.native-history"));
        assert_eq!(
            enabled.list_models().await.unwrap()[0].dialects,
            [ResponsesDialect::Classic, ResponsesDialect::Lite]
        );
        fixture.captured().await;
        assert_eq!(reads.load(Ordering::SeqCst), 4);
    }
}

async fn refused(
    provider: &DeepSeekProvider<Store>,
    wire: Value,
) -> caidex_model_core::ProviderError {
    if wire["stream"] != true {
        return provider
            .create_response(lite(wire), local_context())
            .await
            .err()
            .expect("must refuse");
    }
    let mut events = match provider.stream_response(lite(wire), local_context()).await {
        Ok(response) => response.events,
        Err(error) => return error,
    };
    while let Some(event) = events.next().await {
        match event {
            Err(error) => return error,
            Ok(ProviderStreamEvent::Model(event)) => {
                let wire = event.response.wire();
                assert!(
                    !matches!(
                        wire["item"]["type"].as_str(),
                        Some("function_call" | "custom_tool_call")
                    ),
                    "premature tool: {wire}"
                );
                assert!(wire["item"].get("encrypted_content").is_none());
                assert!(
                    !event.response.kind().contains("call_arguments")
                        && !event.response.kind().contains("tool_call_input")
                );
                assert!(event.response.terminal().is_none());
            }
            _ => (),
        }
    }
    panic!("bad response did not fail");
}
fn next(original: &Value, response: &caidex_model_core::CanonicalResponse) -> Value {
    let mut wire = original.clone();
    let input = wire["input"].as_array_mut().unwrap();
    input.extend(response.output().iter().cloned());
    input.push(
        json!({"type":"custom_tool_call_output","call_id":"call-one","output":"actual result"}),
    );
    input.push(json!({"role":"user","content":"Continue"}));
    wire
}

#[tokio::test]
async fn lite_bad_parameters_tools_and_original_budget_refuse_before_key() {
    let fixture = Fixture::start(vec![Reply::json(custom_native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let enabled = provider(&fixture, broker.clone());
    let mut lite_model = metadata("fixture", "native-fixture");
    lite_model.dialects = vec![ResponsesDialect::Lite];
    let config = || {
        DeepSeekConfig::new(reference())
            .unwrap()
            .with_base_url(&fixture.base)
            .unwrap()
    };
    assert!(
        DeepSeekProvider::new(config(), vec![lite_model.clone()], broker.clone(), limits())
            .is_err()
    );
    let mut invalid_metadata = lite_model.clone();
    invalid_metadata.dialects.push(ResponsesDialect::Lite);
    assert!(
        DeepSeekProvider::with_lite_options(
            config(),
            vec![invalid_metadata],
            broker.clone(),
            limits(),
            Default::default()
        )
        .is_err()
    );
    let classic = fixture
        .provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        )
        .with_runtime_context();
    assert_eq!(
        refused(&classic, source(false)).await.code,
        "unsupported_dialect"
    );
    for corrupt in 0..19 {
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
            9 => wire["input"][0]["tools"][0]["tools"][0]["defer_loading"] = true.into(),
            10 => {
                wire["input"][0]["tools"][0]["tools"][0]["format"] =
                    json!({"type":"grammar","syntax":"unknown","definition":"x"})
            }
            11 => {
                wire["input"][0]["tools"][0]["tools"][0]["format"] =
                    json!({"type":"grammar","syntax":"lark","definition":""})
            }
            12 => wire["input"][0]["tools"][0]["tools"][0]["name"] = "bad/name".into(),
            13 => wire["input"][0]["tools"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"web_search"})),
            14 => {
                wire["tool_choice"] =
                    json!({"type":"custom","namespace":"functions","name":"absent"})
            }
            15 => {
                wire["tool_choice"] =
                    json!({"type":"function","namespace":"functions","name":"exec"})
            }
            16 => wire["input"][0]["tools"][0]["tools"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"function","name":"read"})),
            17 => wire["input"][0]["tools"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"function","name":"caidex_ns_0"})),
            _ => wire["input"]
                .as_array_mut()
                .unwrap()
                .push(json!({"role":"developer","content":"late priority"})),
        }
        for stream in [false, true] {
            wire["stream"] = stream.into();
            assert_eq!(
                refused(&enabled, wire.clone()).await.http_status,
                400,
                "case {corrupt}"
            );
        }
    }
    let mut small = limits();
    small.request_bytes = 200;
    let bounded = DeepSeekProvider::with_lite_options(
        config(),
        vec![lite_model],
        broker.clone(),
        small,
        Default::default(),
    )
    .unwrap()
    .with_runtime_context();
    assert_eq!(refused(&bounded, source(false)).await.http_status, 413);
    for cancelled in [false, true] {
        let mut context = local_context();
        if cancelled {
            context.cancellation.cancel();
        } else {
            context.deadline = Some(std::time::Instant::now());
        }
        assert_eq!(
            enabled
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
async fn lite_single_call_rejects_all_terminal_states_before_delivery_and_releases_slot() {
    for stream in [false, true] {
        for status in ["completed", "incomplete", "failed"] {
            for second_custom in [false, true] {
                let mut native = custom_native();
                let mut other = if second_custom {
                    native["output"][2].clone()
                } else {
                    tool_native("caidex_ns_1")["output"][2].clone()
                };
                other["id"] = "fc_two".into();
                other["call_id"] = "call-two".into();
                native["output"].as_array_mut().unwrap().push(other);
                native["status"] = status.into();
                if status == "incomplete" {
                    native["incomplete_details"] = json!({"reason":"max_output_tokens"});
                }
                if status == "failed" {
                    native["error"] = json!({"code":"fixture_error","message":"synthetic failure"});
                }
                let mut bad = reply(native.clone(), stream);
                if stream {
                    let mut chunks = native_chunks(&native);
                    chunks.last_mut().unwrap()["type"] = format!("response.{status}").into();
                    bad.body = chunks.into_iter().map(event).collect();
                    bad.stall = 2;
                }
                let mut fixture = Fixture::start(vec![bad, Reply::json(custom_native())]).await;
                let (broker, _) = fixture_broker(Some(KEY));
                let enabled = provider(&fixture, broker);
                assert_eq!(refused(&enabled, source(stream)).await.http_status, 502);
                fixture.captured().await;
                if stream {
                    fixture.disconnected().await;
                }
                deliver(&enabled, source(false)).await;
                fixture.captured().await;
            }
        }
    }
}

#[tokio::test]
async fn lite_parallel_true_or_absent_and_named_custom_choice_preserve_valid_calls() {
    for stream in [false, true] {
        for explicit in [false, true] {
            let mut native = custom_native();
            let mut second = tool_native("caidex_ns_1")["output"][2].clone();
            second["id"] = "fc_two".into();
            second["call_id"] = "call-two".into();
            native["output"].as_array_mut().unwrap().push(second);
            let mut fixture =
                Fixture::start(vec![reply(native, stream), reply(custom_native(), stream)]).await;
            let (broker, _) = fixture_broker(Some(KEY));
            let enabled = provider(&fixture, broker);
            let mut wire = source(stream);
            if explicit {
                wire["parallel_tool_calls"] = true.into();
            } else {
                wire.as_object_mut().unwrap().remove("parallel_tool_calls");
            }
            wire["input"][0].as_object_mut().unwrap().remove("id");
            wire["input"][0]["tools"][0]["tools"][0]["format"] = json!({"type":"grammar","syntax":if explicit {"lark"} else {"regex"},"definition":"start: /.+/"});
            let response = deliver(&enabled, wire.clone()).await;
            assert_eq!(response.output()[2]["type"], "custom_tool_call");
            assert_eq!(response.output()[3]["type"], "function_call");
            assert_eq!(
                stored(&response.output()[0])["tool_mapping"]["lite_single_tool_call"],
                false
            );
            fixture.captured().await;
            wire["parallel_tool_calls"] = false.into();
            wire["tool_choice"] = json!({"type":"custom","namespace":"functions","name":"exec"});
            deliver(&enabled, wire).await;
            assert_eq!(
                fixture.captured().await.body.unwrap()["tool_choice"],
                json!({"type":"function","name":"caidex_ns_0"})
            );
        }
    }
}

#[tokio::test]
async fn lite_native_custom_payload_kind_choice_and_stream_deltas_are_validated() {
    for stream in [false, true] {
        for corrupt in 0..10 {
            let mut native = custom_native();
            let mut wire = source(stream);
            match corrupt {
                0 => native["output"][2]["arguments"] = json!({"input":"x"}),
                1 => native["output"][2]["arguments"] = "{}".into(),
                2 => native["output"][2]["arguments"] = "{\"input\":1}".into(),
                3 => native["output"][2]["arguments"] = "{\"input\":\"x\",\"extra\":true}".into(),
                4 => native["output"][2]["arguments"] = "[]".into(),
                5 => native["output"][2]["name"] = "exec".into(),
                6 => {
                    native["output"][2]["type"] = "custom_tool_call".into();
                    native["output"][2]["input"] = "raw".into();
                    native["output"][2]
                        .as_object_mut()
                        .unwrap()
                        .remove("arguments");
                }
                7 => wire["tool_choice"] = "none".into(),
                8 => {
                    wire["tool_choice"] =
                        json!({"type":"function","namespace":"functions","name":"read"})
                }
                _ => native["output"][2]["status"] = "in_progress".into(),
            }
            let mut bad = reply(native, stream);
            if stream {
                bad.stall = 2;
            }
            let mut fixture = Fixture::start(vec![bad]).await;
            let (broker, _) = fixture_broker(Some(KEY));
            let enabled = provider(&fixture, broker);
            assert_eq!(
                refused(&enabled, wire).await.http_status,
                502,
                "case {corrupt}"
            );
            if stream {
                fixture.disconnected().await;
            }
        }
    }
    for corrupt in 0..3 {
        let mut chunks = native_chunks(&custom_native());
        match corrupt {
            0 => {
                chunks
                    .iter_mut()
                    .find(|v| v["type"] == "response.function_call_arguments.delta")
                    .unwrap()["delta"] = "{\"input\":\"tampered\"}".into()
            }
            1 => {
                chunks
                    .iter_mut()
                    .find(|v| v["type"] == "response.function_call_arguments.done")
                    .unwrap()["arguments"] = "{\"input\":\"tampered\"}".into()
            }
            _ => {
                chunks
                    .iter_mut()
                    .find(|v| {
                        v["type"] == "response.output_item.added"
                            && v["item"]["type"] == "function_call"
                    })
                    .unwrap()["output_index"] = 99.into()
            }
        }
        let mut bad = Reply::stream(chunks.into_iter().map(event).collect());
        bad.stall = 2;
        let mut fixture = Fixture::start(vec![bad]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        assert_eq!(
            refused(&provider(&fixture, broker), source(true))
                .await
                .http_status,
            502
        );
        fixture.disconnected().await;
    }
}

#[tokio::test]
async fn lite_history_binds_source_policy_dialect_scope_and_result_kind_before_key() {
    for stream in [false, true] {
        let mut fixture = Fixture::start(vec![reply(custom_native(), stream)]).await;
        let (broker, reads) = fixture_broker(Some(KEY));
        let enabled = provider(&fixture, broker.clone());
        let original = source(stream);
        let response = deliver(&enabled, original.clone()).await;
        fixture.captured().await;
        let good = next(&original, &response);
        for corrupt in 0..12 {
            let mut wire = good.clone();
            match corrupt {
                0 => wire["parallel_tool_calls"] = true.into(),
                1 => {
                    wire["input"][0]["tools"][0]["tools"][0]["description"] =
                        "changed source".into()
                }
                2 => wire["input"][1]["content"] = "changed prefix".into(),
                3 => wire["input"][5]["input"] = "changed display".into(),
                4 => wire["input"][6]["type"] = "function_call_output".into(),
                5 => wire["input"][6]["call_id"] = "other".into(),
                6 => {
                    wire["input"][3]["encrypted_content"] = wire["input"][3]["encrypted_content"]
                        .as_str()
                        .unwrap()
                        .replacen("v3:", "v2:", 1)
                        .into()
                }
                _ => {
                    let mut history = stored(&wire["input"][3]);
                    match corrupt {
                        7 => history["version"] = 2.into(),
                        8 => history["tool_mapping"]["lite_single_tool_call"] = Value::Null,
                        9 => {
                            history["tool_mapping"]
                                .as_object_mut()
                                .unwrap()
                                .remove("lite_single_tool_call");
                        }
                        10 => {
                            history["response"]["output"][2]["arguments"] = "{\"input\":3}".into()
                        }
                        _ => {
                            history["request"]["tools"][0]["parameters"]["properties"]["input"]["type"] =
                                "number".into()
                        }
                    }
                    wire["input"][3]["encrypted_content"] =
                        format!("caidex.deepseek.native-history.v3:{history}").into();
                }
            }
            assert_eq!(
                refused(&enabled, wire).await.http_status,
                400,
                "case {corrupt}"
            );
        }
        let mut classic_wire = good.clone();
        classic_wire["tools"] = classic_wire["input"][0]["tools"].clone();
        classic_wire["input"].as_array_mut().unwrap().remove(0);
        classic_wire["parallel_tool_calls"] = true.into();
        assert_eq!(
            enabled
                .create_response(request(classic_wire), local_context())
                .await
                .err()
                .unwrap()
                .http_status,
            400
        );
        for change in 0..4 {
            let mut reference = reference();
            let mut base = fixture.base.clone();
            let mut model = metadata("fixture", "native-fixture");
            match change {
                0 => reference.owner = Id::new("other").unwrap(),
                1 => reference.profile = Id::new("other").unwrap(),
                2 => base.push_str("/other"),
                _ => model.native_model = "other".into(),
            }
            model.dialects = vec![ResponsesDialect::Lite];
            let other = DeepSeekProvider::with_lite_options(
                DeepSeekConfig::new(reference)
                    .unwrap()
                    .with_base_url(&base)
                    .unwrap(),
                vec![model],
                broker.clone(),
                limits(),
                Default::default(),
            )
            .unwrap()
            .with_runtime_context();
            assert_eq!(refused(&other, good.clone()).await.http_status, 400);
        }
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn lite_caller_supplied_custom_history_maps_only_matching_results() {
    let mut fixture = Fixture::start(vec![Reply::json(history_text())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let enabled = provider(&fixture, broker);
    let mut wire = source(false);
    wire["input"].as_array_mut().unwrap().extend([
        json!({"type":"custom_tool_call","name":"exec","namespace":"functions","call_id":"manual","input":"exact\n你好🙂"}),
        json!({"type":"custom_tool_call_output","call_id":"manual","output":"actual output"}),
        json!({"role":"user","content":"Continue"}),
    ]);
    deliver(&enabled, wire.clone()).await;
    let sent = fixture.captured().await.body.unwrap();
    assert_eq!(sent["input"][2]["type"], "function_call");
    assert_eq!(
        serde_json::from_str::<Value>(sent["input"][2]["arguments"].as_str().unwrap()).unwrap(),
        json!({"input":"exact\n你好🙂"})
    );
    assert_eq!(sent["input"][3]["type"], "function_call_output");
    for corrupt in 0..4 {
        let mut bad = wire.clone();
        match corrupt {
            0 => {
                bad["input"][3]["type"] = "function_call".into();
                bad["input"][3]["arguments"] = "{\"input\":\"x\"}".into();
                bad["input"][3].as_object_mut().unwrap().remove("input");
            }
            1 => bad["input"][4]["type"] = "function_call_output".into(),
            2 => bad["input"][4]["call_id"] = "unknown".into(),
            _ => bad["input"][3]["input"] = 3.into(),
        }
        assert_eq!(refused(&enabled, bad).await.http_status, 400);
    }
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn lite_only_text_route_keeps_metadata_and_unsupported_tool_reasoning_gates() {
    let mut fixture =
        Fixture::start(vec![Reply::json(history_text()), Reply::json(catalog())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let mut model = metadata("fixture", "native-fixture");
    model.dialects = vec![ResponsesDialect::Lite];
    model.capabilities.native_tools = CapabilitySupport::Unsupported;
    let enabled = DeepSeekProvider::with_lite_options(
        DeepSeekConfig::new(reference())
            .unwrap()
            .with_base_url(&fixture.base)
            .unwrap(),
        vec![model.clone()],
        broker.clone(),
        limits(),
        Default::default(),
    )
    .unwrap()
    .with_runtime_context();
    assert_eq!(
        enabled.metadata("fixture").unwrap().dialects,
        model.dialects
    );
    assert_eq!(
        enabled.capabilities("fixture").unwrap().native_tools,
        CapabilitySupport::Unsupported
    );
    let CredentialRequirement::Bearer {
        reference: configured,
    } = enabled.credential_requirements("fixture").unwrap()
    else {
        panic!("expected executor Bearer");
    };
    assert_eq!(configured, reference());
    assert_eq!(
        refused(&enabled, source(false)).await.code,
        "unsupported_tools"
    );
    let mut wire = source(false);
    wire["input"].as_array_mut().unwrap().remove(0);
    deliver(&enabled, wire.clone()).await;
    fixture.captured().await;
    assert_eq!(
        enabled.list_models().await.unwrap()[0].dialects,
        model.dialects
    );
    fixture.captured().await;
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    model.capabilities.reasoning = CapabilitySupport::Unsupported;
    let (broker, rejected_reads) = fixture_broker(Some(KEY));
    let no_reasoning = DeepSeekProvider::with_lite_options(
        DeepSeekConfig::new(reference())
            .unwrap()
            .with_base_url(&fixture.base)
            .unwrap(),
        vec![model],
        broker,
        limits(),
        Default::default(),
    )
    .unwrap()
    .with_runtime_context();
    assert_eq!(
        refused(&no_reasoning, wire).await.code,
        "unsupported_reasoning"
    );
    assert_eq!(rejected_reads.load(Ordering::SeqCst), 0);
    assert!(
        enabled
            .create_response(request(basic(false)), RequestContext::default())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn lite_partial_custom_stream_cancel_drop_and_compiled_budget_preserve_limits() {
    for cancel in [false, true] {
        let mut chunks = native_chunks(&custom_native());
        chunks.truncate(
            chunks
                .iter()
                .position(|c| c["type"] == "response.function_call_arguments.done")
                .unwrap(),
        );
        let count = chunks.len() + 1; // response.created also adds the local reasoning placeholder.
        let mut bad = Reply::stream(chunks.into_iter().map(event).collect());
        bad.stall = 2;
        let mut fixture = Fixture::start(vec![bad, Reply::json(custom_native())]).await;
        let (broker, _) = fixture_broker(Some(KEY));
        let enabled = provider(&fixture, broker);
        let context = local_context();
        let cancellation = context.cancellation.clone();
        let mut response = enabled
            .stream_response(lite(source(true)), context)
            .await
            .unwrap();
        for _ in 0..count {
            if let ProviderStreamEvent::Model(event) =
                response.events.next().await.unwrap().unwrap()
            {
                assert!(!matches!(
                    event.response.wire()["item"]["type"].as_str(),
                    Some("function_call" | "custom_tool_call")
                ));
                assert!(
                    event.response.wire()["item"]
                        .get("encrypted_content")
                        .is_none()
                );
            }
        }
        if cancel {
            cancellation.cancel();
            assert_eq!(
                response.events.next().await.unwrap().err().unwrap().code,
                "provider_cancelled"
            );
            assert!(response.events.next().await.is_none());
        }
        drop(response);
        fixture.disconnected().await;
        deliver(&enabled, source(false)).await;
    }
    let fixture = Fixture::start(vec![Reply::json(history_text())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let mut wire = source(false);
    wire.as_object_mut().unwrap().remove("include");
    wire.as_object_mut().unwrap().remove("reasoning");
    // Grammar is guidance, so its generated native description expands the body.
    wire["input"][0]["tools"][0]["tools"][0]["format"] =
        json!({"type":"grammar","syntax":"regex","definition":"x+"});
    let mut small = limits();
    small.request_bytes = wire.to_string().len() + 8;
    let mut model = metadata("fixture", "native-fixture");
    model.dialects = vec![ResponsesDialect::Lite];
    let enabled = DeepSeekProvider::with_lite_options(
        DeepSeekConfig::new(reference())
            .unwrap()
            .with_base_url(&fixture.base)
            .unwrap(),
        vec![model],
        broker,
        small,
        Default::default(),
    )
    .unwrap()
    .with_runtime_context();
    assert_eq!(refused(&enabled, wire).await.http_status, 413);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn lite_gateway_translates_dialect_and_keeps_tokens_context_and_raw_history_separate() {
    let mut fixture = Fixture::start(vec![
        reply(custom_native(), true),
        Reply::json(history_text()),
    ])
    .await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let enabled = provider(&fixture, broker.clone())
        .with_reasoning_effort_mapping("medium".into(), "high".into())
        .unwrap()
        .with_reasoning_effort_mapping("high".into(), "low".into())
        .unwrap()
        .with_verbosity_instruction("low".into(), "Executor guidance".into())
        .unwrap();
    let gateway =
        caidex_model_gateway::start_with_provider(Arc::new(enabled), broker.redactor(), limits())
            .await
            .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut original = source(true);
    original["reasoning"]["effort"] = "medium".into();
    original["text"] = json!({"verbosity":"low"});
    original["client_metadata"] = json!({"origin":"LOCAL_CLIENT"});
    original["prompt_cache_key"] = "LOCAL_CACHE".into();
    let mut outgoing = client
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("x-openai-internal-codex-responses-lite", "true")
        .header("content-type", "application/json")
        .body(original.to_string());
    for (name, value) in local_context().headers.iter() {
        outgoing = outgoing.header(name, value);
    }
    let response = outgoing.send().await.unwrap();
    assert_eq!(response.status(), 200);
    let mut parser = caidex_model_core::ResponsesStream::new(limits().frame_bytes).unwrap();
    let events = parser.push(&response.bytes().await.unwrap()).unwrap();
    parser.finish().unwrap();
    let display = caidex_model_core::CanonicalResponse::new(
        events
            .iter()
            .find(|e| e.response.kind() == "response.completed")
            .unwrap()
            .response
            .wire()["response"]
            .clone(),
    )
    .unwrap();
    assert_eq!(
        events
            .iter()
            .find(|e| e.response.kind() == "response.custom_tool_call_input.delta")
            .unwrap()
            .response
            .wire()["delta"],
        "line 1\n你好🙂\n"
    );
    let captured = fixture.captured().await;
    assert_eq!(
        captured.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert!(
        captured
            .header("x-openai-internal-codex-responses-lite")
            .is_none()
    );
    assert!(
        !captured.headers.contains(gateway.token().expose())
            && !captured.headers.contains("LOCAL_")
    );
    let native = captured.body.unwrap();
    assert_eq!(native["reasoning"], json!({"effort":"high"}));
    assert_eq!(native["instructions"], "Executor guidance");
    assert!(native.get("client_metadata").is_none() && native.get("prompt_cache_key").is_none());
    assert!(
        !display.wire().to_string().contains(KEY)
            && !display
                .wire()
                .to_string()
                .contains(gateway.token().expose())
    );
    let mut followup = next(&original, &display);
    followup["stream"] = false.into();
    let response = client
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("x-openai-internal-codex-responses-lite", "true")
        .header("content-type", "application/json")
        .body(followup.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let response: Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert_eq!(response["output"][0]["type"], "reasoning");
    let native_second = fixture.captured().await.body.unwrap();
    assert_eq!(native_second["reasoning"]["effort"], "high");
    assert_eq!(
        &native_second["input"].as_array().unwrap()[2..5],
        custom_native()["output"].as_array().unwrap()
    );
    assert!(!native_second.to_string().contains("native-history"));
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn lite_and_classic_function_history_cannot_cross_dialects_or_upgrade_carriers() {
    let mut fixture = Fixture::start(vec![Reply::json(tool_native("caidex_ns_0"))]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    let enabled = provider(&fixture, broker);
    let mut lite_wire = source(false);
    lite_wire["input"][0]["tools"][0]["tools"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    lite_wire["parallel_tool_calls"] = true.into();
    let declarations = lite_wire["input"][0]["tools"].clone();
    let mut classic_wire = lite_wire.clone();
    classic_wire["tools"] = declarations;
    classic_wire["input"].as_array_mut().unwrap().remove(0);
    classic_wire
        .as_object_mut()
        .unwrap()
        .remove("parallel_tool_calls");
    let classic = enabled
        .create_response(request(classic_wire.clone()), local_context())
        .await
        .unwrap()
        .response;
    assert!(
        classic.output()[0]["encrypted_content"]
            .as_str()
            .unwrap()
            .starts_with("caidex.deepseek.native-history.v1:")
    );
    let lite_response = deliver(&enabled, lite_wire.clone()).await;
    assert_eq!(
        stored(&lite_response.output()[0])["tool_mapping"]["lite_single_tool_call"],
        false
    );
    fixture.captured().await;
    fixture.captured().await;
    for (from_lite, output) in [(true, lite_response.output()), (false, classic.output())] {
        let mut other = if from_lite {
            classic_wire.clone()
        } else {
            lite_wire.clone()
        };
        let input = other["input"].as_array_mut().unwrap();
        input.extend(output.iter().cloned());
        input.push(
            json!({"type":"function_call_output","call_id":"call-one","output":"actual result"}),
        );
        input.push(json!({"role":"user","content":"Continue"}));
        let error = if from_lite {
            enabled
                .create_response(request(other), local_context())
                .await
                .err()
                .unwrap()
        } else {
            refused(&enabled, other).await
        };
        assert_eq!(error.code, "deepseek_history_prefix_mismatch");
    }
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 2);
}
