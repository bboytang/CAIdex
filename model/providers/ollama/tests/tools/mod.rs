use super::*;
use caidex_model_core::CanonicalResponse;

fn namespace() -> Value {
    json!({"type":"namespace","name":"functions","description":"Fixture namespace guidance","tools":[{"type":"function","name":"echo","description":"Echo exact arguments","parameters":{"type":"object"},"strict":false}]})
}
fn search() -> Value {
    json!({"type":"tool_search","execution":"client","description":"Discover local tools","parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}})
}
fn function(id: &str) -> Value {
    json!({"type":"function_call","id":format!("fc_{id}"),"status":"completed","namespace":"functions","name":"echo","call_id":id,"arguments":" { \"n\": 18446744073709551616, \"decimal\": 1.00, \"text\": \"你好🙂\" } "})
}
fn searching(id: &str) -> Value {
    json!({"type":"tool_search_call","id":format!("ts_{id}"),"status":"completed","execution":"client","call_id":id,"arguments":{"query":"fixture namespace","future":{"n":18446744073709551616_u128}}})
}
fn terminal(items: Vec<Value>) -> Value {
    json!({"id":"fixture","object":"response","status":"completed","output":items,"future":{"big":18446744073709551616_u128}})
}
fn finished() -> Value {
    terminal(vec![
        json!({"type":"message","id":"msg_end","status":"completed","role":"assistant","content":[{"type":"output_text","text":"done"}]}),
    ])
}
fn reply(native: Value, stream: bool) -> Reply {
    if !stream {
        return Reply::json(native);
    }
    let kind = match native["status"].as_str().unwrap() {
        "failed" => "response.failed",
        "incomplete" => "response.incomplete",
        _ => "response.completed",
    };
    Reply::stream(format!(
        "{CREATED}event: {kind}\ndata: {}\n\n",
        json!({"type":kind,"sequence_number":1,"response":native})
    ))
}
async fn deliver(
    provider: &OllamaProvider<Store>,
    mut wire: Value,
    stream: bool,
) -> caidex_model_core::ProviderResult<CanonicalResponse> {
    wire["stream"] = stream.into();
    if !stream {
        return provider
            .create_response(request(wire), RequestContext::default())
            .await
            .map(|v| v.response);
    }
    let mut events = provider
        .stream_response(request(wire), RequestContext::default())
        .await?
        .events;
    let mut terminal = None;
    while let Some(event) = events.next().await {
        if let ProviderStreamEvent::Model(event) = event?
            && event.response.terminal().is_some()
        {
            terminal =
                Some(CanonicalResponse::new(event.response.wire()["response"].clone()).unwrap());
        }
    }
    Ok(terminal.unwrap())
}

#[tokio::test]
async fn namespace_json_and_sse_compile_guidance_and_replay_exact_calls_and_legacy_results() {
    for stream in [false, true] {
        for legacy in 0..3 {
            let native = terminal(vec![function("c1")]);
            let mut fixture = Fixture::start(vec![
                reply(native.clone(), stream),
                reply(finished(), stream),
            ])
            .await;
            let (broker, reads) = broker();
            let provider = fixture.provider(broker, true).with_native_tools();
            let user = json!({"role":"user","content":"hello"});
            let source = json!({"model":"fixture","input":[user],"tools":[namespace()]});
            let first = deliver(&provider, source.clone(), stream).await.unwrap();
            assert_eq!(first.output()[1], native["output"][0]);
            let original = fixture.request().await.body.unwrap();
            assert!(original["tools"][0].get("description").is_none());
            assert_eq!(
                original["tools"][0]["tools"][0]["description"],
                "Namespace description: Fixture namespace guidance\nEcho exact arguments"
            );
            let mut input = vec![user.clone()];
            input.extend(first.output().iter().cloned());
            let result = if legacy == 2 {
                json!({"type":"function_call_output","name":"functions.echo","output":"exact result🙂"})
            } else if legacy == 1 {
                json!({"type":"function_call_output","namespace":"functions","name":"echo","output":"exact result🙂"})
            } else {
                json!({"type":"function_call_output","call_id":"c1","namespace":"functions","name":"echo","output":"exact result🙂"})
            };
            input.push(result.clone());
            input.push(json!({"role":"user","content":"next"}));
            let mut next = source.clone();
            next["input"] = json!(input);
            let mut changed = next.clone();
            changed["tools"][0]["description"] = "Changed policy".into();
            assert_eq!(
                deliver(&provider, changed, stream)
                    .await
                    .err()
                    .unwrap()
                    .code,
                "ollama_history_prefix_mismatch"
            );
            deliver(&provider, next, stream).await.unwrap();
            let captured = fixture.request().await.body.unwrap();
            assert_eq!(captured["input"][1], native["output"][0]);
            assert_eq!(captured["input"][2], result);
            assert_eq!(captured["tools"], original["tools"]);
            assert_eq!(reads.load(Ordering::SeqCst), 2);
        }
    }
}

#[tokio::test]
async fn client_search_json_and_sse_bind_dynamic_declarations_and_repeat_exact_history() {
    for stream in [false, true] {
        let responses = [
            terminal(vec![searching("s1")]),
            terminal(vec![function("c1")]),
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
        let provider = fixture.provider(broker, true).with_native_tools();
        let mut source = json!({"model":"fixture","input":[{"role":"user","content":"search"}],"tools":[search()]});
        let mut captured = Vec::new();
        for turn in 0..4 {
            let response = deliver(&provider, source.clone(), stream).await.unwrap();
            captured.push(fixture.request().await.body.unwrap());
            if turn == 3 {
                break;
            }
            let input = source["input"].as_array_mut().unwrap();
            input.extend(response.output().iter().cloned());
            input.push(if turn==1 { json!({"type":"function_call_output","call_id":"c1","output":"discovered result"}) } else { json!({"type":"tool_search_output","execution":"client","status":"completed","call_id":if turn==0 {"s1"} else {"s2"},"tools":[namespace()]}) });
            input.push(json!({"role":"user","content":"continue"}));
            if turn == 2 {
                let mut changed = source.clone();
                let items = changed["input"].as_array_mut().unwrap();
                let last = items.len() - 2;
                items[last]["tools"][0]["tools"][0]["parameters"] =
                    json!({"type":"object","properties":{"changed":{"type":"string"}}});
                assert_eq!(
                    deliver(&provider, changed, stream)
                        .await
                        .err()
                        .unwrap()
                        .code,
                    "ollama_invalid_tools"
                );
            }
        }
        assert_eq!(captured[1]["input"][1], responses[0]["output"][0]);
        assert_eq!(captured[2]["input"][4], responses[1]["output"][0]);
        assert_eq!(captured[3]["input"][7], responses[2]["output"][0]);
        assert_eq!(captured[1]["tools"], json!([search()]));
        assert_eq!(
            captured[1]["input"][2]["tools"][0]["tools"][0]["description"],
            "Namespace description: Fixture namespace guidance\nEcho exact arguments"
        );
        assert_eq!(
            captured[3]["input"][2]["tools"],
            captured[3]["input"][8]["tools"]
        );
        assert_eq!(reads.load(Ordering::SeqCst), 4);
    }
}

#[tokio::test]
async fn ambiguous_declarations_and_unpaired_or_unsupported_items_refuse_before_authentication() {
    let fixture = Fixture::start(vec![Reply::json(finished())]).await;
    let (broker, reads) = broker();
    let default = fixture.provider(broker.clone(), true).with_native_history();
    for tool in [namespace(), search()] {
        assert_eq!(
            deliver(
                &default,
                json!({"model":"fixture","input":"hello","tools":[tool]}),
                false
            )
            .await
            .err()
            .unwrap()
            .http_status,
            400
        );
    }
    let provider = fixture.provider(broker, true).with_native_tools();
    let mut cases = vec![
        json!({"tools":[namespace(),{"type":"function","name":"functions.echo","parameters":{}}]}),
        json!({"tools":[namespace(),{"type":"function","name":"functions:echo","parameters":{}}]}),
        json!({"tools":[namespace(),namespace()]}),
        json!({"tools":[search(),{"type":"function","name":"tool_search","parameters":{}}]}),
        json!({"tools":[{"type":"tool_search","execution":"server","parameters":{}}]}),
        json!({"tools":[{"type":"custom","name":"exec"}]}),
        json!({"tools":[{"type":"web_search"}]}),
        json!({"tools":[namespace()],"input":[{"type":"function_call","call_id":"c1","namespace":"wrong","name":"echo","arguments":"{}"},{"type":"function_call_output","call_id":"c1","output":"result"}]}),
        json!({"tools":[namespace()],"input":[{"type":"function_call_output","call_id":"orphan","output":"result"}]}),
        json!({"tools":[namespace()],"input":[function("c1"),{"type":"function_call_output","name":"functions:echo","output":"native alias mismatch"}]}),
        json!({"tools":[search()],"input":[searching("s1")]}),
        json!({"tools":[search()],"input":[function("c1"),{"type":"function_call_output","call_id":"c1","output":"before discovery"},searching("s1"),{"type":"tool_search_output","execution":"client","status":"completed","call_id":"s1","tools":[namespace()]}]}),
        json!({"tools":[search()],"input":[{"type":"tool_search_call","execution":"client","call_id":"s1","arguments":[]},{"type":"tool_search_output","execution":"client","status":"completed","call_id":"s1","tools":[]}]}),
        json!({"tools":[search()],"input":[{"type":"tool_search_output","execution":"client","status":"completed","call_id":"s1","tools":[]}]}),
        json!({"tools":[search()],"input":[searching("s1"),{"type":"function_call_output","call_id":"s1","output":"wrong kind"}]}),
        json!({"tools":[namespace()],"input":[function("c1"),{"type":"tool_search_output","execution":"client","status":"completed","call_id":"c1","tools":[]}]}),
        json!({"tools":[namespace()],"input":[function("c1"),function("c2"),{"type":"function_call_output","namespace":"functions","name":"echo","output":"ambiguous"}]}),
    ];
    for flag in ["strict", "defer_loading"] {
        let mut tool = namespace();
        tool["tools"][0][flag] = true.into();
        cases.push(json!({"tools":[tool]}));
    }
    let mut nested = namespace();
    nested["tools"] = json!([namespace()]);
    cases.push(json!({"tools":[nested]}));
    let mut collision = namespace();
    collision["tools"] = json!([{"type":"function","name":"_echo","parameters":{}},{"type":"function","name":"functions_echo","parameters":{}}]);
    cases.push(json!({"tools":[collision]}));
    for extra in cases {
        for stream in [false, true] {
            let mut wire = json!({"model":"fixture","input":"hello"});
            wire.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let error = deliver(&provider, wire, stream).await.err().unwrap();
            assert_eq!(error.http_status, 400);
            assert!(!format!("{error:?}").contains(KEY));
        }
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn invalid_native_calls_and_search_never_deliver_terminal_tools_or_carriers() {
    let mut unknown = function("c1");
    unknown["name"] = "undeclared".into();
    let mut args = function("c1");
    args["arguments"] = "[]".into();
    let mut server = searching("s1");
    server["execution"] = "server".into();
    let mut search_args = searching("s1");
    search_args["arguments"] = json!([]);
    let mut status = function("c1");
    status["status"] = "in_progress".into();
    let mut qualified = function("c1");
    qualified.as_object_mut().unwrap().remove("namespace");
    qualified["name"] = "functions.echo".into();
    let mut same_item_id = function("c2");
    same_item_id["id"] = "fc_c1".into();
    for output in [
        vec![qualified],
        vec![function("c1"), same_item_id],
        vec![
            json!({"type":"message","id":"fc_c1","status":"completed","role":"assistant","content":[{"type":"output_text","text":"same item ID"}]}),
            function("c1"),
        ],
        vec![unknown],
        vec![args],
        vec![server],
        vec![search_args],
        vec![status],
        vec![function("c1"), function("c1")],
        vec![
            json!({"type":"custom_tool_call","id":"custom","call_id":"c1","name":"echo","input":"arbitrary"}),
        ],
    ] {
        for stream in [false, true] {
            let fixture = Fixture::start(vec![reply(terminal(output.clone()), stream)]).await;
            let (broker, _) = broker();
            let provider = fixture.provider(broker, true).with_native_tools();
            let source = json!({"model":"fixture","input":"hello","tools":[namespace(),search()],"stream":stream});
            if !stream {
                assert_eq!(
                    deliver(&provider, source, false)
                        .await
                        .err()
                        .unwrap()
                        .http_status,
                    502
                );
                continue;
            }
            let mut events = provider
                .stream_response(request(source), RequestContext::default())
                .await
                .unwrap()
                .events;
            let mut failed = false;
            while let Some(event) = events.next().await {
                match event {
                    Err(error) => {
                        assert_eq!(error.http_status, 502);
                        failed = true;
                    }
                    Ok(ProviderStreamEvent::Model(event)) => {
                        assert_ne!(event.response.kind(), "response.output_item.done");
                        assert!(event.response.terminal().is_none());
                    }
                    _ => (),
                }
            }
            assert!(failed);
        }
    }
    // A completed old call ID must not become executable again on a later turn.
    let mut fixture = Fixture::start(vec![Reply::json(terminal(vec![function("c1")]))]).await;
    let (broker, _) = broker();
    let provider = fixture.provider(broker, true).with_native_tools();
    let mut source = json!({"model":"fixture","input":[{"role":"user","content":"hello"}],"tools":[namespace()]});
    let first = deliver(&provider, source.clone(), false).await.unwrap();
    fixture.request().await;
    source["input"]
        .as_array_mut()
        .unwrap()
        .extend(first.output().iter().cloned());
    source["input"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"function_call_output","call_id":"c1","output":"result"}));
    assert_eq!(
        deliver(&provider, source, false).await.err().unwrap().code,
        "ollama_invalid_native_tools"
    );
}

#[tokio::test]
async fn failed_and_incomplete_search_keep_actual_state_without_executable_done() {
    for state in ["failed", "incomplete"] {
        let mut native = terminal(vec![searching("s1")]);
        native["status"] = state.into();
        if state == "failed" {
            native["error"] = json!({"code":"fixture_failure","message":"safe"});
        } else {
            native["incomplete_details"] = json!({"reason":"max_output_tokens"});
        }
        let fixture = Fixture::start(vec![reply(native, true)]).await;
        let (broker, _) = broker();
        let provider = fixture.provider(broker, true).with_native_tools();
        let mut events = provider
            .stream_response(
                request(
                    json!({"model":"fixture","input":"hello","tools":[search()],"stream":true}),
                ),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .events;
        let mut terminal = false;
        while let Some(event) = events.next().await {
            if let ProviderStreamEvent::Model(event) = event.unwrap() {
                if event.response.kind() == "response.output_item.done" {
                    assert_ne!(event.response.wire()["item"]["type"], "tool_search_call");
                }
                if event.response.terminal().is_some() {
                    assert_eq!(event.response.wire()["response"]["status"], state);
                    terminal = true;
                }
            }
        }
        assert!(terminal);
    }
}

#[tokio::test]
async fn tool_guidance_budget_capability_and_precancel_guard_shared_transport() {
    let fixture = Fixture::start(vec![Reply::json(finished())]).await;
    let (broker, reads) = broker();
    let limited = OllamaProvider::new(
        OllamaConfig::new(&fixture.base, Some(reference())).unwrap(),
        vec![model("fixture", "native-fixture")],
        broker.clone(),
        Limits {
            request_bytes: 1024,
            ..limits()
        },
    )
    .unwrap()
    .with_native_tools();
    let mut large = namespace();
    large["description"] = "guidance".repeat(40).into();
    large["tools"] = json!([{"type":"function","name":"a","parameters":{}},{"type":"function","name":"b","parameters":{}},{"type":"function","name":"c","parameters":{}}]);
    for stream in [false, true] {
        assert_eq!(
            deliver(
                &limited,
                json!({"model":"fixture","input":"hello","tools":[large]}),
                stream
            )
            .await
            .err()
            .unwrap()
            .http_status,
            413
        );
    }
    let mut metadata = model("fixture", "native-fixture");
    metadata.capabilities.native_tools = caidex_model_core::CapabilitySupport::Unsupported;
    let unsupported = OllamaProvider::new(
        OllamaConfig::new(&fixture.base, Some(reference())).unwrap(),
        vec![metadata],
        broker.clone(),
        limits(),
    )
    .unwrap()
    .with_native_tools();
    assert_eq!(
        deliver(
            &unsupported,
            json!({"model":"fixture","input":"hello","tools":[namespace()]}),
            false
        )
        .await
        .err()
        .unwrap()
        .code,
        "ollama_unsupported_capability"
    );
    let provider = fixture.provider(broker, true).with_native_tools();
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert_eq!(
        provider
            .create_response(
                request(json!({"model":"fixture","input":"hello","tools":[search()]})),
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
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}
