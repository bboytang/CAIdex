use super::*;
use caidex_provider_ollama::ModelDetails;

fn enabled(fixture: &Fixture, broker: Arc<Broker<Store>>) -> OllamaProvider<Store> {
    fixture
        .provider(broker, true)
        .with_runtime_context()
        .with_native_history()
        .with_verbosity_instruction("low".into(), "Fixture concise guidance".into())
        .unwrap()
        .with_model_details(vec![(
            "fixture".into(),
            ModelDetails::parse(
                "native-fixture".into(),
                json!({"thinking":{"values":[false,"low","medium","high"],"default":"medium"}}),
            )
            .unwrap(),
        )])
        .unwrap()
}
fn wire() -> Value {
    json!({"model":"fixture","instructions":"Original instruction","input":[{"type":"message","id":"dev","role":"developer","content":[{"type":"input_text","text":"Priority guidance"}]},{"type":"message","id":"user","role":"user","content":"hello"}],
        "tools":[{"type":"function","name":"echo","parameters":{"type":"object"},"strict":false}],
        "include":["reasoning.encrypted_content"],"prompt_cache_key":"local-cache","client_metadata":{"session_id":"local-session","opaque":"local attribution"},
        "reasoning":{"effort":"medium","summary":"auto","context":"all_turns"},"text":{"verbosity":"low"},"store":false})
}
fn headers() -> ContextHeaders {
    let mut headers = ContextHeaders::default();
    for (name, value) in [
        ("session_id", "local-session"),
        ("x-client-request-id", "local-request"),
        ("x-codex-turn-metadata", "local-turn"),
    ] {
        headers.insert(name, value.into(), REQUEST_HEADERS).unwrap();
    }
    headers
}

fn context() -> RequestContext {
    RequestContext {
        headers: headers(),
        ..Default::default()
    }
}
fn response() -> Value {
    let mut native = response_wire();
    native["output"].as_array_mut().unwrap().insert(0,json!({"type":"reasoning","id":"rs_1","summary":[{"type":"summary_text","text":"native thought"}],"encrypted_content":"native thought"}));
    native
}

#[tokio::test]
async fn runtime_parameters_compile_native_intent_and_do_not_forward_local_attribution() {
    for stream in [false, true] {
        let reply = if stream {
            Reply::stream(format!(
                "{CREATED}event: response.completed\ndata: {}\n\n",
                json!({"type":"response.completed","sequence_number":1,"response":response()})
            ))
        } else {
            Reply::json(response())
        };
        let mut fixture = Fixture::start(vec![reply]).await;
        let (broker, reads) = broker();
        let provider = enabled(&fixture, broker);
        let mut source = wire();
        source["stream"] = stream.into();
        if stream {
            let mut events = provider
                .stream_response(request(source.clone()), context())
                .await
                .unwrap()
                .events;
            while let Some(event) = events.next().await {
                event.unwrap();
            }
        } else {
            provider
                .create_response(request(source.clone()), context())
                .await
                .unwrap();
        }
        let captured = fixture.request().await;
        for key in ["session_id", "x-client-request-id", "x-codex-turn-metadata"] {
            assert!(captured.header(key).is_none());
        }
        assert_eq!(
            captured.header("Authorization"),
            Some(format!("Bearer {KEY}").as_str())
        );
        let body = captured.body.unwrap();
        assert_eq!(body["model"], "native-fixture");
        assert_eq!(body["input"][0]["role"], "system");
        assert_eq!(body["input"][0]["content"], source["input"][0]["content"]);
        assert_eq!(
            body["instructions"],
            "Original instruction\nFixture concise guidance"
        );
        assert_eq!(body["think"], "medium");
        assert_eq!(body["tools"], source["tools"]);
        for key in [
            "include",
            "prompt_cache_key",
            "client_metadata",
            "reasoning",
            "text",
        ] {
            assert!(body.get(key).is_none(), "{key}");
        }
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn runtime_compiled_prefix_replays_history_and_rejects_changed_mapping_or_developer_order() {
    let mut fixture = Fixture::start(vec![Reply::json(response())]).await;
    let (broker, reads) = broker();
    let provider = enabled(&fixture, broker);
    let source = wire();
    let first = provider
        .create_response(request(source.clone()), context())
        .await
        .unwrap()
        .response;
    let first_body = fixture.request().await.body.unwrap();
    let mut input = source["input"].as_array().unwrap().clone();
    input.extend(first.output().iter().cloned());
    input.push(json!({"type":"function_call_output","call_id":"c1","output":"result"}));
    input.push(json!({"role":"user","content":"next"}));
    let mut next = source.clone();
    next["input"] = json!(input);
    next["prompt_cache_key"] = "new-local-attribution".into();
    provider
        .create_response(request(next.clone()), context())
        .await
        .unwrap();
    let native = fixture.request().await.body.unwrap();
    assert_eq!(
        &native["input"].as_array().unwrap()[..2],
        first_body["input"].as_array().unwrap()
    );
    assert_eq!(
        &native["input"].as_array().unwrap()[2..4],
        response()["output"].as_array().unwrap()
    );
    assert_eq!(native["instructions"], first_body["instructions"]);
    let provider = provider
        .with_verbosity_instruction("medium".into(), "Changed policy".into())
        .unwrap();
    let mut changed = next.clone();
    changed["text"]["verbosity"] = "medium".into();
    assert_eq!(
        provider
            .create_response(request(changed), context())
            .await
            .err()
            .unwrap()
            .code,
        "ollama_history_prefix_mismatch"
    );
    next["instructions"] = "Changed binding".into();
    assert_eq!(
        provider
            .create_response(request(next), context())
            .await
            .err()
            .unwrap()
            .code,
        "ollama_history_prefix_mismatch"
    );
    let mut wrong = source;
    wrong["input"].as_array_mut().unwrap().reverse();
    assert_eq!(
        provider
            .create_response(request(wrong), context())
            .await
            .err()
            .unwrap()
            .http_status,
        400
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn runtime_defaults_and_unmapped_intent_fail_before_credentials() {
    let fixture = Fixture::start(vec![Reply::json(response())]).await;
    let (broker, reads) = broker();
    let plain = fixture.provider(broker.clone(), true);
    let provider = enabled(&fixture, broker);
    assert_eq!(
        plain
            .create_response(request(wire()), context())
            .await
            .err()
            .unwrap()
            .code,
        "ollama_unsupported_context_headers"
    );
    assert_eq!(
        plain
            .create_response(request(wire()), RequestContext::default())
            .await
            .err()
            .unwrap()
            .http_status,
        400
    );
    let mut cases = Vec::new();
    for (key, value) in [
        ("include", json!(["message.output_text.logprobs"])),
        (
            "include",
            json!(["reasoning.encrypted_content", "reasoning.encrypted_content"]),
        ),
        ("include", json!([null])),
        ("client_metadata", json!({"bad":1})),
        ("prompt_cache_key", json!("")),
        ("text", json!({"verbosity":"high"})),
        ("reasoning", json!({"effort":"medium","summary":"detailed"})),
        (
            "reasoning",
            json!({"effort":"medium","context":"current_turn"}),
        ),
        ("reasoning", json!({"effort":"xhigh"})),
        ("reasoning", json!({"effort":"medium","future":true})),
    ] {
        let mut source = wire();
        source[key] = value;
        cases.push(source);
    }
    for source in cases {
        for stream in [false, true] {
            let mut source = source.clone();
            source["stream"] = stream.into();
            let error = if stream {
                provider
                    .stream_response(request(source), context())
                    .await
                    .err()
                    .unwrap()
            } else {
                provider
                    .create_response(request(source), context())
                    .await
                    .err()
                    .unwrap()
            };
            assert_eq!(error.http_status, 400);
            assert!(!format!("{error:?}").contains(KEY));
        }
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn local_context_applies_to_metadata_and_turn_state_cancel_and_deadline_remain_refused() {
    let mut fixture = Fixture::start(vec![
        Reply::json(catalog()),
        Reply::json(json!({"thinking":{"values":[false,"medium"],"default":"medium"}})),
    ])
    .await;
    let (broker, reads) = broker();
    let provider = OllamaProvider::new(
        OllamaConfig::new(&fixture.base, Some(reference()))
            .unwrap()
            .with_show_endpoint(&format!("{}/show", fixture.base))
            .unwrap(),
        vec![model("fixture", "native-fixture")],
        broker,
        limits(),
    )
    .unwrap()
    .with_runtime_context();
    assert_eq!(provider.discover_models(context()).await.unwrap().len(), 2);
    provider.show_model("fixture", context()).await.unwrap();
    for _ in 0..2 {
        let captured = fixture.request().await;
        for name in ["session_id", "x-client-request-id", "x-codex-turn-metadata"] {
            assert!(captured.header(name).is_none());
        }
        assert_eq!(
            captured.header("authorization"),
            Some(format!("Bearer {KEY}").as_str())
        );
    }
    let mut state = context();
    state
        .headers
        .insert(
            "x-codex-turn-state",
            "unsupported-turn-state".into(),
            REQUEST_HEADERS,
        )
        .unwrap();
    assert_eq!(
        provider.discover_models(state).await.err().unwrap().code,
        "ollama_unsupported_context_headers"
    );
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert_eq!(
        provider
            .show_model(
                "fixture",
                RequestContext {
                    cancellation: cancelled,
                    headers: headers(),
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
        provider
            .create_response(
                request(json!({"model":"fixture","input":"hello"})),
                RequestContext {
                    deadline: Some(std::time::Instant::now()),
                    headers: headers(),
                    ..Default::default()
                }
            )
            .await
            .err()
            .unwrap()
            .code,
        "provider_timeout"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn original_metadata_and_expanded_guidance_budgets_and_history_opt_in_bound_intent() {
    let fixture = Fixture::start(vec![Reply::json(response())]).await;
    let (broker, reads) = broker();
    let provider = OllamaProvider::new(
        OllamaConfig::new(&fixture.base, Some(reference())).unwrap(),
        vec![model("fixture", "native-fixture")],
        broker.clone(),
        Limits {
            request_bytes: 512,
            ..limits()
        },
    )
    .unwrap()
    .with_runtime_context()
    .with_verbosity_instruction("low".into(), "guide".repeat(256))
    .unwrap();
    for source in [
        json!({"model":"fixture","input":"hello","client_metadata":{"big":"x".repeat(2048)}}),
        json!({"model":"fixture","input":"hello","text":{"verbosity":"low"}}),
    ] {
        for stream in [false, true] {
            let mut source = source.clone();
            source["stream"] = stream.into();
            let error = if stream {
                provider
                    .stream_response(request(source), context())
                    .await
                    .err()
                    .unwrap()
            } else {
                provider
                    .create_response(request(source), context())
                    .await
                    .err()
                    .unwrap()
            };
            assert_eq!(error.http_status, 413);
            assert_eq!(error.code, "invalid_or_oversized_body");
        }
    }
    let no_history = fixture
        .provider(broker.clone(), true)
        .with_runtime_context();
    for extra in [
        json!({"include":["reasoning.encrypted_content"]}),
        json!({"reasoning":{"summary":"auto"}}),
        json!({"reasoning":{"context":"all_turns"}}),
    ] {
        let mut source = json!({"model":"fixture","input":"hello"});
        source
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert_eq!(
            no_history
                .create_response(request(source), context())
                .await
                .err()
                .unwrap()
                .code,
            "ollama_unsupported_runtime_parameter"
        );
    }
    for (name, guidance) in [("unknown", "guide"), ("low", " ")] {
        assert_eq!(
            fixture
                .provider(broker.clone(), true)
                .with_verbosity_instruction(name.into(), guidance.into())
                .err()
                .unwrap()
                .code,
            "ollama_invalid_verbosity_mapping"
        );
    }
    assert!(
        fixture
            .provider(broker.clone(), true)
            .with_verbosity_instruction("low".into(), "first".into())
            .unwrap()
            .with_verbosity_instruction("low".into(), "second".into())
            .is_err()
    );
    let mut restricted = model("fixture", "native-fixture");
    restricted.capabilities.reasoning = caidex_model_core::CapabilitySupport::Unsupported;
    let restricted = OllamaProvider::new(
        OllamaConfig::new(&fixture.base, Some(reference())).unwrap(),
        vec![restricted],
        broker,
        limits(),
    )
    .unwrap()
    .with_runtime_context()
    .with_native_history();
    assert_eq!(
        restricted
            .create_response(
                request(json!({"model":"fixture","input":"hello","reasoning":{"summary":"auto"}})),
                context()
            )
            .await
            .err()
            .unwrap()
            .code,
        "ollama_unsupported_capability"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn runtime_verbosity_and_native_schema_preserve_strict_delivery_and_optional_defaults() {
    for valid in [false, true] {
        let native = json!({"id":"fixture","status":"completed","output":[{"type":"message","id":"msg_1","status":"completed","role":"assistant","content":[{"type":"output_text","text":if valid {"{\"ok\":true}"} else {"{\"ok\":1}"}}]}]});
        let mut fixture = Fixture::start(vec![Reply::json(native.clone())]).await;
        let (broker, _) = broker();
        let provider = enabled(&fixture, broker).with_structured_output();
        let mut source = wire();
        source["text"] = json!({"verbosity":"low","format":{"type":"json_schema","name":"fixture","strict":true,"schema":{"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"],"additionalProperties":false}}});
        let result = provider
            .create_response(request(source.clone()), context())
            .await;
        if valid {
            assert_eq!(
                result.unwrap().response.output().last().unwrap(),
                &native["output"][0]
            );
        } else {
            assert_eq!(
                result.err().unwrap().code,
                "ollama_invalid_structured_output"
            );
        }
        let captured = fixture.request().await.body.unwrap();
        assert_eq!(captured["text"]["format"], source["text"]["format"]);
        assert!(captured["text"].get("verbosity").is_none());
    }
    let mut fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, _) = broker();
    let provider = fixture.provider(broker, true).with_runtime_context();
    provider.create_response(request(json!({"model":"fixture","input":"hello","include":[],"prompt_cache_key":null,"client_metadata":null,"reasoning":{"summary":null,"context":null},"text":{"verbosity":null}})),context()).await.unwrap();
    let captured = fixture.request().await.body.unwrap();
    for key in [
        "include",
        "prompt_cache_key",
        "client_metadata",
        "reasoning",
        "text",
    ] {
        assert!(captured.get(key).is_none());
    }
}
