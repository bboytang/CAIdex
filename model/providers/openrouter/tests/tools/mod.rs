use super::*;

fn function(name: &str) -> Value {
    json!({"type":"function","name":name,"description":"fixture tool","parameters":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]},"strict":false})
}
fn call(name: &str, call_id: &str) -> Value {
    json!({"type":"function_call","id":format!("item_{call_id}"),"status":"completed","name":name,"call_id":call_id,"arguments":"{\"text\":\"中文🙂\"}"})
}
fn wire(stream: bool) -> Value {
    json!({"model":"fixture","input":[],"stream":stream,"tools":[function("exec"),function("inspect")],"tool_choice":"auto","parallel_tool_calls":true})
}
fn response(calls: Vec<Value>) -> Value {
    let mut native = response_wire();
    native["output"][2]["id"] = json!(18446744073709551616_u128);
    native["output"]
        .as_array_mut()
        .unwrap()
        .extend(calls.into_iter().map(|mut item| {
            item["future"] = json!({"opaque":18446744073709551616_u128});
            item
        }));
    native["openrouter_metadata"] = json!({"requested":"native-fixture","attempt":1,"endpoints":{"available":[{"provider":"Fixture","selected":true}]}});
    native
}
fn provider(
    fixture: &Fixture,
    broker: Arc<Broker<Store>>,
    limits: Limits,
) -> OpenRouterProvider<Store> {
    fixture
        .provider(broker, limits)
        .with_backend_selection("fixture".into(), "fixture-backend/region".into())
        .unwrap()
        .with_native_tools("fixture".into())
        .unwrap()
}
fn canonical(wire: Value) -> CanonicalRequest {
    CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap()
}
fn sse(events: &[Value]) -> String {
    events.iter().map(|v| format!("data: {v}\n\n")).collect()
}
fn chunks(native: &Value) -> Vec<Value> {
    let mut events =
        vec![json!({"type":"response.created","response":{"id":"fixture","output":[]}})];
    let calls: Vec<_> = native["output"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(_, i)| i["type"] == "function_call")
        .collect();
    for (index, item) in &calls {
        let mut added = (*item).clone();
        added["status"] = "in_progress".into();
        added["arguments"] = "".into();
        events.push(json!({"type":"response.output_item.added","output_index":index,"item":added}));
    }
    for (index, item) in &calls {
        events.push(json!({"type":"response.function_call_arguments.delta","output_index":index,"item_id":item["id"],"delta":"{\"text\":"}));
    }
    for (index, item) in calls.iter().rev() {
        events.push(json!({"type":"response.function_call_arguments.delta","output_index":index,"item_id":item["id"],"delta":"\"中文🙂\"}"}));
        events.push(json!({"type":"response.function_call_arguments.done","output_index":index,"item_id":item["id"],"arguments":item["arguments"]}));
        events.push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
    }
    events.push(json!({"type":"response.completed","response":native}));
    events
}

#[tokio::test]
async fn function_json_preserves_native_calls_controls_and_route_identity() {
    let native = response(vec![call("exec", "a"), call("inspect", "b")]);
    let mut fixture = Fixture::start(vec![Reply::json(native.clone())]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = provider(&fixture, broker, limits())
        .with_runtime_context()
        .with_reasoning_effort_mapping("fixture".into(), "xhigh".into(), "high".into())
        .unwrap()
        .with_verbosity_instruction("fixture".into(), "low".into(), "Concise".into())
        .unwrap()
        .with_service_tier_mapping("fixture".into(), "priority".into(), "fast".into())
        .unwrap();
    let mut source = runtime_wire();
    source["tools"] = wire(false)["tools"].clone();
    source["tool_choice"] = "required".into();
    source["parallel_tool_calls"] = true.into();
    source["reasoning"] = json!({"effort":"xhigh"});
    source["text"]["verbosity"] = "low".into();
    source["service_tier"] = "priority".into();
    source["instructions"] = "original".into();
    let result = provider
        .create_response(canonical(source.clone()), runtime_context())
        .await
        .unwrap();
    let captured = fixture.request().await;
    let sent = captured.body.unwrap();
    assert_eq!(sent["tools"], source["tools"]);
    assert_eq!(sent["tool_choice"], "required");
    assert_eq!(sent["parallel_tool_calls"], true);
    assert_eq!(
        sent["provider"],
        json!({"only":["fixture-backend/region"],"require_parameters":true,"allow_fallbacks":false})
    );
    assert_eq!(sent["model"], "native-fixture");
    assert_eq!(sent["instructions"], "original\nConcise");
    assert_eq!(sent["reasoning"], json!({"effort":"high"}));
    assert_eq!(sent["service_tier"], "fast");
    assert!(!captured.headers.contains("executor-private"));
    assert_eq!(result.response.wire(), &native);
    assert_eq!(
        provider.capabilities("fixture").unwrap().native_tools,
        CapabilitySupport::Unknown
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn grouped_function_results_replay_without_reordering_or_losing_text() {
    let first = response(vec![call("exec", "a"), call("inspect", "b")]);
    let second = response(vec![]);
    let mut fixture = Fixture::start(vec![
        Reply::json(first.clone()),
        Reply::json(second.clone()),
    ])
    .await;
    let (broker, _) = super::broker(Some(KEY));
    let provider = provider(&fixture, broker, limits());
    let delivered = provider
        .create_response(canonical(wire(false)), RequestContext::default())
        .await
        .unwrap();
    assert_eq!(delivered.response.wire(), &first);
    fixture.request().await;
    let mut next = wire(false);
    next["tool_choice"] = "none".into();
    next["input"] = json!([call("exec","a"),call("inspect","b"),{"type":"function_call_output","call_id":"b","output":[{"type":"input_text","text":"中文🙂"}]},{"type":"function_call_output","id":"result_a","status":"completed","call_id":"a","output":"ok"},{"role":"user","content":"next"}]);
    let result = provider
        .create_response(canonical(next.clone()), RequestContext::default())
        .await
        .unwrap();
    assert_eq!(
        fixture.request().await.body.unwrap()["input"],
        next["input"]
    );
    assert_eq!(result.response.wire(), &second);
}

#[tokio::test]
async fn selection_modes_and_named_choice_are_forwarded_once_and_enforced() {
    for (choice, calls) in [
        (Value::Null, vec![]),
        (json!("auto"), vec![]),
        (json!("none"), vec![]),
        (json!("required"), vec![call("inspect", "a")]),
        (
            json!({"type":"function","name":"exec"}),
            vec![call("exec", "a")],
        ),
    ] {
        let mut fixture = Fixture::start(vec![Reply::json(response(calls))]).await;
        let (broker, _) = super::broker(Some(KEY));
        let provider = provider(&fixture, broker, limits());
        let mut source = wire(false);
        source["tool_choice"] = choice.clone();
        provider
            .create_response(canonical(source.clone()), RequestContext::default())
            .await
            .unwrap();
        let sent = fixture.request().await.body.unwrap();
        assert_eq!(sent["tools"], source["tools"]);
        assert_eq!(sent["tool_choice"], choice);
    }
}

#[tokio::test]
async fn bad_declarations_choices_and_advanced_modes_fail_before_credentials() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = provider(&fixture, broker, limits());
    let mut cases = Vec::new();
    for tools in [
        json!({}),
        json!([function("exec"), function("exec")]),
        json!([function("bad name")]),
        json!([{"type":"custom","name":"exec"}]),
        json!([{"type":"namespace","name":"ns","tools":[function("exec")]}]),
        json!([{"type":"web_search"}]),
    ] {
        let mut v = wire(false);
        v["tools"] = tools;
        cases.push(v);
    }
    for (key, value) in [
        ("strict", json!(true)),
        ("defer_loading", json!(true)),
        ("description", json!(42)),
        ("parameters", json!([])),
        ("unknown", json!(true)),
    ] {
        let mut v = wire(false);
        v["tools"][0][key] = value;
        cases.push(v);
    }
    for choice in [
        json!("bad"),
        json!({"type":"function","name":"missing"}),
        json!({"type":"function","name":"exec","namespace":"ns"}),
        json!({"type":"allowed_tools","mode":"auto","tools":[]}),
        json!(false),
    ] {
        let mut v = wire(false);
        v["tool_choice"] = choice;
        cases.push(v);
    }
    let mut empty = wire(false);
    empty["tools"] = json!([]);
    empty["tool_choice"] = "required".into();
    cases.push(empty);
    let mut bad_parallel = wire(false);
    bad_parallel["parallel_tool_calls"] = "false".into();
    cases.push(bad_parallel);
    for v in cases {
        assert!(
            provider
                .create_response(canonical(v), RequestContext::default())
                .await
                .is_err()
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unpaired_reused_and_undeclared_input_calls_fail_before_credentials() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = broker(Some(KEY));
    let provider = provider(&fixture, broker, limits());
    let result = json!({"type":"function_call_output","call_id":"a","output":"ok"});
    let mut bad_args = call("exec", "a");
    bad_args["arguments"] = "[]".into();
    let mut bad_result = result.clone();
    bad_result["output"] = json!([{"type":"input_image","image_url":"https://example.test/x"}]);
    for input in [
        json!([result]),
        json!([call("exec", "a")]),
        json!([call("exec", "a"), result, result]),
        json!([call("exec", "a"), result, call("exec", "a"), result]),
        json!([call("missing", "a"), result]),
        json!([bad_args, result]),
        json!([call("exec", "a"), bad_result]),
        json!([call("exec","a"),{"role":"user","content":"cross pending"},result]),
        json!([{"type":"reasoning","encrypted_content":"foreign"}]),
    ] {
        let mut v = wire(false);
        v["input"] = input;
        assert!(
            provider
                .create_response(canonical(v), RequestContext::default())
                .await
                .is_err()
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn tool_configuration_default_route_and_capability_gates_stay_closed() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = broker(Some(KEY));
    assert!(
        fixture
            .provider(broker.clone(), limits())
            .with_native_tools("fixture".into())
            .is_err()
    );
    assert!(
        fixture
            .provider(broker.clone(), limits())
            .with_native_tools("missing".into())
            .is_err()
    );
    assert!(
        provider(&fixture, broker.clone(), limits())
            .with_native_tools("fixture".into())
            .is_err()
    );
    for configured in [false, true] {
        let p = if configured {
            provider(&fixture, broker.clone(), limits())
        } else {
            fixture.provider(broker.clone(), limits())
        };
        let mut v = wire(false);
        if configured {
            v["model"] = "not-visible".into();
        }
        assert!(
            p.create_response(canonical(v), RequestContext::default())
                .await
                .is_err()
        );
    }
    for (tools, parallel) in [
        (CapabilitySupport::Unsupported, CapabilitySupport::Unknown),
        (CapabilitySupport::Unknown, CapabilitySupport::Unsupported),
    ] {
        let mut metadata = model("fixture", "native-fixture");
        metadata.capabilities.native_tools = tools;
        metadata.capabilities.parallel_tools = parallel;
        let p = OpenRouterProvider::new(
            OpenRouterConfig::new(reference())
                .unwrap()
                .with_base_url(&fixture.base)
                .unwrap(),
            vec![metadata],
            broker.clone(),
            limits(),
        )
        .unwrap()
        .with_backend_selection("fixture".into(), "fixture-backend".into())
        .unwrap()
        .with_native_tools("fixture".into())
        .unwrap();
        assert!(
            p.create_response(canonical(wire(false)), RequestContext::default())
                .await
                .is_err()
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn invalid_native_calls_never_deliver_json_success() {
    let mut cases = Vec::new();
    for (key, value) in [
        ("name", json!("undeclared")),
        ("arguments", json!("[]")),
        ("arguments", json!("{bad")),
        ("call_id", json!("\n")),
        ("id", json!("")),
        ("namespace", json!("ns")),
        ("status", json!("in_progress")),
    ] {
        let mut c = call("exec", "a");
        c[key] = value;
        cases.push((wire(false), response(vec![c])));
    }
    cases.push((
        wire(false),
        response(vec![call("exec", "a"), call("inspect", "a")]),
    ));
    for choice in [json!("none"), json!({"type":"function","name":"inspect"})] {
        let mut v = wire(false);
        v["tool_choice"] = choice;
        cases.push((v, response(vec![call("exec", "a")])));
    }
    let mut single = wire(false);
    single["parallel_tool_calls"] = false.into();
    cases.push((
        single,
        response(vec![call("exec", "a"), call("inspect", "b")]),
    ));
    let mut required = wire(false);
    required["tool_choice"] = "required".into();
    cases.push((required, response(vec![])));
    for status in ["failed", "incomplete"] {
        let mut native = response(vec![call("exec", "a")]);
        native["status"] = status.into();
        cases.push((wire(false), native));
    }
    let mut reused = wire(false);
    reused["input"] =
        json!([call("exec","a"),{"type":"function_call_output","call_id":"a","output":"ok"}]);
    cases.push((reused.clone(), response(vec![call("exec", "a")])));
    let mut reused_item = call("exec", "b");
    reused_item["id"] = "item_a".into();
    cases.push((reused, response(vec![reused_item])));
    for kind in ["custom_tool_call", "mcp_call", "tool_search_call"] {
        let item = if kind == "custom_tool_call" {
            json!({"type":kind,"name":"exec","call_id":"a","input":"bad"})
        } else if kind == "tool_search_call" {
            json!({"type":kind,"execution":"client","call_id":"a","arguments":{}})
        } else {
            json!({"type":kind})
        };
        cases.push((wire(false), response(vec![item])));
    }
    for (source, native) in cases {
        let fixture = Fixture::start(vec![Reply::json(native)]).await;
        let (broker, _) = super::broker(Some(KEY));
        let p = provider(&fixture, broker, limits());
        let error = p
            .create_response(canonical(source), RequestContext::default())
            .await
            .err()
            .unwrap();
        assert_eq!(error.http_status, 502);
        assert!(!format!("{error:?}").contains(KEY));
    }
}

#[tokio::test]
async fn interleaved_sse_calls_match_terminal_and_preserve_every_native_event() {
    let native = response(vec![call("exec", "a"), call("inspect", "b")]);
    let events = chunks(&native);
    let mut fixture = Fixture::start(vec![Reply::stream(sse(&events))]).await;
    let (broker, _) = super::broker(Some(KEY));
    let p = provider(&fixture, broker, limits());
    let mut stream = p
        .stream_response(canonical(wire(true)), RequestContext::default())
        .await
        .unwrap()
        .events;
    let mut actual = Vec::new();
    while let Some(event) = stream.next().await {
        if let ProviderStreamEvent::Model(model) = event.unwrap() {
            actual.push(model.response.wire().clone());
        }
    }
    assert_eq!(actual, events);
    assert_eq!(
        fixture.request().await.body.unwrap()["tools"],
        wire(true)["tools"]
    );
}

#[tokio::test]
async fn corrupt_sse_calls_are_rejected_before_delivery_and_release_connection() {
    let native = response(vec![call("exec", "a")]);
    let good = chunks(&native);
    let mut cases = Vec::new();
    for (index, key, value) in [
        (2, "item_id", json!("wrong")),
        (3, "delta", json!("bad")),
        (4, "arguments", json!("{}")),
        (5, "output_index", json!(99)),
    ] {
        let mut v = good.clone();
        v[index][key] = value;
        cases.push(v);
    }
    let mut missing = good.clone();
    missing.remove(5);
    cases.push(missing);
    let mut changed = good.clone();
    changed[6]["response"]["output"][3]["name"] = "inspect".into();
    cases.push(changed);
    let mut duplicate = good.clone();
    duplicate.insert(5, duplicate[4].clone());
    cases.push(duplicate);
    let mut early = good.clone();
    early[0]["response"]["output"] = json!([call("exec", "early")]);
    cases.push(early);
    let mut incomplete = good.clone();
    incomplete[6]["type"] = "response.incomplete".into();
    incomplete[6]["response"]["status"] = "incomplete".into();
    cases.push(incomplete);
    for events in cases {
        let mut fixture =
            Fixture::start(vec![Reply::stream(sse(&events)), Reply::stream(sse(&good))]).await;
        let (broker, _) = super::broker(Some(KEY));
        let mut configured = limits();
        configured.in_flight = 1;
        let p = provider(&fixture, broker, configured);
        let mut stream = p
            .stream_response(canonical(wire(true)), RequestContext::default())
            .await
            .unwrap()
            .events;
        assert!(stream.next().await.unwrap().is_err());
        assert!(stream.next().await.is_none());
        fixture.request().await;
        fixture.disconnected().await;
        let mut retry = p
            .stream_response(canonical(wire(true)), RequestContext::default())
            .await
            .unwrap()
            .events;
        while let Some(event) = retry.next().await {
            event.unwrap();
        }
        fixture.request().await;
    }
}

#[tokio::test]
async fn buffered_tools_cancel_deadline_and_drop_close_sockets_and_release_slot() {
    for mode in ["cancel", "deadline", "drop"] {
        let native = response(vec![call("exec", "a")]);
        let good = chunks(&native);
        let mut fixture = Fixture::start(vec![
            Reply {
                stall: 2,
                ..Reply::stream(sse(&good[..2]))
            },
            Reply::stream(sse(&good)),
        ])
        .await;
        let (broker, _) = super::broker(Some(KEY));
        let mut configured = limits();
        configured.in_flight = 1;
        let p = provider(&fixture, broker, configured);
        let token = CancellationToken::default();
        let mut context = RequestContext {
            cancellation: token.clone(),
            ..RequestContext::default()
        };
        if mode == "deadline" {
            context.deadline = Some(std::time::Instant::now() + Duration::from_millis(200));
        }
        let mut stream = p
            .stream_response(canonical(wire(true)), context)
            .await
            .unwrap()
            .events;
        fixture.request().await;
        if mode == "drop" {
            drop(stream);
        } else {
            let pending = tokio::spawn(async move {
                let error = stream.next().await.unwrap().err().unwrap();
                assert!(stream.next().await.is_none());
                error
            });
            if mode == "cancel" {
                tokio::task::yield_now().await;
                token.cancel();
            }
            let error = tokio::time::timeout(WAIT, pending).await.unwrap().unwrap();
            assert_eq!(error.http_status, if mode == "cancel" { 503 } else { 504 });
        }
        fixture.disconnected().await;
        let mut next = p
            .stream_response(canonical(wire(true)), RequestContext::default())
            .await
            .unwrap()
            .events;
        while let Some(event) = next.next().await {
            event.unwrap();
        }
        fixture.request().await;
    }
}

#[tokio::test]
async fn cancelled_queue_and_single_call_policy_never_deliver_pending_tools() {
    let native = response(vec![call("exec", "a")]);
    let good = chunks(&native);
    let fixture = Fixture::start(vec![Reply::stream(sse(&good))]).await;
    let (broker, _) = super::broker(Some(KEY));
    let p = provider(&fixture, broker, limits());
    let token = CancellationToken::default();
    let context = RequestContext {
        cancellation: token.clone(),
        ..RequestContext::default()
    };
    let mut stream = p
        .stream_response(canonical(wire(true)), context)
        .await
        .unwrap()
        .events;
    assert!(matches!(
        stream.next().await.unwrap().unwrap(),
        ProviderStreamEvent::Model(_)
    ));
    token.cancel();
    assert_eq!(stream.next().await.unwrap().err().unwrap().http_status, 503);
    assert!(stream.next().await.is_none());
    let native = response(vec![call("exec", "a"), call("inspect", "b")]);
    let fixture = Fixture::start(vec![Reply::stream(sse(&chunks(&native)))]).await;
    let (broker, _) = super::broker(Some(KEY));
    let p = provider(&fixture, broker, limits());
    let mut source = wire(true);
    source["parallel_tool_calls"] = false.into();
    let mut stream = p
        .stream_response(canonical(source), RequestContext::default())
        .await
        .unwrap()
        .events;
    assert!(stream.next().await.unwrap().is_err());
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn tool_source_and_compiled_budgets_precede_credentials() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = broker(Some(KEY));
    let mut configured = limits();
    configured.request_bytes = 1024;
    let p = provider(&fixture, broker, configured)
        .with_verbosity_instruction("fixture".into(), "low".into(), "g".repeat(1025))
        .unwrap();
    let mut source = wire(false);
    source["tools"][0]["description"] = "d".repeat(1025).into();
    let mut compiled = wire(false);
    compiled["text"] = json!({"verbosity":"low"});
    for v in [source, compiled] {
        assert_eq!(
            p.create_response(canonical(v), RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            "invalid_or_oversized_body"
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn done_only_and_terminal_only_streams_preserve_valid_native_calls() {
    let native = response(vec![call("exec", "a")]);
    let all = chunks(&native);
    for events in [
        vec![
            all[0].clone(),
            all[1].clone(),
            all[5].clone(),
            all[6].clone(),
        ],
        vec![all[0].clone(), all[6].clone()],
    ] {
        let fixture = Fixture::start(vec![Reply::stream(sse(&events))]).await;
        let (broker, _) = super::broker(Some(KEY));
        let p = provider(&fixture, broker, limits());
        let mut stream = p
            .stream_response(canonical(wire(true)), RequestContext::default())
            .await
            .unwrap()
            .events;
        let mut actual = Vec::new();
        while let Some(event) = stream.next().await {
            if let ProviderStreamEvent::Model(model) = event.unwrap() {
                actual.push(model.response.wire().clone());
            }
        }
        assert_eq!(actual, events);
    }
}

#[tokio::test]
async fn aggregate_tool_stream_budget_rejects_before_delivery_and_releases_slot() {
    let native = response(vec![call("exec", "a")]);
    let mut events = chunks(&native);
    for _ in 0..12 {
        events.insert(
            1,
            json!({"type":"response.future_extension","text":"x".repeat(256)}),
        );
    }
    let mut fixture = Fixture::start(vec![
        Reply::stream(sse(&events)),
        Reply::stream(sse(&chunks(&native))),
    ])
    .await;
    let (broker, _) = super::broker(Some(KEY));
    let mut configured = limits();
    configured.response_bytes = 2048;
    configured.in_flight = 1;
    let p = provider(&fixture, broker, configured);
    let mut stream = p
        .stream_response(canonical(wire(true)), RequestContext::default())
        .await
        .unwrap()
        .events;
    assert_eq!(
        stream.next().await.unwrap().err().unwrap().code,
        "openrouter_tool_stream_too_large"
    );
    assert!(stream.next().await.is_none());
    fixture.request().await;
    fixture.disconnected().await;
    let mut next = p
        .stream_response(canonical(wire(true)), RequestContext::default())
        .await
        .unwrap()
        .events;
    while let Some(event) = next.next().await {
        event.unwrap();
    }
    fixture.request().await;
}

#[tokio::test]
async fn gateway_delivers_native_function_data_without_executing_or_forwarding_identity() {
    let native = response(vec![call("exec", "a")]);
    let mut fixture = Fixture::start(vec![Reply::json(native.clone())]).await;
    let (broker, reads) = super::broker(Some(KEY));
    let p = Arc::new(provider(&fixture, broker.clone(), limits()).with_runtime_context());
    let gateway = caidex_model_gateway::start_with_provider(p, broker.redactor(), limits())
        .await
        .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let url = format!("http://{}/v1/responses", gateway.address());
    let result = client
        .post(url)
        .bearer_auth(gateway.token().expose())
        .header("x-client-request-id", "executor-private")
        .header("content-type", "application/json")
        .body(wire(false).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(result.status(), 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&result.bytes().await.unwrap()).unwrap(),
        native
    );
    let captured = fixture.request().await;
    assert_eq!(
        captured.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert!(!captured.headers.contains("executor-private"));
    assert!(!captured.headers.contains(gateway.token().expose()));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    gateway.shutdown().await.unwrap();
}
