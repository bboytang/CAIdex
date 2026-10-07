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
        let response = NativeMessage::parse(wire)
            .unwrap()
            .to_responses(LIMIT)
            .unwrap();
        let usage = response.usage().unwrap().unwrap();
        assert_eq!(usage.input_tokens, None);
        assert_eq!(usage.total_tokens, None);
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
