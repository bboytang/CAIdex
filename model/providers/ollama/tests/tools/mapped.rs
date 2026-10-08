use super::*;

fn custom() -> Value {
    json!({"type":"namespace","name":"functions","description":"Original namespace","tools":[{"type":"custom","name":"patch","description":"Apply exact patch","format":{"type":"grammar","syntax":"lark","definition":"start: TEXT\nTEXT: /.+/"}}]})
}
fn call(id: &str) -> Value {
    json!({"type":"function_call","id":format!("fc_{id}"),"status":"completed","namespace":"functions","name":"patch","call_id":id,"arguments":" { \"input\" : \"line 1\\n你好🙂\\n\" } "})
}

#[tokio::test]
async fn custom_json_sse_v2_binds_original_grammar_and_replays_raw_native_arguments() {
    for stream in [false, true] {
        let native = terminal(vec![call("c1")]);
        let mut ordinary = function("c2");
        ordinary.as_object_mut().unwrap().remove("namespace");
        let mut fixture = Fixture::start(vec![
            reply(native.clone(), stream),
            reply(terminal(vec![ordinary.clone()]), stream),
            reply(finished(), stream),
        ])
        .await;
        let (broker, reads) = broker();
        let provider = fixture
            .provider(broker, true)
            .with_custom_tools_as_functions();
        let user = json!({"role":"user","content":"patch"});
        let mut wire =
            json!({"model":"fixture","input":[user],"tools":[custom(),namespace()["tools"][0]]});
        let first = deliver(&provider, wire.clone(), stream).await.unwrap();
        assert!(
            first.output()[0]["encrypted_content"]
                .as_str()
                .unwrap()
                .starts_with("caidex.ollama.native-history.v2:")
        );
        assert_eq!(first.output()[1]["type"], "custom_tool_call");
        assert_eq!(first.output()[1]["input"], "line 1\n你好🙂\n");
        assert!(first.output()[1].get("arguments").is_none());
        let original = fixture.request().await.body.unwrap();
        let declaration = &original["tools"][0]["tools"][0];
        assert_eq!(declaration["type"], "function");
        assert_eq!(
            declaration["parameters"],
            json!({"type":"object","properties":{"input":{"type":"string"}},"required":["input"],"additionalProperties":false})
        );
        assert!(
            declaration["description"]
                .as_str()
                .unwrap()
                .contains("start: TEXT\nTEXT: /.+/")
        );
        assert!(
            declaration["description"]
                .as_str()
                .unwrap()
                .contains("not native constrained decoding")
        );
        wire["input"]
            .as_array_mut()
            .unwrap()
            .extend(first.output().iter().cloned());
        wire["input"].as_array_mut().unwrap().push(
            json!({"type":"custom_tool_call_output","call_id":"c1","output":"exact result\n🙂"}),
        );
        let mut changed = wire.clone();
        changed["tools"][0]["tools"][0]["format"]["definition"] = "start: OTHER".into();
        assert!(deliver(&provider, changed, stream).await.is_err());
        let mut changed = wire.clone();
        changed["tools"] = original["tools"].clone();
        assert!(
            deliver(&provider, changed, stream).await.is_err(),
            "identical native schema must not erase original custom kind"
        );
        let mut changed = wire.clone();
        changed["input"][2]["input"] = "changed".into();
        assert!(deliver(&provider, changed, stream).await.is_err());
        for variant in 0..3 {
            let mut changed = wire.clone();
            let capsule = changed["input"][1]["encrypted_content"]
                .as_str()
                .unwrap()
                .strip_prefix("caidex.ollama.native-history.v2:")
                .unwrap();
            let mut record: Value = serde_json::from_str(capsule).unwrap();
            match variant {
                0 => {
                    record["request"]["tools"][0]["tools"][0]["parameters"] =
                        json!({"type":"object"})
                }
                1 => record["version"] = 1.into(),
                _ => record["response"]["output"][0]["arguments"] = "{\"input\":1}".into(),
            }
            changed["input"][1]["encrypted_content"] =
                format!("caidex.ollama.native-history.v2:{record}").into();
            assert!(deliver(&provider, changed, stream).await.is_err());
        }
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        let second = deliver(&provider, wire.clone(), stream).await.unwrap();
        assert_eq!(second.output()[1], ordinary);
        let captured = fixture.request().await.body.unwrap();
        assert_eq!(captured["input"][1], native["output"][0]);
        assert_eq!(
            captured["input"][2],
            json!({"type":"function_call_output","call_id":"c1","output":"exact result\n🙂"})
        );
        wire["input"]
            .as_array_mut()
            .unwrap()
            .extend(second.output().iter().cloned());
        wire["input"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"function_call_output","call_id":"c2","output":"ordinary result"}));
        wire["input"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"user","content":"third"}));
        deliver(&provider, wire, stream).await.unwrap();
        let third = fixture.request().await.body.unwrap();
        assert_eq!(third["input"][1], native["output"][0]);
        assert!(third["input"].as_array().unwrap().contains(&ordinary));
        assert_eq!(reads.load(Ordering::SeqCst), 3);
    }
}

#[tokio::test]
async fn custom_discovery_v2_binds_prefix_subset_and_repeated_original_declarations() {
    for stream in [false, true] {
        let responses = [
            terminal(vec![searching("s1")]),
            terminal(vec![call("c1")]),
            terminal(vec![searching("s2")]),
            finished(),
        ];
        let mut fixture = Fixture::start(
            responses
                .iter()
                .cloned()
                .map(|v| reply(v, stream))
                .collect(),
        )
        .await;
        let (broker, reads) = broker();
        let provider = fixture
            .provider(broker, true)
            .with_custom_tools_as_functions();
        let mut wire = json!({"model":"fixture","input":[{"role":"user","content":"search"}],"tools":[search()]});
        for turn in 0..4 {
            let response = deliver(&provider, wire.clone(), stream).await.unwrap();
            let captured = fixture.request().await.body.unwrap();
            if turn == 0 {
                let capsule = response.output()[0]["encrypted_content"]
                    .as_str()
                    .unwrap()
                    .strip_prefix("caidex.ollama.native-history.v2:")
                    .unwrap();
                assert_eq!(
                    serde_json::from_str::<Value>(capsule).unwrap()["tool_mapping"]["search_results"],
                    json!([])
                );
            } else {
                let results: Vec<_> = captured["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|v| v["type"] == "tool_search_output")
                    .collect();
                assert_eq!(results[0]["tools"][0]["tools"][0]["type"], "function");
            }
            if turn == 1 {
                assert_eq!(response.output()[1]["type"], "custom_tool_call");
            }
            if turn >= 2 {
                assert!(
                    captured["input"]
                        .as_array()
                        .unwrap()
                        .contains(&responses[1]["output"][0])
                );
            }
            if turn == 3 {
                break;
            }
            let input = wire["input"].as_array_mut().unwrap();
            input.extend(response.output().iter().cloned());
            input.push(if turn==1 { json!({"type":"custom_tool_call_output","call_id":"c1","output":"result"}) } else { json!({"type":"tool_search_output","execution":"client","status":"completed","call_id":if turn==0 {"s1"} else {"s2"},"tools":[custom()]}) });
            input.push(json!({"role":"user","content":"continue"}));
            if turn == 2 {
                let mut changed = wire.clone();
                let items = changed["input"].as_array_mut().unwrap();
                let last = items.len() - 2;
                items[last]["tools"][0]["tools"][0]["format"]["definition"] = "changed".into();
                assert_eq!(
                    deliver(&provider, changed, stream)
                        .await
                        .err()
                        .unwrap()
                        .code,
                    "ollama_invalid_tool_mapping"
                );
            }
        }
        assert_eq!(reads.load(Ordering::SeqCst), 4);
    }
}

#[tokio::test]
async fn custom_bad_terminal_arguments_never_release_tools_history_or_success() {
    for stream in [false, true] {
        for arguments in [
            "{}",
            "[]",
            "{\"input\":1}",
            "{\"input\":\"ok\",\"extra\":true}",
            "bad",
        ] {
            let mut bad = call("c1");
            bad["arguments"] = arguments.into();
            let mut fixture = Fixture::start(vec![
                reply(terminal(vec![bad]), stream),
                reply(finished(), stream),
            ])
            .await;
            let (broker, reads) = broker();
            let provider = fixture
                .provider(broker, true)
                .with_custom_tools_as_functions();
            let mut wire =
                json!({"model":"fixture","input":"patch","tools":[custom()],"stream":stream});
            if stream {
                let mut events = provider
                    .stream_response(request(wire.clone()), RequestContext::default())
                    .await
                    .unwrap()
                    .events;
                let mut failed = false;
                while let Some(event) = events.next().await {
                    match event {
                        Err(error) => {
                            assert_eq!(error.code, "ollama_invalid_native_tools");
                            failed = true;
                        }
                        Ok(ProviderStreamEvent::Model(event)) => {
                            assert!(event.response.terminal().is_none());
                            assert!(!event.response.kind().contains("custom_tool_call"));
                            assert_ne!(event.response.kind(), "response.output_item.done");
                        }
                        _ => (),
                    }
                }
                assert!(failed);
            } else {
                assert_eq!(
                    deliver(&provider, wire.clone(), stream)
                        .await
                        .err()
                        .unwrap()
                        .code,
                    "ollama_invalid_native_tools"
                );
            }
            fixture.request().await;
            wire["input"] = "retry explicitly".into();
            deliver(&provider, wire, stream).await.unwrap();
            fixture.request().await;
            assert_eq!(reads.load(Ordering::SeqCst), 2);
        }
    }
}

#[tokio::test]
async fn custom_defaults_declarations_and_history_kinds_refuse_before_authentication() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = broker();
    let default = fixture.provider(broker.clone(), true).with_native_tools();
    let mapped = fixture
        .provider(broker.clone(), true)
        .with_custom_tools_as_functions();
    for stream in [false, true] {
        let source = json!({"model":"fixture","input":"patch","tools":[custom()]});
        assert!(deliver(&default, source.clone(), stream).await.is_err());
        for bad in [
            json!({"type":"grammar","syntax":"other","definition":"x"}),
            json!({"type":"grammar","syntax":"lark","definition":" "}),
            json!({"type":"text","unknown":1}),
            json!({"type":"grammar","syntax":"regex","definition":"x","unknown":1}),
        ] {
            let mut wire = source.clone();
            wire["tools"][0]["tools"][0]["format"] = bad;
            assert!(deliver(&mapped, wire, stream).await.is_err());
        }
        for flag in ["defer_loading", "strict"] {
            let mut wire = source.clone();
            wire["tools"][0]["tools"][0][flag] = true.into();
            assert!(deliver(&mapped, wire, stream).await.is_err());
        }
        for result in [
            json!({"type":"function_call_output","call_id":"c1","output":"wrong kind"}),
            json!({"type":"function_call_output","namespace":"functions","name":"patch","output":"legacy wrong kind"}),
            json!({"type":"custom_tool_call_output","call_id":"missing","output":"unpaired"}),
        ] {
            let mut wire = source.clone();
            wire["input"] = json!([{"type":"custom_tool_call","namespace":"functions","name":"patch","call_id":"c1","input":"patch"},result]);
            assert!(deliver(&mapped, wire, stream).await.is_err());
        }
    }
    let wire = json!({"model":"fixture","input":"patch","tools":[custom()]});
    for budget in [wire.to_string().len() - 1, wire.to_string().len() + 20] {
        let mut bounded = limits();
        bounded.request_bytes = budget;
        let provider = OllamaProvider::new(
            OllamaConfig::new(&fixture.base, Some(reference())).unwrap(),
            vec![model("fixture", "native-fixture")],
            broker.clone(),
            bounded,
        )
        .unwrap()
        .with_custom_tools_as_functions();
        for stream in [false, true] {
            assert_eq!(
                deliver(&provider, wire.clone(), stream)
                    .await
                    .err()
                    .unwrap()
                    .code,
                "invalid_or_oversized_body"
            );
        }
    }
    let cancelled = RequestContext::default();
    cancelled.cancellation.cancel();
    assert_eq!(
        mapped
            .create_response(
                request(wire.clone()),
                RequestContext {
                    cancellation: cancelled.cancellation.clone(),
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
        mapped
            .stream_response(request(wire), cancelled)
            .await
            .err()
            .unwrap()
            .code,
        "provider_cancelled"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn custom_v2_and_native_v1_profiles_never_upgrade_or_mix() {
    for stream in [false, true] {
        let mut fixture = Fixture::start(vec![
            reply(terminal(vec![call("c1")]), stream),
            reply(finished(), stream),
        ])
        .await;
        let (broker, reads) = broker();
        let mapped = fixture
            .provider(broker.clone(), true)
            .with_custom_tools_as_functions();
        let native = fixture.provider(broker, true).with_native_tools();
        let user = json!({"role":"user","content":"patch"});
        let first = deliver(
            &mapped,
            json!({"model":"fixture","input":[user],"tools":[custom()]}),
            stream,
        )
        .await
        .unwrap();
        let captured = fixture.request().await.body.unwrap();
        let mut input = vec![user.clone()];
        input.extend(first.output().iter().cloned());
        input.push(json!({"type":"custom_tool_call_output","call_id":"c1","output":"result"}));
        assert!(
            deliver(
                &native,
                json!({"model":"fixture","input":input,"tools":captured["tools"]}),
                stream
            )
            .await
            .is_err()
        );
        let second = deliver(
            &native,
            json!({"model":"fixture","input":[user],"tools":captured["tools"]}),
            stream,
        )
        .await
        .unwrap();
        fixture.request().await;
        assert!(
            second.output()[0]["encrypted_content"]
                .as_str()
                .unwrap()
                .starts_with("caidex.ollama.native-history.v1:")
        );
        let mut input = vec![user];
        input.extend(second.output().iter().cloned());
        assert!(
            deliver(
                &mapped,
                json!({"model":"fixture","input":input,"tools":[custom()]}),
                stream
            )
            .await
            .is_err()
        );
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn custom_stream_emits_input_events_only_after_valid_terminal_and_preserves_partial_state() {
    for status in ["completed", "failed", "incomplete"] {
        let mut flat = call("c1");
        flat.as_object_mut().unwrap().remove("namespace");
        let mut declaration = custom()["tools"][0].clone();
        declaration["format"] = json!({"type":"text"});
        let mut native = terminal(vec![flat]);
        native["status"] = status.into();
        let fixture = Fixture::start(vec![reply(native, true)]).await;
        let (broker, _) = broker();
        let provider = fixture
            .provider(broker, true)
            .with_custom_tools_as_functions();
        let mut events = provider
            .stream_response(
                request(
                    json!({"model":"fixture","input":"patch","tools":[declaration],"stream":true}),
                ),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .events;
        let mut wires = Vec::new();
        while let Some(event) = events.next().await {
            if let ProviderStreamEvent::Model(event) = event.unwrap() {
                wires.push(event.response.wire().clone());
            }
        }
        assert_eq!(wires.last().unwrap()["response"]["status"], status);
        assert!(!wires.iter().any(|v| {
            v["type"]
                .as_str()
                .unwrap()
                .starts_with("response.function_call_arguments")
        }));
        let calls: Vec<_> = wires
            .iter()
            .filter(|v| v["item"]["type"] == "custom_tool_call")
            .collect();
        if status == "completed" {
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[0]["item"]["input"], "");
            assert_eq!(calls[0]["item"]["status"], "in_progress");
            assert_eq!(calls[1]["item"]["input"], "line 1\n你好🙂\n");
            for (kind, field) in [
                ("response.custom_tool_call_input.delta", "delta"),
                ("response.custom_tool_call_input.done", "input"),
            ] {
                let input: Vec<_> = wires.iter().filter(|v| v["type"] == kind).collect();
                assert_eq!(input.len(), 1);
                assert_eq!(input[0][field], "line 1\n你好🙂\n");
                assert_eq!(input[0]["item_id"], "fc_c1");
                assert_eq!(input[0]["output_index"], 1);
            }
        } else {
            assert!(calls.is_empty());
            assert!(!wires.iter().any(|v| {
                v["type"]
                    .as_str()
                    .unwrap()
                    .starts_with("response.custom_tool_call_input")
            }));
        }
    }
}
