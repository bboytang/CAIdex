use super::*;

fn deferred() -> Value {
    let mut declaration = namespace();
    declaration["tools"][0]["defer_loading"] = true.into();
    declaration["tools"].as_array_mut().unwrap().push(
        json!({"type":"custom","name":"patch","format":{"type":"text"},"defer_loading":true}),
    );
    declaration
}
fn patch() -> Value {
    json!({"type":"function_call","id":"fc_c2","status":"completed","namespace":"functions","name":"patch","call_id":"c2","arguments":" { \"input\" : \"exact\\n🙂\" } "})
}

#[tokio::test]
async fn deferred_json_sse_hide_root_catalog_and_bind_discovery_flags_and_raw_history() {
    for stream in [false, true] {
        let responses = [
            terminal(vec![searching("s1")]),
            terminal(vec![function("c1")]),
            terminal(vec![searching("s2")]),
            terminal(vec![patch()]),
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
        let provider = fixture.provider(broker, true).with_deferred_tool_search();
        let mut source = json!({"model":"fixture","input":[{"role":"user","content":"discover"}],"tools":[search(),deferred()]});
        for turn in 0..5 {
            let response = deliver(&provider, source.clone(), stream).await.unwrap();
            let native = fixture.request().await.body.unwrap();
            assert_eq!(native["tools"][1]["tools"], json!([]));
            if turn > 0 {
                let result = native["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|v| v["type"] == "tool_search_output")
                    .unwrap();
                assert!(
                    result["tools"][0]["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|v| v.get("defer_loading").is_none())
                );
                assert_eq!(result["tools"][0]["tools"][1]["type"], "function");
            }
            if turn >= 2 {
                assert!(
                    native["input"]
                        .as_array()
                        .unwrap()
                        .contains(&responses[1]["output"][0])
                );
            }
            if turn == 3 {
                assert_eq!(response.output()[1]["type"], "custom_tool_call");
                assert_eq!(response.output()[1]["input"], "exact\n🙂");
            }
            if turn == 4 {
                assert!(native["input"].as_array().unwrap().contains(&patch()));
                break;
            }
            let input = source["input"].as_array_mut().unwrap();
            input.extend(response.output().iter().cloned());
            input.push(if turn==1 { json!({"type":"function_call_output","call_id":"c1","output":"ordinary result"}) } else if turn==3 { json!({"type":"custom_tool_call_output","call_id":"c2","output":"custom result"}) } else {
                let mut loaded=deferred();
                if turn==0 { for member in loaded["tools"].as_array_mut().unwrap() { member["defer_loading"]=false.into(); } }
                json!({"type":"tool_search_output","execution":"client","status":"completed","call_id":if turn==0 {"s1"} else {"s2"},"tools":[loaded]})
            });
            input.push(json!({"role":"user","content":"continue"}));
            if turn == 2 {
                let mut changed = source.clone();
                let items = changed["input"].as_array_mut().unwrap();
                let last = items.len() - 2;
                items[last]["tools"][0]["tools"][0]["parameters"] =
                    json!({"type":"object","properties":{"changed":{"type":"string"}}});
                assert!(deliver(&provider, changed, stream).await.is_err());
                assert_eq!(reads.load(Ordering::SeqCst), 3);
            }
        }
        assert_eq!(reads.load(Ordering::SeqCst), 5);
    }
}

#[tokio::test]
async fn deferred_defaults_hidden_validation_and_calls_before_discovery_refuse_before_authentication()
 {
    let fixture = Fixture::start(vec![]).await;
    let (broker, reads) = broker();
    let default = fixture
        .provider(broker.clone(), true)
        .with_custom_tools_as_functions();
    let provider = fixture.provider(broker, true).with_deferred_tool_search();
    let source = json!({"model":"fixture","input":"discover","tools":[search(),deferred()]});
    for stream in [false, true] {
        assert!(deliver(&default, source.clone(), stream).await.is_err());
        let mut no_search = source.clone();
        no_search["tools"] = json!([deferred()]);
        assert!(deliver(&provider, no_search, stream).await.is_err());
        for (field, bad) in [
            ("defer_loading", json!("true")),
            ("parameters", Value::Null),
            ("strict", json!(true)),
            ("unknown", json!(true)),
        ] {
            let mut wire = source.clone();
            wire["tools"][1]["tools"][0][field] = bad;
            assert!(
                deliver(&provider, wire, stream).await.is_err(),
                "hidden declarations must be validated"
            );
        }
        let mut collision = source.clone();
        collision["tools"].as_array_mut().unwrap().push(
            json!({"type":"function","name":"functions.echo","parameters":{"type":"object"}}),
        );
        assert!(
            deliver(&provider, collision, stream).await.is_err(),
            "hidden aliases remain unambiguous"
        );
        let mut early = source.clone();
        early["input"] = json!([searching("s1"),function("c1"),{"type":"function_call_output","call_id":"c1","output":"premature"},{"type":"tool_search_output","execution":"client","status":"completed","call_id":"s1","tools":[deferred()]}]);
        assert!(deliver(&provider, early, stream).await.is_err());
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn deferred_native_hidden_calls_never_deliver_tools_or_history_and_release_slot() {
    for stream in [false, true] {
        for hidden in [function("c1"), patch()] {
            let mut fixture = Fixture::start(vec![
                reply(terminal(vec![hidden]), stream),
                reply(terminal(vec![searching("s1")]), stream),
            ])
            .await;
            let (broker, reads) = broker();
            let provider = fixture.provider(broker, true).with_deferred_tool_search();
            let source = json!({"model":"fixture","input":"discover","tools":[search(),deferred()],"stream":stream});
            if stream {
                let mut events = provider
                    .stream_response(request(source.clone()), RequestContext::default())
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
                        }
                        _ => (),
                    }
                }
                assert!(failed);
            } else {
                assert_eq!(
                    deliver(&provider, source.clone(), stream)
                        .await
                        .err()
                        .unwrap()
                        .code,
                    "ollama_invalid_native_tools"
                );
            }
            fixture.request().await;
            deliver(&provider, source, stream).await.unwrap();
            fixture.request().await;
            assert_eq!(reads.load(Ordering::SeqCst), 2);
        }
    }
}

#[tokio::test]
async fn deferred_v2_policy_and_original_flags_stay_bound_when_native_tools_are_identical() {
    for stream in [false, true] {
        let mut fixture =
            Fixture::start(vec![reply(finished(), stream), reply(finished(), stream)]).await;
        let (broker, reads) = broker();
        let old = fixture
            .provider(broker.clone(), true)
            .with_custom_tools_as_functions();
        let provider = fixture.provider(broker, true).with_deferred_tool_search();
        let user = json!({"role":"user","content":"plain"});
        let mut source = json!({"model":"fixture","input":[user],"tools":[namespace()]});
        let response = deliver(&old, source.clone(), stream).await.unwrap();
        fixture.request().await;
        let mut next = source.clone();
        next["input"]
            .as_array_mut()
            .unwrap()
            .extend(response.output().iter().cloned());
        assert_eq!(
            deliver(&provider, next, stream).await.err().unwrap().code,
            "ollama_history_prefix_mismatch"
        );
        source["tools"][0]["tools"][0]["defer_loading"] = false.into();
        let response = deliver(&provider, source.clone(), stream).await.unwrap();
        fixture.request().await;
        source["input"]
            .as_array_mut()
            .unwrap()
            .extend(response.output().iter().cloned());
        source["tools"][0]["tools"][0]
            .as_object_mut()
            .unwrap()
            .remove("defer_loading");
        assert_eq!(
            deliver(&provider, source, stream).await.err().unwrap().code,
            "ollama_history_prefix_mismatch"
        );
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}
