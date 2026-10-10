use super::*;

fn custom(name: &str) -> Value {
    json!({"type":"custom","name":name,"description":"native freeform","format":{"type":"grammar","syntax":"lark","definition":"start: /.+/"}})
}
fn namespace(ns: &str, tools: Vec<Value>) -> Value {
    json!({"type":"namespace","name":ns,"description":"original namespace guidance","tools":tools})
}
fn advanced_wire(stream: bool) -> Value {
    let mut v = wire(stream);
    v["tools"] = json!([
        function("exec"),
        custom("patch"),
        namespace("functions", vec![function("exec"), custom("patch")]),
        namespace("other", vec![function("exec")])
    ]);
    v
}
fn custom_call(ns: Option<&str>, call_id: &str) -> Value {
    let mut v = json!({"type":"custom_tool_call","id":format!("item_{call_id}"),"status":"completed","name":"patch","call_id":call_id,"input":"*** Begin Patch\n中文🙂\n*** End Patch"});
    if let Some(ns) = ns {
        v["namespace"] = ns.into();
    }
    v
}
fn ns_call(ns: &str, call_id: &str) -> Value {
    let mut v = call("exec", call_id);
    v["namespace"] = ns.into();
    v
}
fn advanced(
    fixture: &Fixture,
    broker: Arc<Broker<Store>>,
    limits: Limits,
) -> OpenRouterProvider<Store> {
    provider(fixture, broker, limits)
        .with_advanced_tools("fixture".into())
        .unwrap()
}
fn mixed() -> Vec<Value> {
    vec![
        call("exec", "a"),
        custom_call(None, "b"),
        ns_call("functions", "c"),
        custom_call(Some("functions"), "d"),
        ns_call("other", "e"),
    ]
}
fn native_chunks(native: &Value) -> Vec<Value> {
    let mut events =
        vec![json!({"type":"response.created","response":{"id":"fixture","output":[]}})];
    for (index, item) in native["output"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            matches!(
                item["type"].as_str(),
                Some("function_call" | "custom_tool_call")
            )
        })
    {
        let custom = item["type"] == "custom_tool_call";
        let key = if custom { "input" } else { "arguments" };
        let kind = if custom {
            "response.custom_tool_call_input"
        } else {
            "response.function_call_arguments"
        };
        let mut added = item.clone();
        added["status"] = "in_progress".into();
        added[key] = "".into();
        events.push(json!({"type":"response.output_item.added","output_index":index,"item":added}));
        for delta in ["", item[key].as_str().unwrap()] {
            events.push(json!({"type":format!("{kind}.delta"),"output_index":index,"item_id":item["id"],"delta":delta,"future":{"big":18446744073709551616_u128}}));
        }
        let mut done =
            json!({"type":format!("{kind}.done"),"output_index":index,"item_id":item["id"]});
        done[key] = item[key].clone();
        events.push(done);
        events.push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
    }
    events.push(json!({"type":"response.completed","response":native}));
    events
}
async fn delivered(p: &OpenRouterProvider<Store>, source: Value) -> Vec<Value> {
    let mut events = p
        .stream_response(canonical(source), RequestContext::default())
        .await
        .unwrap()
        .events;
    let mut values = Vec::new();
    while let Some(event) = events.next().await {
        if let ProviderStreamEvent::Model(model) = event.unwrap() {
            values.push(model.response.wire().clone());
        }
    }
    values
}

#[tokio::test]
async fn explicit_advanced_route_gate_preserves_flat_and_default_rejections() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = super::super::broker(Some(KEY));
    assert!(
        fixture
            .provider(broker.clone(), limits())
            .with_advanced_tools("fixture".into())
            .is_err()
    );
    assert!(
        provider(&fixture, broker.clone(), limits())
            .with_advanced_tools("missing".into())
            .is_err()
    );
    assert!(
        advanced(&fixture, broker.clone(), limits())
            .with_advanced_tools("fixture".into())
            .is_err()
    );
    for p in [
        fixture.provider(broker.clone(), limits()),
        provider(&fixture, broker.clone(), limits()),
    ] {
        assert!(
            p.create_response(canonical(advanced_wire(false)), RequestContext::default())
                .await
                .is_err()
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn native_namespace_custom_json_preserves_qualified_identity_format_and_extensions() {
    let native = response(mixed());
    let mut fixture = Fixture::start(vec![Reply::json(native.clone()); 3]).await;
    let (broker, reads) = super::super::broker(Some(KEY));
    let p = advanced(&fixture, broker, limits()).with_runtime_context();
    for format in [
        Some(json!({"type":"grammar","syntax":"lark","definition":"start: /.+/"})),
        Some(json!({"type":"text"})),
        None,
    ] {
        let mut source = advanced_wire(false);
        if let Some(format) = format {
            source["tools"][1]["format"] = format;
        } else {
            source["tools"][1].as_object_mut().unwrap().remove("format");
        }
        let result = p
            .create_response(canonical(source.clone()), runtime_context())
            .await
            .unwrap();
        let captured = fixture.request().await;
        let sent = captured.body.unwrap();
        for key in ["tools", "tool_choice", "parallel_tool_calls"] {
            assert_eq!(sent[key], source[key]);
        }
        assert_eq!(result.response.wire(), &native);
        assert!(!captured.headers.contains("executor-private"));
    }
    assert_eq!(
        p.capabilities("fixture").unwrap().native_tools,
        CapabilitySupport::Unknown
    );
    assert_eq!(reads.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn named_and_allowed_selections_compile_exact_native_subsets_once() {
    let cases = vec![
        (
            json!({"type":"function","namespace":"functions","name":"exec"}),
            vec![namespace("functions", vec![function("exec")])],
            "required",
            ns_call("functions", "a"),
        ),
        (
            json!({"type":"custom","name":"patch"}),
            vec![custom("patch")],
            "required",
            custom_call(None, "a"),
        ),
        (
            json!({"type":"custom","namespace":"functions","name":"patch"}),
            vec![namespace("functions", vec![custom("patch")])],
            "required",
            custom_call(Some("functions"), "a"),
        ),
        (
            json!({"type":"allowed_tools","mode":"auto","tools":[{"type":"function","namespace":"other","name":"exec"},{"type":"custom","name":"patch"}]}),
            vec![custom("patch"), namespace("other", vec![function("exec")])],
            "auto",
            ns_call("other", "a"),
        ),
        (
            json!({"type":"allowed_tools","mode":"required","tools":[{"type":"function","name":"exec"},{"type":"custom","name":"patch"}]}),
            vec![function("exec"), custom("patch")],
            "required",
            custom_call(None, "a"),
        ),
    ];
    for (choice, tools, mode, item) in cases {
        let mut fixture = Fixture::start(vec![Reply::json(response(vec![item]))]).await;
        let (broker, _) = super::super::broker(Some(KEY));
        let p = advanced(&fixture, broker, limits());
        let mut source = advanced_wire(false);
        source["tool_choice"] = choice;
        p.create_response(canonical(source), RequestContext::default())
            .await
            .unwrap();
        let sent = fixture.request().await.body.unwrap();
        assert_eq!(sent["tools"], json!(tools));
        assert_eq!(sent["tool_choice"], mode);
        assert_eq!(sent["provider"]["only"], json!(["fixture-backend/region"]));
    }
}

#[tokio::test]
async fn paired_custom_and_function_history_preserves_order_and_text_results() {
    let mut fixture = Fixture::start(vec![Reply::json(response(vec![]))]).await;
    let (broker, _) = super::super::broker(Some(KEY));
    let p = advanced(&fixture, broker, limits());
    let mut input = mixed();
    for item in mixed().into_iter().rev() {
        input.push(json!({"type":if item["type"]=="custom_tool_call"{"custom_tool_call_output"}else{"function_call_output"},"call_id":item["call_id"],"output":[{"type":"input_text","text":"原始结果🙂"}]}));
    }
    let mut source = advanced_wire(false);
    source["input"] = json!(input);
    p.create_response(canonical(source.clone()), RequestContext::default())
        .await
        .unwrap();
    assert_eq!(
        fixture.request().await.body.unwrap()["input"],
        source["input"]
    );
}

#[tokio::test]
async fn malformed_namespace_custom_declarations_fail_before_credentials() {
    let mut bad = vec![
        namespace("ns", vec![]),
        namespace("ns", vec![namespace("nested", vec![function("x")])]),
        namespace("ns", vec![function("x"), custom("x")]),
        json!({"type":"custom","name":"patch","async":true}),
        json!({"type":"custom","name":"patch","format":{"type":"grammar","syntax":"regex","definition":""}}),
        json!({"type":"custom","name":"patch","format":{"type":"grammar","syntax":"unknown","definition":"x"}}),
        json!({"type":"custom","name":"patch","format":{"type":"text","future":true}}),
        json!({"type":"custom","name":"patch","defer_loading":true}),
        json!({"type":"openrouter:bash"}),
    ];
    let mut ns = namespace("ns", vec![function("x")]);
    ns["description"] = Value::Null;
    bad.push(ns);
    let mut f = function("x");
    f["strict"] = true.into();
    bad.push(namespace("ns", vec![f]));
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = super::super::broker(Some(KEY));
    let p = advanced(&fixture, broker, limits());
    for tool in bad {
        let mut source = advanced_wire(false);
        source["tools"] = json!([tool]);
        assert!(
            p.create_response(canonical(source), RequestContext::default())
                .await
                .is_err()
        );
    }
    for tools in [
        json!([
            namespace("ns", vec![function("x")]),
            namespace("ns", vec![function("y")])
        ]),
        json!([function("exec"), custom("exec")]),
    ] {
        let mut source = advanced_wire(false);
        source["tools"] = tools;
        assert!(
            p.create_response(canonical(source), RequestContext::default())
                .await
                .is_err()
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn invalid_selection_and_cross_kind_results_fail_before_credentials() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = super::super::broker(Some(KEY));
    let p = advanced(&fixture, broker, limits());
    for choice in [
        json!({"type":"function","name":"patch"}),
        json!({"type":"custom","namespace":"missing","name":"patch"}),
        json!({"type":"allowed_tools","mode":"none","tools":[]}),
        json!({"type":"allowed_tools","mode":"auto","tools":[]}),
        json!({"type":"allowed_tools","mode":"auto","tools":[{"type":"custom","name":"patch"},{"type":"custom","name":"patch"}]}),
    ] {
        let mut source = advanced_wire(false);
        source["tool_choice"] = choice;
        assert!(
            p.create_response(canonical(source), RequestContext::default())
                .await
                .is_err()
        );
    }
    for input in [
        json!([custom_call(None,"a"),{"type":"function_call_output","call_id":"a","output":"bad"}]),
        json!([call("exec","a"),{"type":"custom_tool_call_output","call_id":"a","output":"bad"}]),
        json!([custom_call(None,"a"),{"role":"user","content":"new"},{"type":"custom_tool_call_output","call_id":"a","output":"bad"}]),
    ] {
        let mut source = advanced_wire(false);
        source["input"] = input;
        assert!(
            p.create_response(canonical(source), RequestContext::default())
                .await
                .is_err()
        );
    }
    let mut ambiguous = custom_call(None, "a");
    ambiguous["arguments"] = "{}".into();
    let mut source = advanced_wire(false);
    source["input"] =
        json!([ambiguous,{"type":"custom_tool_call_output","call_id":"a","output":"bad"}]);
    assert!(
        p.create_response(canonical(source), RequestContext::default())
            .await
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn invalid_native_qualified_calls_and_custom_payloads_never_deliver_success() {
    let mut cases = Vec::new();
    for (key, value) in [
        ("name", json!("exec")),
        ("namespace", json!("missing")),
        ("input", json!({})),
        ("async", json!(true)),
        ("subagent_id", json!("server-agent")),
        ("id", json!("msg_reply")),
        ("status", json!("incomplete")),
    ] {
        let mut item = custom_call(None, "a");
        item[key] = value;
        cases.push((advanced_wire(false), response(vec![item])));
    }
    for choice in [
        json!("none"),
        json!({"type":"custom","namespace":"functions","name":"patch"}),
        json!({"type":"allowed_tools","mode":"auto","tools":[{"type":"function","name":"exec"}]}),
    ] {
        let mut source = advanced_wire(false);
        source["tool_choice"] = choice;
        cases.push((source, response(vec![custom_call(None, "a")])));
    }
    let mut single = advanced_wire(false);
    single["parallel_tool_calls"] = false.into();
    cases.push((single, response(mixed())));
    let mut required = advanced_wire(false);
    required["tool_choice"] = "required".into();
    cases.push((required, response(vec![])));
    cases.push((
        advanced_wire(false),
        response(vec![custom_call(None, "a"), ns_call("functions", "a")]),
    ));
    for (source, native) in cases {
        let fixture = Fixture::start(vec![Reply::json(native)]).await;
        let (broker, _) = super::super::broker(Some(KEY));
        let p = advanced(&fixture, broker, limits());
        assert_eq!(
            p.create_response(canonical(source), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            502
        );
    }
}

#[tokio::test]
async fn mixed_native_sse_preserves_custom_text_function_json_and_opaque_events() {
    let native = response(mixed());
    let events = native_chunks(&native);
    let mut fixture = Fixture::start(vec![Reply::stream(sse(&events))]).await;
    let (broker, _) = super::super::broker(Some(KEY));
    let p = advanced(&fixture, broker, limits());
    assert_eq!(delivered(&p, advanced_wire(true)).await, events);
    assert_eq!(
        fixture.request().await.body.unwrap()["tools"],
        advanced_wire(true)["tools"]
    );
}

#[tokio::test]
async fn corrupt_custom_sse_is_rejected_before_delivery_and_releases_slot() {
    let native = response(vec![custom_call(Some("functions"), "a")]);
    let events = native_chunks(&native);
    let mut cases = Vec::new();
    for (index, key, value) in [
        (2, "type", json!("response.function_call_arguments.delta")),
        (2, "item_id", json!("other")),
        (4, "input", json!("different")),
    ] {
        let mut e = events.clone();
        e[index][key] = value;
        cases.push(e);
    }
    let mut e = events.clone();
    e[5]["item"]["namespace"] = "other".into();
    cases.push(e);
    let mut e = events.clone();
    e[1]["item"]["namespace"] = "other".into();
    cases.push(e);
    let mut e = events.clone();
    e.remove(5);
    cases.push(e);
    for broken in cases {
        let mut fixture = Fixture::start(vec![
            Reply::stream(sse(&broken)),
            Reply::stream(sse(&events)),
        ])
        .await;
        let (broker, _) = super::super::broker(Some(KEY));
        let configured = Limits {
            in_flight: 1,
            ..limits()
        };
        let p = advanced(&fixture, broker, configured);
        let mut stream = p
            .stream_response(canonical(advanced_wire(true)), RequestContext::default())
            .await
            .unwrap()
            .events;
        assert_eq!(stream.next().await.unwrap().err().unwrap().http_status, 502);
        assert!(stream.next().await.is_none());
        fixture.request().await;
        fixture.disconnected().await;
        assert_eq!(delivered(&p, advanced_wire(true)).await, events);
        fixture.request().await;
    }
}

#[tokio::test]
async fn custom_done_only_terminal_only_and_empty_input_are_preserved() {
    let mut item = custom_call(None, "a");
    item["input"] = "".into();
    let native = response(vec![item]);
    let all = native_chunks(&native);
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
        let (broker, _) = super::super::broker(Some(KEY));
        let p = advanced(&fixture, broker, limits());
        assert_eq!(delivered(&p, advanced_wire(true)).await, events);
    }
}

#[tokio::test]
async fn selection_does_not_bypass_source_or_compiled_request_budgets() {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = super::super::broker(Some(KEY));
    let configured = Limits {
        request_bytes: 2048,
        ..limits()
    };
    let p = advanced(&fixture, broker, configured)
        .with_verbosity_instruction("fixture".into(), "low".into(), "g".repeat(2049))
        .unwrap();
    let mut source = advanced_wire(false);
    source["tools"][2]["description"] = "d".repeat(2049).into();
    source["tool_choice"] = json!({"type":"custom","name":"patch"});
    let mut compiled = advanced_wire(false);
    compiled["text"] = json!({"verbosity":"low"});
    for v in [source, compiled] {
        assert_eq!(
            p.create_response(canonical(v), RequestContext::default())
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
async fn gateway_delivers_native_custom_namespace_data_without_execution() {
    let native = response(vec![custom_call(Some("functions"), "a")]);
    let mut fixture = Fixture::start(vec![Reply::json(native.clone())]).await;
    let (broker, reads) = super::super::broker(Some(KEY));
    let p = Arc::new(advanced(&fixture, broker.clone(), limits()).with_runtime_context());
    let gateway = caidex_model_gateway::start_with_provider(p, broker.redactor(), limits())
        .await
        .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let result = client
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("x-client-request-id", "executor-private")
        .header("content-type", "application/json")
        .body(advanced_wire(false).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(result.status(), 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&result.bytes().await.unwrap()).unwrap(),
        native
    );
    let captured = fixture.request().await;
    assert!(!captured.headers.contains("executor-private"));
    assert!(!captured.headers.contains(gateway.token().expose()));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    gateway.shutdown().await.unwrap();
}
