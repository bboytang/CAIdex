use caidex_model_core::{CanonicalRequest, ResponsesDialect, StreamState};
use caidex_provider_anthropic::{MessageOutcome, NativeMessage};
use serde_json::{Value, json};
const LIMIT: usize = 128 * 1024;
fn message(reason: &str) -> Value {
    json!({"type":"message","id":"msg-projection","role":"assistant","model":"native-fixture",
        "content":[{"type":"thinking","thinking":"原生🙂","signature":"signature+/==\n","future":"keep"},
        {"type":"redacted_thinking","data":"opaque+/==\n"},
        {"type":"text","text":"answer","citations":[{"type":"future_citation","data":42}]},
        {"type":"tool_use","id":"call-one","name":"data_only","input":{"n":18446744073709551616_u128}},
        {"type":"future_block","opaque":{"keep":true}}],"stop_reason":reason,
        "usage":{"input_tokens":2,"cache_creation_input_tokens":3,"cache_read_input_tokens":5,"output_tokens":7,"future":{"cost":"0.0001"}},
        "future":{"precise":18446744073709551616_u128}})
}
fn projected() -> Vec<Value> {
    NativeMessage::parse(message("tool_use"))
        .unwrap()
        .to_responses(LIMIT)
        .unwrap()
        .output()
        .to_vec()
}

#[test]
fn native_projection_roundtrips_all_native_fields_through_classic_and_lite_wire() {
    let native = NativeMessage::parse(message("tool_use")).unwrap();
    let response = native.to_responses(LIMIT).unwrap();
    assert_eq!(response.output().len(), 3);
    assert_eq!(response.output()[1]["phase"], "commentary");
    assert_eq!(
        response.output()[2]["arguments"],
        "{\"n\":18446744073709551616}"
    );
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let request = CanonicalRequest::new(
            json!({"model":"alias","input":response.output(),"stream":true}),
            dialect,
        )
        .unwrap();
        let stored = serde_json::to_vec(&request).unwrap();
        let restored: Value = serde_json::from_slice(&stored).unwrap();
        let native_again = NativeMessage::from_responses_output(
            restored["input"].as_array().unwrap(),
            "native-fixture",
            LIMIT,
        )
        .unwrap();
        assert_eq!(native_again.wire(), native.wire());
        assert_eq!(native_again.replay_message(), native.replay_message());
        assert!(!format!("{request:?}").contains("signature"));
    }
    assert!(!format!("{response:?}").contains("signature"));
}

#[test]
fn input_binding_reports_block_lossy_projection_and_replay_but_preserve_future_reports() {
    for (kind, reason, code) in [
        (
            "thinking_dropped",
            "prefix_binding_mismatch",
            "anthropic_input_thinking_dropped",
        ),
        (
            "thinking_dropped",
            "model_binding_mismatch",
            "anthropic_input_thinking_dropped",
        ),
        (
            "thinking_dropped",
            "organization_binding_mismatch",
            "anthropic_input_thinking_dropped",
        ),
        (
            "thinking_dropped",
            "end_user_binding_mismatch",
            "anthropic_input_thinking_dropped",
        ),
        (
            "thinking_mismatch_allowed",
            "prefix_binding_mismatch",
            "anthropic_input_binding_mismatch",
        ),
    ] {
        let mut wire = message("tool_use");
        let report = json!([{"type":kind,"reason":reason,"path":"messages.1.content.0","private":"PRIVATE_REPORT"}]);
        wire["input_transformations"] = report.clone();
        let native = NativeMessage::parse(wire).unwrap();
        let error = native.to_responses(LIMIT).unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.http_status, 502);
        assert!(!format!("{error:?}").contains("PRIVATE_REPORT"));
        assert_eq!(
            native.input_transformations().unwrap(),
            report.as_array().unwrap()
        );

        // Old or altered carriers cannot bypass the common replay check.
        let mut output = projected();
        let prefix = "caidex.anthropic.native-message.v1:";
        let mut envelope: Value = serde_json::from_str(
            output[0]["encrypted_content"]
                .as_str()
                .unwrap()
                .strip_prefix(prefix)
                .unwrap(),
        )
        .unwrap();
        envelope["message"]["input_transformations"] = report;
        output[0]["encrypted_content"] = format!("{prefix}{envelope}").into();
        assert_eq!(
            NativeMessage::from_responses_output(&output, "native-fixture", LIMIT)
                .unwrap_err()
                .code,
            "invalid_anthropic_replay"
        );
    }
    for report in [
        Value::Null,
        json!([]),
        json!([{"type":"future_transform","opaque":{"n":18446744073709551616_u128}}]),
        json!([{"type":"thinking_dropped","reason":"future_binding","path":"messages.1.content.0","opaque":"retain"}]),
    ] {
        let mut wire = message("tool_use");
        wire["input_transformations"] = report;
        let native = NativeMessage::parse(wire.clone()).unwrap();
        let response = native.to_responses(LIMIT).unwrap();
        let restored =
            NativeMessage::from_responses_output(response.output(), "native-fixture", LIMIT)
                .unwrap();
        assert_eq!(restored.wire(), &wire);
    }
}
#[test]
fn stop_outcomes_and_phases_are_preserved_without_flattening_limits_or_pause() {
    for (reason, state, detail) in [
        ("end_turn", StreamState::Completed, None),
        ("stop_sequence", StreamState::Completed, None),
        ("tool_use", StreamState::Completed, None),
        ("refusal", StreamState::Completed, None),
        (
            "max_tokens",
            StreamState::Incomplete,
            Some("max_output_tokens"),
        ),
        (
            "model_context_window_exceeded",
            StreamState::Incomplete,
            Some("context_window_exceeded"),
        ),
        (
            "pause_turn",
            StreamState::Incomplete,
            Some("provider_pause_turn"),
        ),
        (
            "future_reason",
            StreamState::Incomplete,
            Some("unknown_provider_stop_reason"),
        ),
    ] {
        let native = NativeMessage::parse(message(reason)).unwrap();
        let response = native.to_responses(LIMIT).unwrap();
        assert_eq!(response.state(), state);
        assert_eq!(response.wire()["caidex_native_stop_reason"], reason);
        assert_eq!(
            response.wire()["incomplete_details"]["reason"].as_str(),
            detail
        );
        let restored =
            NativeMessage::from_responses_output(response.output(), "native-fixture", LIMIT)
                .unwrap();
        assert_eq!(restored.outcome(), native.outcome());
    }
    let mut wire = message("end_turn");
    wire["stop_details"] = json!({"type":"refusal","future":"keep"});
    let native = NativeMessage::parse(wire).unwrap();
    assert_eq!(native.outcome(), MessageOutcome::Refusal);
    assert_eq!(
        native.to_responses(LIMIT).unwrap().wire()["caidex_native_outcome"],
        "Refusal"
    );
}
#[test]
fn cached_input_is_counted_once_and_native_usage_remains_exact() {
    let native = NativeMessage::parse(message("end_turn")).unwrap();
    let response = native.to_responses(LIMIT).unwrap();
    let usage = response.usage().unwrap().unwrap();
    assert_eq!(usage.input_tokens, Some(10));
    assert_eq!(usage.output_tokens, Some(7));
    assert_eq!(usage.total_tokens, Some(17));
    assert_eq!(usage.raw["input_tokens_details"]["cached_tokens"], 5);
    assert_eq!(usage.raw["caidex_native_usage"], native.wire()["usage"]);
    for key in [
        "input_tokens",
        "cache_read_input_tokens",
        "cache_creation_input_tokens",
    ] {
        let mut wire = message("end_turn");
        wire["usage"].as_object_mut().unwrap().remove(key);
        let response = NativeMessage::parse(wire.clone())
            .unwrap()
            .to_responses(LIMIT)
            .unwrap();
        assert!(response.usage().unwrap().is_none());
        assert_eq!(response.wire()["caidex_native_usage"], wire["usage"]);
        let restored =
            NativeMessage::from_responses_output(response.output(), "native-fixture", LIMIT)
                .unwrap();
        assert_eq!(restored.wire()["usage"], wire["usage"]);
    }
    let mut wire = message("end_turn");
    wire["usage"]["input_tokens"] = u64::MAX.into();
    assert_eq!(
        NativeMessage::parse(wire)
            .unwrap()
            .to_responses(LIMIT)
            .unwrap_err()
            .code,
        "anthropic_usage_overflow"
    );
}
#[test]
fn wrong_model_version_provider_bad_capsules_and_size_limits_fail_safely() {
    let output = projected();
    assert_eq!(
        NativeMessage::from_responses_output(&output, "other-model", LIMIT)
            .unwrap_err()
            .code,
        "anthropic_replay_model_mismatch"
    );
    assert!(NativeMessage::from_responses_output(&output, "native-fixture", 5).is_err());
    assert!(NativeMessage::from_responses_output(&[], "native-fixture", LIMIT).is_err());
    for capsule in [
        "openai-opaque",
        "caidex.anthropic.native-message.v2:{}",
        "caidex.anthropic.native-message.v1:{bad private payload",
        "caidex.anthropic.native-message.v1:{\"provider\":\"gemini\",\"version\":1}",
        "caidex.anthropic.native-message.v1:{\"provider\":\"anthropic\",\"version\":2}",
    ] {
        let mut output = output.clone();
        output[0]["encrypted_content"] = capsule.into();
        let error =
            NativeMessage::from_responses_output(&output, "native-fixture", LIMIT).unwrap_err();
        assert!(!format!("{error:?}").contains("private payload"));
    }
    let native = NativeMessage::parse(message("tool_use")).unwrap();
    assert!(native.to_responses(0).is_err());
    assert!(native.to_responses(5).is_err());
}
#[test]
fn edited_removed_reordered_and_duplicated_projection_items_cannot_replay_stale_data() {
    let original = projected();
    for kind in [
        "text",
        "role",
        "name",
        "call",
        "input",
        "namespace",
        "summary",
        "removed",
        "reordered",
        "duplicated",
    ] {
        let mut output = original.clone();
        match kind {
            "text" => output[1]["content"][0]["text"] = "edited".into(),
            "role" => output[1]["role"] = "user".into(),
            "name" => output[2]["name"] = "other".into(),
            "call" => output[2]["call_id"] = "other".into(),
            "input" => output[2]["arguments"] = "{\"n\":1}".into(),
            "namespace" => output[2]["namespace"] = "other".into(),
            "summary" => output[0]["summary"][0]["text"] = "edited".into(),
            "removed" => {
                output.pop();
            }
            "reordered" => output.swap(1, 2),
            "duplicated" => output.push(output[1].clone()),
            _ => unreachable!(),
        };
        assert!(
            NativeMessage::from_responses_output(&output, "native-fixture", LIMIT).is_err(),
            "{kind}"
        );
    }
}
#[test]
fn runtime_presentation_ids_and_equivalent_tool_json_do_not_mutate_native_replay() {
    let mut output = projected();
    for item in &mut output {
        item.as_object_mut().unwrap().remove("id");
        item["status"] = "completed".into();
    }
    output[2]["arguments"] = " { \"n\" : 18446744073709551616 } ".into();
    let native = NativeMessage::from_responses_output(&output, "native-fixture", LIMIT).unwrap();
    assert_eq!(native.wire(), &message("tool_use"));
}
#[test]
fn server_and_unknown_blocks_stay_opaque_and_never_become_runtime_client_tools() {
    let mut wire = message("end_turn");
    wire["content"] = json!([
        {"type":"server_tool_use","id":"server-one","name":"provider_owned","input":{}},
        {"type":"future_block","data":{"precision":18446744073709551616_u128}}]);
    let native = NativeMessage::parse(wire.clone()).unwrap();
    let response = native.to_responses(LIMIT).unwrap();
    assert_eq!(response.output().len(), 1);
    assert_eq!(
        NativeMessage::from_responses_output(response.output(), "native-fixture", LIMIT)
            .unwrap()
            .wire(),
        &wire
    );
}

#[test]
fn fallback_history_keeps_raw_wire_but_echoes_only_valid_hops_and_projects_serving_calls() {
    use caidex_provider_anthropic::MessagesRequest;
    let content = json!([
        {"type":"thinking","thinking":"primary","signature":"primary-signature"},
        {"type":"redacted_thinking","data":"primary-redaction"},
        {"type":"connector_text","text":"primary narration"},
        {"type":"text","text":"partial primary"},
        {"type":"tool_use","id":"declined-call","name":"data_only","input":{}},
        {"type":"server_tool_use","id":"server-kept","name":"web_search","input":{}},
        {"type":"web_search_tool_result","tool_use_id":"server-kept","content":[]},
        {"type":"server_tool_use","id":"server-orphan","name":"web_search","input":{}},
        {"type":"future_block","opaque":18446744073709551616_u128},
        {"type":"fallback","from":{"model":"primary"},"to":{"model":"middle"}},
        {"type":"thinking","thinking":"middle","signature":"middle-signature"},
        {"type":"tool_use","id":"middle-call","name":"data_only","input":{}},
        {"type":"fallback","from":{"model":"middle"},"to":{"model":"native-fixture"}},
        {"type":"thinking","thinking":"serving","signature":"serving-signature"},
        {"type":"text","text":"serving answer"},
        {"type":"tool_use","id":"serving-call","name":"data_only","input":{"n":18446744073709551616_u128}},
        {"type":"server_tool_use","id":"serving-server","name":"web_search","input":{}}
    ]);
    let mut wire = message("tool_use");
    wire["content"] = content.clone();
    let native = NativeMessage::parse(wire.clone()).unwrap();
    let response = native.to_responses(LIMIT).unwrap();
    assert_eq!(response.output().len(), 4); // carrier, two texts, serving call
    assert_eq!(response.output()[3]["call_id"], "serving-call");
    let expected = json!({"role":"assistant","content":[content[3],content[5],content[6],content[8],content[9],content[12],content[13],content[14],content[15],content[16]]});
    assert_eq!(native.replay_message(), expected);
    assert_eq!(native.wire(), &wire); // Echo filtering never deletes stored history.
    let restored =
        NativeMessage::from_responses_output(response.output(), "native-fixture", LIMIT).unwrap();
    assert_eq!(restored.wire(), &wire);
    assert_eq!(restored.replay_message(), expected);
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let mut input = vec![json!({"role":"user","content":"start"})];
        input.extend_from_slice(response.output());
        input.push(json!({"type":"function_call_output","call_id":"serving-call","output":"done"}));
        let canonical =
            CanonicalRequest::new(json!({"model":"alias","input":input}), dialect).unwrap();
        let compiled =
            MessagesRequest::from_responses(&canonical, "native-fixture", 100, LIMIT, 10).unwrap();
        assert_eq!(compiled.wire()["messages"][1], expected);
        assert_eq!(
            compiled.wire()["messages"][2]["content"][0]["tool_use_id"],
            "serving-call"
        );
        let mut bad = canonical.wire().clone();
        bad["input"].as_array_mut().unwrap().push(json!({"type":"function_call_output","call_id":"declined-call","output":"must not run"}));
        assert!(
            MessagesRequest::from_responses(
                &CanonicalRequest::new(bad, dialect).unwrap(),
                "native-fixture",
                100,
                LIMIT,
                10
            )
            .is_err()
        );
    }
}
