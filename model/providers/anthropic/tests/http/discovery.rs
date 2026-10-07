use super::organization::{ORGANIZATION, organization};
use super::*;
use caidex_model_core::{CanonicalRequest, ModelProvider, ResponsesDialect};
use caidex_provider_anthropic::{AnthropicProvider, NativeMessage, ToolMap};
use futures_util::StreamExt;

fn search() -> Value {
    json!({"type":"tool_search","execution":"client","description":"Find Runtime tools","parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}})
}
fn loaded() -> Value {
    json!({"type":"namespace","name":"calendar","future":"preserve","tools":[{"type":"function","name":"create","defer_loading":true,"parameters":{"type":"object","properties":{"title":{"type":"string"}}},"future":"preserve"}]})
}
fn canonical(history: &[Value], stream: bool) -> CanonicalRequest {
    let mut input = vec![json!({"role":"user","content":"find calendar"})];
    input.extend_from_slice(history);
    CanonicalRequest::new(json!({"model":"alias","instructions":"fixed","tools":[search()],"input":input,"stream":stream}), ResponsesDialect::Classic).unwrap()
}
fn provider(base: &str) -> (AnthropicProvider<Store>, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let broker = Arc::new(Broker::new(
        Id::new("executor").unwrap(),
        Store {
            reads: reads.clone(),
            key: Some(KEY),
        },
    ));
    let config = AnthropicConfig::new(reference())
        .unwrap()
        .with_base_url(base)
        .unwrap()
        .with_expected_organization(ORGANIZATION)
        .unwrap()
        .with_inline_tools();
    let client = AnthropicClient::new(config, broker, Limits::default()).unwrap();
    let mut model = super::provider::profile();
    model.supports_system_messages = true;
    model.supports_tool_discovery = true;
    (
        AnthropicProvider::new(client, vec![model], 10).unwrap(),
        reads,
    )
}
fn native_call(name: Value, id: &str, input: Value) -> Value {
    let mut native = reply();
    native["id"] = id.into();
    native["content"][1] = json!({"type":"tool_use","id":id,"name":name,"input":input});
    native["stop_reason"] = "tool_use".into();
    native
}
fn sse(native: &Value) -> String {
    let mut start = native.clone();
    start["content"] = json!([]);
    start["stop_reason"] = Value::Null;
    let mut events = vec![json!({"type":"message_start","message":start})];
    for (index, block) in native["content"].as_array().unwrap().iter().enumerate() {
        let mut start = block.clone();
        if block["type"] == "tool_use" {
            start["input"] = json!({});
        }
        events.push(json!({"type":"content_block_start","index":index,"content_block":start}));
        if block["type"] == "tool_use" {
            let arguments = block["input"].to_string();
            let split = arguments
                .char_indices()
                .nth(arguments.chars().count() / 2)
                .unwrap()
                .0;
            for partial in [&arguments[..split], &arguments[split..]] {
                events.push(json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":partial}}));
            }
        }
        events.push(json!({"type":"content_block_stop","index":index}));
    }
    events.push(json!({"type":"message_delta","delta":{"stop_reason":native["stop_reason"]},"usage":native["usage"]}));
    events.push(json!({"type":"message_stop"}));
    events
        .into_iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect()
}

#[tokio::test]
async fn inline_and_thinking_betas_are_sent_together_for_json_and_sse() {
    for streaming in [false, true] {
        let mut native = reply();
        native["input_transformations"] = json!([]);
        let (base, mut requests, _, task) = fixture_with_responses(
            vec![(
                200,
                if streaming {
                    sse(&native)
                } else {
                    native.to_string()
                },
                if streaming {
                    "text/event-stream"
                } else {
                    "application/json"
                }
                .into(),
                String::new(),
            )],
            false,
        )
        .await;
        let broker = Arc::new(Broker::new(
            Id::new("executor").unwrap(),
            Store {
                reads: Arc::new(AtomicUsize::new(0)),
                key: Some(KEY),
            },
        ));
        let config = AnthropicConfig::new(reference())
            .unwrap()
            .with_base_url(&base)
            .unwrap()
            .with_inline_tools()
            .with_thinking_binding_controls();
        let client = AnthropicClient::new(config, broker, Limits::default()).unwrap();
        if streaming {
            let mut stream = client
                .stream_message("native", request(), RequestContext::default())
                .await
                .unwrap();
            while let Some(event) = stream.next().await {
                event.unwrap();
            }
        } else {
            client
                .create_message("native", request(), RequestContext::default())
                .await
                .unwrap();
        }
        let (head, _) = received(&mut requests).await;
        assert!(head.to_ascii_lowercase().contains(
            "anthropic-beta: thinking-binding-controls-2026-08-01,inline-tools-2026-09-15"
        ));
        task.await.unwrap();
    }
}

#[tokio::test]
async fn discovery_requires_executor_beta_system_support_and_organization_before_key_reads() {
    for (inline, system, scoped) in [
        (false, true, true),
        (true, false, true),
        (true, true, false),
    ] {
        let reads = Arc::new(AtomicUsize::new(0));
        let broker = Arc::new(Broker::new(
            Id::new("executor").unwrap(),
            Store {
                reads: reads.clone(),
                key: Some(KEY),
            },
        ));
        let mut config = AnthropicConfig::new(reference()).unwrap();
        if inline {
            config = config.with_inline_tools();
        }
        if scoped {
            config = config.with_expected_organization(ORGANIZATION).unwrap();
        }
        let client = AnthropicClient::new(config, broker, Limits::default()).unwrap();
        let mut model = super::provider::profile();
        model.supports_tool_discovery = true;
        model.supports_system_messages = system;
        assert!(AnthropicProvider::new(client, vec![model], 10).is_err());
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }
    let (client, reads) =
        super::organization::scoped_client("http://127.0.0.1:1/v1/", Limits::default());
    let provider = AnthropicProvider::new(client, vec![super::provider::profile()], 10).unwrap();
    for stream in [false, true] {
        let error = if stream {
            provider
                .stream_response(canonical(&[], true), RequestContext::default())
                .await
                .err()
                .unwrap()
        } else {
            provider
                .create_response(canonical(&[], false), RequestContext::default())
                .await
                .err()
                .unwrap()
        };
        assert_eq!(error.code, "unsupported_anthropic_tool_discovery");
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn raw_native_inline_blocks_require_beta_before_authentication_or_post() {
    let (base, mut requests, _, task) = fixture(vec![(200, reply().to_string())], false).await;
    let (client, reads) = client(&base, Some(KEY), Limits::default());
    let mut wire = request();
    wire["messages"].as_array_mut().unwrap().push(json!({"role":"system","content":[{"type":"tool_addition","tool":{"type":"tool_definition","definition":{"name":"calendar","input_schema":{"type":"object"}}}}]}));
    let error = client
        .create_message("native", wire.clone(), RequestContext::default())
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "anthropic_inline_tools_beta_required");
    assert_eq!(
        client
            .stream_message("native", wire, RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "anthropic_inline_tools_beta_required"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert!(requests.try_recv().is_err());
    task.abort();
}

#[tokio::test]
async fn client_discovery_keeps_signed_prefix_loads_inline_and_replays_loaded_calls() {
    for streaming in [false, true] {
        let search_map = ToolMap::new(&[search()], 10).unwrap();
        let loaded_map = ToolMap::new(&[loaded()], 10).unwrap();
        let first_native = native_call(
            search_map.native_tools()[0]["name"].clone(),
            "search-1",
            json!({"query":"calendar","future":18446744073709551616_u128}),
        );
        let second_native = native_call(
            loaded_map.native_tools()[0]["name"].clone(),
            "create-1",
            json!({"title":"你好🙂"}),
        );
        let mut replies = Vec::new();
        for native in [&first_native, &second_native, &reply()] {
            replies.push((
                200,
                organization(ORGANIZATION),
                "application/json".into(),
                String::new(),
            ));
            replies.push((
                200,
                if streaming {
                    sse(native)
                } else {
                    native.to_string()
                },
                if streaming {
                    "text/event-stream"
                } else {
                    "application/json"
                }
                .into(),
                format!("anthropic-organization-id: {ORGANIZATION}\r\n"),
            ));
        }
        let (base, mut requests, _, task) = fixture_with_responses(replies, false).await;
        let (provider, reads) = provider(&base);
        let mut history = Vec::new();
        let mut compiled = Vec::new();
        for turn in 0..3 {
            let response = if streaming {
                let mut response = None;
                let mut stream = provider
                    .stream_response(canonical(&history, true), RequestContext::default())
                    .await
                    .unwrap();
                while let Some(event) = stream.events.next().await {
                    if let caidex_model_core::ProviderStreamEvent::Model(event) = event.unwrap() {
                        assert!(
                            !event
                                .response
                                .kind()
                                .starts_with("response.function_call_arguments")
                                || turn == 1
                        );
                        if event.response.kind() == "response.completed" {
                            response = Some(
                                caidex_model_core::CanonicalResponse::new(
                                    event.response.wire()["response"].clone(),
                                )
                                .unwrap(),
                            );
                        }
                    }
                }
                response.unwrap()
            } else {
                provider
                    .create_response(canonical(&history, false), RequestContext::default())
                    .await
                    .unwrap()
                    .response
            };
            received(&mut requests).await;
            let (head, body) = received(&mut requests).await;
            assert!(
                head.to_ascii_lowercase()
                    .contains("anthropic-beta: inline-tools-2026-09-15")
            );
            compiled.push(serde_json::from_slice::<Value>(&body).unwrap());
            assert!(
                response.output()[0]["encrypted_content"]
                    .as_str()
                    .unwrap()
                    .starts_with("caidex.anthropic.native-message.v4:")
            );
            let restored =
                NativeMessage::from_responses_output(response.output(), "native", 128 * 1024)
                    .unwrap();
            if turn == 0 {
                assert_eq!(restored.wire(), &first_native);
            }
            if turn == 1 {
                assert_eq!(restored.wire(), &second_native);
            }
            history.extend_from_slice(response.output());
            if turn == 0 {
                assert_eq!(response.output()[1]["type"], "tool_search_call");
                assert_eq!(
                    response.output()[1]["arguments"],
                    first_native["content"][1]["input"]
                );
                history.push(json!({"type":"tool_search_output","execution":"client","status":"completed","call_id":"search-1","tools":[loaded()],"future":"preserve"}));
            } else if turn == 1 {
                assert_eq!(response.output()[1]["type"], "function_call");
                assert_eq!(response.output()[1]["namespace"], "calendar");
                history.push(json!({"type":"function_call_output","call_id":"create-1","output":"actual fixture result 你好🙂"}));
            }
        }
        task.await.unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 3);
        for wire in &compiled[1..] {
            assert_eq!(wire["tools"], compiled[0]["tools"]);
            assert_eq!(wire["system"], compiled[0]["system"]);
        }
        assert_eq!(
            compiled[1]["messages"][1],
            json!({"role":"assistant","content":first_native["content"]})
        );
        assert_eq!(
            compiled[1]["messages"][2]["content"][0]["tool_use_id"],
            "search-1"
        );
        assert_eq!(
            compiled[1]["messages"][3]["content"][0]["type"],
            "tool_addition"
        );
        assert_eq!(
            compiled[1]["messages"][3]["content"][0]["tool"]["type"],
            "tool_definition"
        );
        assert_eq!(
            &compiled[2]["messages"].as_array().unwrap()[..4],
            compiled[1]["messages"].as_array().unwrap()
        );
        let mut changed = history.clone();
        changed[2]["tools"][0]["tools"][0]["parameters"]["properties"]["title"]["type"] =
            "integer".into();
        let error = provider
            .create_response(canonical(&changed, false), RequestContext::default())
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, "anthropic_replay_prefix_mismatch");
        assert_eq!(reads.load(Ordering::SeqCst), 3);
        // A discovery snapshot cannot be rewritten or downgraded independently
        // of the native search call, result and inline definitions it binds.
        for mutation in 0..5 {
            let index = if mutation == 4 { 0 } else { 3 };
            let mut changed = history.clone();
            let capsule = changed[index]["encrypted_content"].as_str().unwrap();
            let mut envelope: Value =
                serde_json::from_str(capsule.split_once(':').unwrap().1).unwrap();
            let version = if mutation == 4 { 3 } else { 4 };
            match mutation {
                0 => {
                    envelope["discoveries"][0]["tools"][0]["tools"][0]["parameters"]["properties"]
                        ["title"]["type"] = "integer".into()
                }
                1 => envelope["discoveries"] = json!([]),
                2 => envelope["discoveries"][0]["call_id"] = "forged".into(),
                3 => {
                    envelope["binding"]["prefix"]["messages"][2]["content"][0]["content"][0]["text"] =
                        "edited result".into()
                }
                4 => {
                    envelope["version"] = 3.into();
                    envelope.as_object_mut().unwrap().remove("discoveries");
                }
                _ => unreachable!(),
            }
            changed[index]["encrypted_content"] =
                format!("caidex.anthropic.native-message.v{version}:{envelope}").into();
            assert!(
                NativeMessage::from_responses_output(
                    &changed[index..index + 2],
                    "native",
                    128 * 1024
                )
                .is_err(),
                "mutation {mutation} was accepted"
            );
            for stream in [false, true] {
                let error = if stream {
                    provider
                        .stream_response(canonical(&changed, true), RequestContext::default())
                        .await
                        .err()
                        .unwrap()
                } else {
                    provider
                        .create_response(canonical(&changed, false), RequestContext::default())
                        .await
                        .err()
                        .unwrap()
                };
                assert_eq!(error.code, "invalid_anthropic_replay");
                assert_eq!(reads.load(Ordering::SeqCst), 3);
            }
        }
    }
}
