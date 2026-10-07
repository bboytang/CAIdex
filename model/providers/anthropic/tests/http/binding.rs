use super::organization::{ORGANIZATION, organization, scoped_client};
use super::*;
use caidex_model_core::{CanonicalRequest, ModelProvider, ResponsesDialect};
use caidex_provider_anthropic::{
    AnthropicProvider, MessagesRequest, NativeMessage, RequestOptions, ToolMap,
};
use futures_util::StreamExt;

fn canonical(dialect: ResponsesDialect, history: &[Value], stream: bool) -> CanonicalRequest {
    let mut input = vec![json!({"role":"user","content":"hello"})];
    if !history.is_empty() {
        input.extend_from_slice(history);
        input.push(json!({"role":"user","content":"continue"}));
    }
    let mut wire = json!({"model":"alias","input":input,"stream":stream});
    if dialect == ResponsesDialect::Classic {
        wire["instructions"] = "fixed instructions".into();
    } else {
        wire["input"].as_array_mut().unwrap().insert(
            0,
            json!({"role":"developer","content":"fixed instructions"}),
        );
    }
    CanonicalRequest::new(wire, dialect).unwrap()
}
fn envelope(output: &[Value]) -> Value {
    let capsule = output[0]["encrypted_content"].as_str().unwrap();
    assert!(capsule.starts_with("caidex.anthropic.native-message.v3:"));
    serde_json::from_str(capsule.split_once(':').unwrap().1).unwrap()
}
fn compile(
    wire: Value,
    dialect: ResponsesDialect,
) -> caidex_model_core::ProviderResult<MessagesRequest> {
    MessagesRequest::from_responses_with_options(
        &CanonicalRequest::new(wire, dialect).unwrap(),
        "native",
        200,
        128 * 1024,
        10,
        &RequestOptions {
            expected_organization: Some(ORGANIZATION),
            supports_system_messages: true,
            ..Default::default()
        },
    )
}
fn native_replies(replies: Vec<Value>) -> Vec<(u16, String, String, String)> {
    replies
        .into_iter()
        .flat_map(|reply| {
            [
                (
                    200,
                    organization(ORGANIZATION),
                    "application/json".into(),
                    String::new(),
                ),
                (
                    200,
                    reply.to_string(),
                    "application/json".into(),
                    format!("anthropic-organization-id: {ORGANIZATION}\r\n"),
                ),
            ]
        })
        .collect()
}

#[tokio::test]
async fn bound_history_errors_precede_key_reads_for_json_and_sse_and_legacy_is_not_adopted() {
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let (base, mut requests, _, task) =
            fixture_with_responses(native_replies(vec![reply()]), false).await;
        let (client, reads) = scoped_client(&base, Limits::default());
        let provider =
            AnthropicProvider::new(client, vec![super::provider::profile()], 10).unwrap();
        let first = provider
            .create_response(canonical(dialect, &[], false), RequestContext::default())
            .await
            .unwrap()
            .response;
        received(&mut requests).await;
        received(&mut requests).await;
        task.await.unwrap();
        let original = canonical(dialect, first.output(), false).wire().clone();
        let history_index = if dialect == ResponsesDialect::Classic {
            1
        } else {
            2
        };
        let mut cases = Vec::new();
        let mut changed = original.clone();
        if dialect == ResponsesDialect::Classic {
            changed["instructions"] = "changed mode".into();
        } else {
            changed["input"][0]["content"] = "changed mode".into();
        }
        cases.push((changed, "anthropic_replay_prefix_mismatch"));
        let mut changed = original.clone();
        changed["input"][history_index - 1]["content"] = "edited user history".into();
        cases.push((changed, "anthropic_replay_prefix_mismatch"));
        let mut changed = original.clone();
        changed["input"]
            .as_array_mut()
            .unwrap()
            .remove(history_index - 1);
        cases.push((changed, "anthropic_replay_prefix_mismatch"));
        for (field, value, code) in [
            (
                "organization",
                json!("another-organization"),
                "anthropic_replay_organization_mismatch",
            ),
            ("organization", json!(""), "invalid_anthropic_replay"),
            (
                "prefix",
                json!({"messages":"bad"}),
                "invalid_anthropic_replay",
            ),
            (
                "prefix",
                json!({"messages":[],"untrusted":true}),
                "invalid_anthropic_replay",
            ),
        ] {
            let mut changed = original.clone();
            let mut stored = envelope(first.output());
            stored["binding"][field] = value;
            changed["input"][history_index]["encrypted_content"] =
                format!("caidex.anthropic.native-message.v3:{stored}").into();
            cases.push((changed, code));
        }
        let legacy = NativeMessage::parse(reply())
            .unwrap()
            .to_responses_with_tools(&ToolMap::new(&[], 10).unwrap(), 128 * 1024)
            .unwrap();
        cases.push((
            canonical(dialect, legacy.output(), false).wire().clone(),
            "anthropic_replay_binding_missing",
        ));
        let mut downgrade = original.clone();
        let mut stored = envelope(first.output());
        stored["version"] = 2.into();
        downgrade["input"][history_index]["encrypted_content"] =
            format!("caidex.anthropic.native-message.v2:{stored}").into();
        cases.push((downgrade, "invalid_anthropic_replay"));
        for (wire, code) in cases {
            for streaming in [false, true] {
                let mut wire = wire.clone();
                wire["stream"] = streaming.into();
                let request = CanonicalRequest::new(wire, dialect).unwrap();
                let error = if streaming {
                    provider
                        .stream_response(request, RequestContext::default())
                        .await
                        .err()
                        .unwrap()
                } else {
                    provider
                        .create_response(request, RequestContext::default())
                        .await
                        .err()
                        .unwrap()
                };
                assert_eq!(error.code, code);
                assert_eq!(error.http_status, 400);
                assert!(!format!("{error:?}").contains("private"));
                assert_eq!(reads.load(Ordering::SeqCst), 1);
            }
        }
        let (unscoped, reads) =
            super::client("http://127.0.0.1:1/v1", Some(KEY), Limits::default());
        let unscoped =
            AnthropicProvider::new(unscoped, vec![super::provider::profile()], 10).unwrap();
        let error = unscoped
            .create_response(
                CanonicalRequest::new(original, dialect).unwrap(),
                RequestContext::default(),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, "anthropic_replay_organization_required");
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn bound_tool_prefix_reorders_but_does_not_change_and_resume_appends_mode_in_place() {
    let tools = vec![
        json!({"type":"custom","name":"patch"}),
        json!({"type":"function","name":"lookup","parameters":{"type":"object","properties":{"thinking":{"type":"string"}}}}),
    ];
    let map = ToolMap::new(&tools, 10).unwrap();
    let mut native = reply();
    native["content"].as_array_mut().unwrap().push(json!({"type":"tool_use","id":"tool-one","name":map.native_tools()[0]["name"],"input":{"input":"\npatch🙂\n"}}));
    native["stop_reason"] = "tool_use".into();
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let (base, mut requests, _, task) =
            fixture_with_responses(native_replies(vec![native.clone(), reply()]), false).await;
        let (client, reads) = scoped_client(&base, Limits::default());
        let provider =
            AnthropicProvider::new(client, vec![super::provider::profile()], 10).unwrap();
        let mut first_wire = canonical(dialect, &[], false).wire().clone();
        if dialect == ResponsesDialect::Classic {
            first_wire["tools"] = tools.clone().into();
        } else {
            first_wire["input"].as_array_mut().unwrap().insert(
                0,
                json!({"type":"additional_tools","role":"developer","tools":tools}),
            );
        }
        let first = provider
            .create_response(
                CanonicalRequest::new(first_wire.clone(), dialect).unwrap(),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .response;
        assert_eq!(first.output().last().unwrap()["input"], "\npatch🙂\n");
        let stored = envelope(first.output());
        assert_eq!(
            stored["binding"]["prefix"]["tools"],
            json!(map.native_tools())
        );
        let mut resumed = first_wire;
        resumed["input"]
            .as_array_mut()
            .unwrap()
            .extend_from_slice(first.output());
        resumed["input"].as_array_mut().unwrap().push(json!({"type":"custom_tool_call_output","call_id":"tool-one","output":"result unchanged"}));
        let mut forged = resumed.clone();
        let mut stored = stored.clone();
        stored["tools"][0]["description"] = "not the original declaration".into();
        let history_index = if dialect == ResponsesDialect::Classic {
            1
        } else {
            3
        };
        forged["input"][history_index]["encrypted_content"] =
            format!("caidex.anthropic.native-message.v3:{stored}").into();
        assert_eq!(
            compile(forged, dialect).unwrap_err().code,
            "invalid_anthropic_replay"
        );
        let tool_path = if dialect == ResponsesDialect::Classic {
            "/tools"
        } else {
            "/input/0/tools"
        };
        for change in 0..4 {
            let mut changed = resumed.clone();
            let declarations = changed
                .pointer_mut(tool_path)
                .unwrap()
                .as_array_mut()
                .unwrap();
            match change {
                0 => {
                    declarations.pop();
                }
                1 => {
                    declarations.push(json!({"type":"custom","name":"new-tool"}));
                }
                2 => {
                    declarations[1]["parameters"]["properties"]["thinking"]["type"] =
                        "number".into()
                }
                _ => declarations[0]["description"] = "changed description".into(),
            }
            assert_eq!(
                compile(changed, dialect).unwrap_err().code,
                "anthropic_replay_prefix_mismatch"
            );
        }
        resumed
            .pointer_mut(tool_path)
            .unwrap()
            .as_array_mut()
            .unwrap()
            .reverse();
        resumed["tool_choice"] = "none".into();
        resumed["input"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"developer","content":"next mode, appended"}));
        let expected = compile(resumed.clone(), dialect).unwrap();
        assert_eq!(
            expected.wire()["messages"][1],
            NativeMessage::parse(native.clone())
                .unwrap()
                .replay_message()
        );
        assert_eq!(
            expected.wire()["messages"][2]["content"][0]["content"],
            json!([{"type":"text","text":"result unchanged"}])
        );
        assert_eq!(
            expected.wire()["messages"][3],
            json!({"role":"system","content":[{"type":"text","text":"next mode, appended"}]})
        );
        // Recreate the execution provider from persisted JSON, not from a cache.
        drop(provider);
        let (client, resumed_reads) = scoped_client(&base, Limits::default());
        let mut model = super::provider::profile();
        model.max_tokens = 200;
        model.supports_system_messages = true;
        let provider = AnthropicProvider::new(client, vec![model], 10).unwrap();
        let mut resumed: Value = serde_json::from_str(&resumed.to_string()).unwrap();
        let second = provider
            .create_response(
                CanonicalRequest::new(resumed.clone(), dialect).unwrap(),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .response;
        assert_eq!(
            envelope(second.output())["binding"]["prefix"]["messages"],
            expected.wire()["messages"]
        );
        resumed["input"]
            .as_array_mut()
            .unwrap()
            .extend_from_slice(second.output());
        resumed["input"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"user","content":"next"}));
        compile(resumed.clone(), dialect).unwrap();
        for item in resumed["input"].as_array_mut().unwrap() {
            if item["type"] == "custom_tool_call_output" {
                item["output"] = "edited old tool result".into();
            }
        }
        assert_eq!(
            compile(resumed, dialect).unwrap_err().code,
            "anthropic_replay_prefix_mismatch"
        );
        received(&mut requests).await;
        received(&mut requests).await;
        received(&mut requests).await;
        let (_, body) = received(&mut requests).await;
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            *expected.wire()
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(resumed_reads.load(Ordering::SeqCst), 1);
        task.await.unwrap();
    }
}

#[tokio::test]
async fn bound_thinking_trim_keeps_a_suffix_and_rejects_gaps_and_reinsertion() {
    let mut replies = Vec::new();
    for turn in 0..4 {
        let mut native = reply();
        native["id"] = format!("msg-{turn}").into();
        native["content"][0]["signature"] = format!("signature-{turn}").into();
        replies.push(native);
    }
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let (base, mut requests, _, task) =
            fixture_with_responses(native_replies(replies.clone()), false).await;
        let (client, reads) = scoped_client(&base, Limits::default());
        let provider =
            AnthropicProvider::new(client, vec![super::provider::profile()], 10).unwrap();
        let mut wire = canonical(dialect, &[], false).wire().clone();
        for turn in 0..3 {
            let response = provider
                .create_response(
                    CanonicalRequest::new(wire.clone(), dialect).unwrap(),
                    RequestContext::default(),
                )
                .await
                .unwrap()
                .response;
            wire["input"]
                .as_array_mut()
                .unwrap()
                .extend_from_slice(response.output());
            wire["input"]
                .as_array_mut()
                .unwrap()
                .push(json!({"role":"user","content":format!("turn-{turn}")}));
        }
        compile(wire.clone(), dialect).unwrap();
        let first_index = if dialect == ResponsesDialect::Classic {
            1
        } else {
            2
        };
        let mut latest_removed = wire.clone();
        latest_removed["input"]
            .as_array_mut()
            .unwrap()
            .remove(first_index + 6);
        compile(latest_removed, dialect).unwrap();
        let mut all_removed = wire.clone();
        all_removed["input"]
            .as_array_mut()
            .unwrap()
            .retain(|item| item["type"] != "reasoning");
        compile(all_removed, dialect).unwrap();
        let mut gap = wire.clone();
        gap["input"].as_array_mut().unwrap().remove(first_index + 3);
        assert_eq!(
            compile(gap, dialect).unwrap_err().code,
            "anthropic_replay_prefix_mismatch"
        );
        let mut trimmed = wire.clone();
        trimmed["input"].as_array_mut().unwrap().remove(first_index);
        let compiled = compile(trimmed.clone(), dialect).unwrap();
        assert_eq!(
            compiled.wire()["messages"][1]["content"],
            json!([{"type":"text","text":"中文🙂"}])
        );
        assert_eq!(
            compiled.wire()["messages"][3]["content"][0]["signature"],
            "signature-1"
        );
        let response = provider
            .create_response(
                CanonicalRequest::new(trimmed.clone(), dialect).unwrap(),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .response;
        let final_binding = envelope(response.output());
        assert!(
            final_binding["binding"]["prefix"]["messages"][1]["content"][0]
                .get("signature")
                .is_none()
        );
        let mut reinserted = wire;
        reinserted["input"]
            .as_array_mut()
            .unwrap()
            .extend_from_slice(response.output());
        assert_eq!(
            compile(reinserted, dialect).unwrap_err().code,
            "anthropic_replay_prefix_mismatch"
        );
        trimmed["input"]
            .as_array_mut()
            .unwrap()
            .extend_from_slice(response.output());
        compile(trimmed, dialect).unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 4);
        for _ in 0..8 {
            received(&mut requests).await;
        }
        task.await.unwrap();
    }
}

fn signed_stream() -> String {
    let mut start = reply();
    start["content"] = json!([]);
    start["stop_reason"] = Value::Null;
    let events = [
        json!({"type":"message_start","message":start}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"private"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig+/==\n"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"中文🙂"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}),
        json!({"type":"message_stop"}),
    ];
    let body: String = events
        .iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect();
    body
}

#[tokio::test]
async fn bound_snapshot_budget_failure_never_returns_done_or_new_history() {
    for streaming in [false, true] {
        let mut replies = native_replies(vec![reply()]);
        if streaming {
            replies[1].1 = signed_stream();
            replies[1].2 = "text/event-stream".into();
        }
        let (base, mut requests, _, task) = fixture_with_responses(replies, false).await;
        let limits = Limits {
            response_bytes: 4096,
            ..Default::default()
        };
        let (client, reads) = scoped_client(&base, limits);
        let provider =
            AnthropicProvider::new(client, vec![super::provider::profile()], 10).unwrap();
        let mut wire = canonical(ResponsesDialect::Classic, &[], streaming)
            .wire()
            .clone();
        wire["input"][0]["content"] = "long original prefix".repeat(500).into();
        let request = CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap();
        let error = if streaming {
            let mut stream = provider
                .stream_response(request, RequestContext::default())
                .await
                .unwrap()
                .events;
            let mut failure = None;
            while let Some(event) = tokio::time::timeout(Duration::from_secs(5), stream.next())
                .await
                .unwrap()
            {
                match event {
                    Ok(caidex_model_core::ProviderStreamEvent::Model(event)) => {
                        assert_ne!(event.response.kind(), "response.output_item.done");
                        assert!(event.response.terminal().is_none());
                    }
                    Ok(caidex_model_core::ProviderStreamEvent::Heartbeat) => (),
                    Err(error) => {
                        failure = Some(error);
                    }
                }
            }
            failure.unwrap()
        } else {
            provider
                .create_response(request, RequestContext::default())
                .await
                .err()
                .unwrap()
        };
        assert_eq!(error.code, "anthropic_replay_too_large");
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        received(&mut requests).await;
        received(&mut requests).await;
        task.await.unwrap();
    }
}

#[tokio::test]
async fn bound_sse_completion_retains_native_signatures_and_replays_to_json() {
    let body = signed_stream();
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let mut replies = native_replies(vec![reply(), reply()]);
        replies[1].1 = body.clone();
        replies[1].2 = "text/event-stream".into();
        let (base, mut requests, _, task) = fixture_with_responses(replies, false).await;
        let (client, reads) = scoped_client(&base, Limits::default());
        let provider =
            AnthropicProvider::new(client, vec![super::provider::profile()], 10).unwrap();
        let mut stream = provider
            .stream_response(canonical(dialect, &[], true), RequestContext::default())
            .await
            .unwrap()
            .events;
        let mut completed = None;
        while let Some(event) = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
        {
            let caidex_model_core::ProviderStreamEvent::Model(event) = event.unwrap() else {
                continue;
            };
            if event.response.kind() == "response.completed" {
                completed = Some(
                    event.response.wire()["response"]["output"]
                        .as_array()
                        .unwrap()
                        .clone(),
                );
            }
        }
        let output = completed.unwrap();
        assert_eq!(envelope(&output)["binding"]["organization"], ORGANIZATION);
        assert_eq!(
            NativeMessage::from_responses_output(&output, "native", 128 * 1024)
                .unwrap()
                .content(),
            reply()["content"].as_array().unwrap()
        );
        provider
            .create_response(
                canonical(dialect, &output, false),
                RequestContext::default(),
            )
            .await
            .unwrap();
        received(&mut requests).await;
        let (head, _) = received(&mut requests).await;
        assert!(
            head.to_ascii_lowercase()
                .contains("accept: text/event-stream")
        );
        received(&mut requests).await;
        let (_, body) = received(&mut requests).await;
        let wire: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(wire["messages"][1]["content"], reply()["content"]);
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        task.await.unwrap();
    }
}

#[tokio::test]
async fn provider_bound_json_records_actual_compiled_prefix_and_replays_without_key_rereads() {
    let mut native_reply = reply();
    native_reply["future"] = json!({"exact":18446744073709551616u128});
    native_reply["content"].as_array_mut().unwrap().push(json!({"type":"future_native_block","thinking":"must not be normalized away","opaque":"+/==\n"}));
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let scope = format!("anthropic-organization-id: {ORGANIZATION}\r\n");
        let (base, mut requests, _, task) = fixture_with_responses(
            vec![
                (
                    200,
                    organization(ORGANIZATION),
                    "application/json".into(),
                    String::new(),
                ),
                (
                    200,
                    native_reply.to_string(),
                    "application/json".into(),
                    scope.clone(),
                ),
                (
                    200,
                    organization(ORGANIZATION),
                    "application/json".into(),
                    String::new(),
                ),
                (
                    200,
                    native_reply.to_string(),
                    "application/json".into(),
                    scope,
                ),
            ],
            false,
        )
        .await;
        let (client, reads) = scoped_client(&base, Limits::default());
        let provider =
            AnthropicProvider::new(client, vec![super::provider::profile()], 10).unwrap();
        let first = provider
            .create_response(canonical(dialect, &[], false), RequestContext::default())
            .await
            .unwrap()
            .response;
        let binding = envelope(first.output());
        assert_eq!(binding["binding"]["organization"], ORGANIZATION);
        assert_eq!(
            binding["binding"]["prefix"],
            json!({
                "system":[{"type":"text","text":"fixed instructions"}],
                "messages":[{"role":"user","content":[{"type":"text","text":"hello"}]}],
            })
        );
        assert_eq!(
            NativeMessage::from_responses_output(first.output(), "native", 128 * 1024)
                .unwrap()
                .wire(),
            &native_reply
        );
        let second = provider
            .create_response(
                canonical(dialect, first.output(), false),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .response;
        let second = envelope(second.output());
        assert_eq!(
            second["binding"]["prefix"]["messages"][1]["content"][0]["signature"],
            "sig+/==\n"
        );
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        let mut request = canonical(dialect, first.output(), false).wire().clone();
        let mappings = [
            caidex_provider_anthropic::ReasoningMapping::new(
                "high".into(),
                Some("medium".into()),
                Some(json!({"type":"adaptive"})),
            )
            .unwrap(),
            caidex_provider_anthropic::ReasoningMapping::new(
                "low".into(),
                None,
                Some(json!({"type":"disabled"})),
            )
            .unwrap(),
        ];
        for effort in ["high", "low"] {
            request["reasoning"] = json!({"effort":effort});
            let compiled = MessagesRequest::from_responses_with_options(
                &CanonicalRequest::new(request.clone(), dialect).unwrap(),
                "native",
                200,
                128 * 1024,
                10,
                &RequestOptions {
                    expected_organization: Some(ORGANIZATION),
                    reasoning_mappings: &mappings,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                compiled.wire()["messages"][1]["content"],
                native_reply["content"]
            );
            assert_eq!(
                compiled.wire()["thinking"]["type"],
                if effort == "high" {
                    "adaptive"
                } else {
                    "disabled"
                }
            );
        }
        for _ in 0..2 {
            assert!(
                received(&mut requests)
                    .await
                    .0
                    .starts_with("GET /proxy/v1/organizations/me ")
            );
            let (head, body) = received(&mut requests).await;
            assert!(head.starts_with("POST /proxy/v1/messages "));
            let wire: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                wire["system"],
                json!([{"type":"text","text":"fixed instructions"}])
            );
            assert!(wire.get("binding").is_none());
        }
        task.await.unwrap();
    }
}

#[tokio::test]
async fn verbosity_style_is_bound_to_actual_native_prefix_before_credential_reads() {
    use caidex_provider_anthropic::VerbosityMapping;
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let mut replies = native_replies(vec![reply(), reply()]);
        replies[3].1 = signed_stream();
        replies[3].2 = "text/event-stream".into();
        let (base, mut requests, _, task) = fixture_with_responses(replies, false).await;
        let (client, reads) = scoped_client(&base, Limits::default());
        let mut profile = super::provider::profile();
        profile.verbosity_mappings = vec![
            VerbosityMapping::new("low".into(), "Keep answers concise.".into()).unwrap(),
            VerbosityMapping::new("high".into(), "Explain with necessary detail.".into()).unwrap(),
        ];
        let provider = AnthropicProvider::new(client, vec![profile], 10).unwrap();
        let mut wire = canonical(dialect, &[], false).wire().clone();
        wire["text"] = json!({"verbosity":"low"});
        let first = provider
            .create_response(
                CanonicalRequest::new(wire, dialect).unwrap(),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .response;
        let expected_system = json!([
            {"type":"text","text":"fixed instructions"},
            {"type":"text","text":"Keep answers concise."}
        ]);
        assert_eq!(
            envelope(first.output())["binding"]["prefix"]["system"],
            expected_system
        );
        let mut resumed = canonical(dialect, first.output(), true).wire().clone();
        resumed["text"] = json!({"verbosity":"low"});
        let mut stream = provider
            .stream_response(
                CanonicalRequest::new(resumed.clone(), dialect).unwrap(),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .events;
        let mut completed = false;
        while let Some(event) = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
        {
            if let caidex_model_core::ProviderStreamEvent::Model(event) = event.unwrap()
                && event.response.kind() == "response.completed"
            {
                completed = true;
            }
        }
        assert!(completed);
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        for level in ["high", "low"] {
            for streaming in [false, true] {
                let mut changed = resumed.clone();
                changed["stream"] = streaming.into();
                if level == "high" {
                    changed["text"]["verbosity"] = level.into();
                } else {
                    changed.as_object_mut().unwrap().remove("text");
                }
                let request = CanonicalRequest::new(changed, dialect).unwrap();
                let error = if streaming {
                    provider
                        .stream_response(request, RequestContext::default())
                        .await
                        .err()
                        .unwrap()
                } else {
                    provider
                        .create_response(request, RequestContext::default())
                        .await
                        .err()
                        .unwrap()
                };
                assert_eq!(error.code, "anthropic_replay_prefix_mismatch");
                assert_eq!(reads.load(Ordering::SeqCst), 2);
            }
        }
        // Updating execution-side guidance must also fail old signed-prefix replay.
        let (client, updated_reads) = scoped_client(&base, Limits::default());
        let mut profile = super::provider::profile();
        profile.verbosity_mappings =
            vec![VerbosityMapping::new("low".into(), "Changed style instruction.".into()).unwrap()];
        let provider = AnthropicProvider::new(client, vec![profile], 10).unwrap();
        let error = provider
            .stream_response(
                CanonicalRequest::new(resumed, dialect).unwrap(),
                RequestContext::default(),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, "anthropic_replay_prefix_mismatch");
        assert_eq!(updated_reads.load(Ordering::SeqCst), 0);
        for _ in 0..2 {
            received(&mut requests).await;
            let (_, body) = received(&mut requests).await;
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["system"], expected_system);
            assert!(body.get("text").is_none());
        }
        task.await.unwrap();
    }
}
