use super::*;

fn provider(fixture: &Fixture, broker: Arc<Broker<Store>>) -> OllamaProvider<Store> {
    let mut metadata = model("fixture", "native-fixture");
    metadata.dialects = vec![ResponsesDialect::Lite];
    OllamaProvider::with_lite_options(
        OllamaConfig::new(&fixture.base, Some(reference())).unwrap(),
        vec![metadata],
        broker,
        limits(),
        Default::default(),
    )
    .unwrap()
}
fn additional() -> Value {
    let mut tools = namespace();
    tools["tools"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"custom","name":"exec","format":{"type":"text"}}));
    json!({"type":"additional_tools","id":"at_fixture","role":"developer","tools":[tools]})
}
fn source(stream: bool) -> Value {
    json!({"model":"fixture","stream":stream,"parallel_tool_calls":false,"input":[additional(),{"role":"user","content":"execute exact input"}]})
}
fn lite(wire: Value) -> CanonicalRequest {
    CanonicalRequest::new(wire, ResponsesDialect::Lite).unwrap()
}
fn custom_call() -> Value {
    json!({"type":"function_call","id":"fc_custom","status":"completed","namespace":"functions","name":"exec","call_id":"custom","arguments":" { \"input\" : \"line 1\\n你好🙂\\n\" } "})
}

#[tokio::test]
async fn lite_only_route_json_sse_compiles_items_and_replays_raw_function_custom_history() {
    for stream in [false, true] {
        let raw = custom_call();
        let mut fixture = Fixture::start(vec![
            reply(terminal(vec![raw.clone()]), stream),
            reply(terminal(vec![function("c2")]), stream),
            reply(finished(), stream),
            Reply::json(catalog()),
        ])
        .await;
        let (broker, reads) = broker();
        let provider = provider(&fixture, broker);
        assert_eq!(
            provider.metadata("fixture").unwrap().dialects,
            [ResponsesDialect::Lite]
        );
        assert!(
            provider
                .metadata("fixture")
                .unwrap()
                .codex_compatibility
                .is_none()
        );
        let mut wire = source(stream);
        for turn in 0..3 {
            let response = deliver_request(&provider, lite(wire.clone()))
                .await
                .unwrap();
            let captured = fixture.request().await;
            assert!(
                captured
                    .header("x-openai-internal-codex-responses-lite")
                    .is_none()
            );
            let native = captured.body.unwrap();
            assert_eq!(native["model"], "native-fixture");
            assert!(native.get("parallel_tool_calls").is_none());
            assert!(
                native["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|v| v["type"] != "additional_tools")
            );
            assert_eq!(native["tools"][0]["tools"][1]["type"], "function");
            if turn > 0 {
                assert!(native["input"].as_array().unwrap().contains(&raw));
            }
            if turn == 0 {
                assert_eq!(response.output()[1]["type"], "custom_tool_call");
                assert_eq!(response.output()[1]["input"], "line 1\n你好🙂\n");
            }
            if turn == 2 {
                break;
            }
            let input = wire["input"].as_array_mut().unwrap();
            input.extend(response.output().iter().cloned());
            input.push(if turn == 0 {
                json!({"type":"custom_tool_call_output","call_id":"custom","output":"exact custom result\n你好"})
            } else {
                json!({"type":"function_call_output","call_id":"c2","output":"ordinary result"})
            });
            input.push(json!({"role":"user","content":"continue"}));
            // Additional-tools IDs are transport identity, consumed locally.
            wire["input"][0]["id"] = format!("at_turn_{turn}").into();
        }
        let models = provider.list_models().await.unwrap();
        fixture.request().await;
        assert_eq!(models[0].dialects, [ResponsesDialect::Lite]);
        assert_eq!(models[0].source, EvidenceSource::ProviderCatalog);
        assert_eq!(reads.load(Ordering::SeqCst), 4);
    }
}

#[tokio::test]
async fn lite_invalid_items_routes_capabilities_and_original_budget_refuse_before_authentication() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = broker();
    let enabled = provider(&fixture, broker.clone());
    let default = fixture
        .provider(broker.clone(), true)
        .with_deferred_tool_search();
    for stream in [false, true] {
        assert_eq!(
            deliver_request(&default, lite(source(stream)))
                .await
                .err()
                .unwrap()
                .code,
            "unsupported_dialect"
        );
        let mut wrong_route = source(stream);
        wrong_route["input"].as_array_mut().unwrap().remove(0);
        assert_eq!(
            deliver_request(&enabled, request(wrong_route))
                .await
                .err()
                .unwrap()
                .code,
            "unsupported_dialect"
        );
        for (field, value) in [
            ("role", json!("user")),
            ("id", json!(null)),
            ("id", json!(1)),
            ("id", json!("\n")),
            ("tools", json!(null)),
            ("unknown", json!(true)),
        ] {
            let mut wire = source(stream);
            wire["input"][0][field] = value;
            assert!(
                deliver_request(&enabled, lite(wire)).await.is_err(),
                "{field}"
            );
        }
        for value in [json!(null), json!("false"), json!(0)] {
            let mut wire = source(stream);
            wire["parallel_tool_calls"] = value;
            assert!(deliver_request(&enabled, lite(wire)).await.is_err());
        }
        for duplicate in [false, true] {
            let mut wire = source(stream);
            if !duplicate {
                wire["input"].as_array_mut().unwrap().remove(0);
            }
            wire["input"].as_array_mut().unwrap().push(additional());
            assert!(deliver_request(&enabled, lite(wire)).await.is_err());
        }
        for (field, value) in [
            ("parameters", json!(null)),
            ("strict", json!(true)),
            ("defer_loading", json!(true)),
            ("unknown", json!(true)),
        ] {
            let mut wire = source(stream);
            wire["input"][0]["tools"][0]["tools"][0][field] = value;
            assert!(deliver_request(&enabled, lite(wire)).await.is_err());
        }
        let mut restricted = model("fixture", "native-fixture");
        restricted.dialects = vec![ResponsesDialect::Lite];
        restricted.capabilities.native_tools = caidex_model_core::CapabilitySupport::Unsupported;
        let restricted = OllamaProvider::with_lite_options(
            OllamaConfig::new(&fixture.base, Some(reference())).unwrap(),
            vec![restricted],
            broker.clone(),
            limits(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            deliver_request(&restricted, lite(source(stream)))
                .await
                .err()
                .unwrap()
                .code,
            "ollama_unsupported_capability"
        );
        let mut metadata = model("fixture", "native-fixture");
        metadata.dialects = vec![ResponsesDialect::Lite];
        let limited = OllamaProvider::with_lite_options(
            OllamaConfig::new(&fixture.base, Some(reference())).unwrap(),
            vec![metadata],
            broker.clone(),
            Limits {
                request_bytes: 1024,
                ..limits()
            },
            Default::default(),
        )
        .unwrap()
        .with_runtime_context();
        let mut large = source(stream);
        large["client_metadata"] = json!({"local_only":"large".repeat(300)});
        assert_eq!(
            deliver_request(&limited, lite(large))
                .await
                .err()
                .unwrap()
                .http_status,
            413
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn lite_single_call_checks_all_terminal_states_before_tools_history_and_releases_slot() {
    for stream in [false, true] {
        for state in ["completed", "failed", "incomplete"] {
            for first in [custom_call(), searching("s1")] {
                let mut native = terminal(vec![first, function("c2")]);
                native["status"] = state.into();
                if state == "failed" {
                    native["error"] = json!({"code":"fixture_failure","message":"safe"});
                }
                if state == "incomplete" {
                    native["incomplete_details"] = json!({"reason":"max_output_tokens"});
                }
                let mut fixture =
                    Fixture::start(vec![reply(native, stream), reply(finished(), stream)]).await;
                let (broker, reads) = broker();
                let enabled = provider(&fixture, broker);
                let mut wire = source(stream);
                wire["input"][0]["tools"]
                    .as_array_mut()
                    .unwrap()
                    .push(search());
                if stream {
                    let mut events = enabled
                        .stream_response(lite(wire.clone()), RequestContext::default())
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
                                assert_ne!(event.response.kind(), "response.output_item.done");
                                assert_ne!(
                                    event.response.kind(),
                                    "response.custom_tool_call_input.done"
                                );
                                assert_ne!(
                                    event.response.kind(),
                                    "response.function_call_arguments.done"
                                );
                                assert!(
                                    event.response.wire()["item"]
                                        .get("encrypted_content")
                                        .is_none()
                                );
                            }
                            _ => (),
                        }
                    }
                    assert!(failed);
                } else {
                    assert_eq!(
                        deliver_request(&enabled, lite(wire.clone()))
                            .await
                            .err()
                            .unwrap()
                            .code,
                        "ollama_invalid_native_tools"
                    );
                }
                fixture.request().await;
                deliver_request(&enabled, lite(wire)).await.unwrap();
                fixture.request().await;
                assert_eq!(reads.load(Ordering::SeqCst), 2);
            }
        }
    }
}

#[tokio::test]
async fn lite_parallel_default_or_true_keeps_actual_multiple_native_calls() {
    for stream in [false, true] {
        for flag in [None, Some(true)] {
            let mut fixture = Fixture::start(vec![reply(
                terminal(vec![custom_call(), function("c2")]),
                stream,
            )])
            .await;
            let (broker, reads) = broker();
            let enabled = provider(&fixture, broker);
            let mut wire = source(stream);
            wire.as_object_mut().unwrap().remove("parallel_tool_calls");
            if let Some(flag) = flag {
                wire["parallel_tool_calls"] = flag.into();
            }
            let response = deliver_request(&enabled, lite(wire)).await.unwrap();
            assert_eq!(response.output().len(), 3);
            assert_eq!(response.output()[1]["type"], "custom_tool_call");
            assert_eq!(response.output()[2]["type"], "function_call");
            let captured = fixture.request().await;
            assert!(captured.body.unwrap().get("parallel_tool_calls").is_none());
            assert_eq!(reads.load(Ordering::SeqCst), 1);
        }
    }
}

#[tokio::test]
async fn lite_history_binds_dialect_delivery_policy_and_original_declarations() {
    for stream in [false, true] {
        let mut fixture = Fixture::start(vec![reply(finished(), stream)]).await;
        let (broker, reads) = broker();
        let mut metadata = model("fixture", "native-fixture");
        metadata.dialects = vec![ResponsesDialect::Classic, ResponsesDialect::Lite];
        let enabled = OllamaProvider::with_lite_options(
            OllamaConfig::new(&fixture.base, Some(reference())).unwrap(),
            vec![metadata],
            broker,
            limits(),
            Default::default(),
        )
        .unwrap();
        let mut wire = source(stream);
        let response = deliver_request(&enabled, lite(wire.clone())).await.unwrap();
        fixture.request().await;
        wire["input"]
            .as_array_mut()
            .unwrap()
            .extend(response.output().iter().cloned());
        for flag in [None, Some(true)] {
            let mut altered = wire.clone();
            altered
                .as_object_mut()
                .unwrap()
                .remove("parallel_tool_calls");
            if let Some(flag) = flag {
                altered["parallel_tool_calls"] = flag.into();
            }
            assert_eq!(
                deliver_request(&enabled, lite(altered))
                    .await
                    .err()
                    .unwrap()
                    .code,
                "ollama_history_prefix_mismatch"
            );
        }
        let mut classic = wire.clone();
        classic["tools"] = classic["input"][0]["tools"].clone();
        classic["input"].as_array_mut().unwrap().remove(0);
        classic["parallel_tool_calls"] = true.into();
        assert_eq!(
            deliver_request(&enabled, request(classic))
                .await
                .err()
                .unwrap()
                .code,
            "ollama_history_prefix_mismatch"
        );
        let mut changed = wire.clone();
        changed["input"][0]["tools"][0]["tools"][1]["format"] =
            json!({"type":"grammar","syntax":"regex","definition":".*"});
        assert_eq!(
            deliver_request(&enabled, lite(changed))
                .await
                .err()
                .unwrap()
                .code,
            "ollama_history_prefix_mismatch"
        );
        let mut bad = wire;
        let capsule = bad["input"][2]["encrypted_content"]
            .as_str()
            .unwrap()
            .strip_prefix("caidex.ollama.native-history.v2:")
            .unwrap();
        let mut stored: Value = serde_json::from_str(capsule).unwrap();
        assert_eq!(stored["tool_mapping"]["lite_single_tool_call"], true);
        stored["tool_mapping"]["lite_single_tool_call"] = Value::Null;
        bad["input"][2]["encrypted_content"] =
            format!("caidex.ollama.native-history.v2:{stored}").into();
        assert_eq!(
            deliver_request(&enabled, lite(bad))
                .await
                .err()
                .unwrap()
                .code,
            "ollama_invalid_history"
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn lite_text_only_reuses_explicit_runtime_context_and_verbosity_without_tool_capability() {
    for stream in [false, true] {
        let mut fixture = Fixture::start(vec![reply(finished(), stream)]).await;
        let (broker, reads) = broker();
        let mut metadata = model("fixture", "native-fixture");
        metadata.dialects = vec![ResponsesDialect::Lite];
        metadata.capabilities.native_tools = caidex_model_core::CapabilitySupport::Unsupported;
        let enabled = OllamaProvider::with_lite_options(
            OllamaConfig::new(&fixture.base, Some(reference())).unwrap(),
            vec![metadata],
            broker,
            limits(),
            Default::default(),
        )
        .unwrap()
        .with_runtime_context()
        .with_verbosity_instruction("low".into(), "Executor concise guidance".into())
        .unwrap();
        let wire = json!({"model":"fixture","stream":stream,"parallel_tool_calls":false,"input":[{"role":"developer","content":"Original priority"},{"role":"user","content":"hello"}],"include":["reasoning.encrypted_content"],"reasoning":{"summary":"auto","context":"all_turns"},"text":{"verbosity":"low"},"prompt_cache_key":"local","client_metadata":{"session_id":"local"}});
        deliver_request(&enabled, lite(wire)).await.unwrap();
        let native = fixture.request().await.body.unwrap();
        assert_eq!(native["input"][0]["role"], "system");
        assert_eq!(native["instructions"], "Executor concise guidance");
        assert_eq!(native["tools"], json!([]));
        for key in [
            "include",
            "reasoning",
            "client_metadata",
            "prompt_cache_key",
            "parallel_tool_calls",
        ] {
            assert!(native.get(key).is_none());
        }
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
}
