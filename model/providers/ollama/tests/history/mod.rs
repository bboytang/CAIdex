use super::*;
mod stream;
use caidex_model_core::CanonicalResponse;
use caidex_provider_ollama::NativeHistory;

const BUDGET: usize = 64 * 1024;
const PREFIX: &str = "caidex.ollama.native-history.v1:";

#[tokio::test]
async fn thinking_only_turn_is_sealed_before_a_new_user_and_ambiguous_attachment_is_rejected() {
    for case in ["trailing", "consecutive", "wrong_role"] {
        let mut terminal = native().wire().clone();
        let thinking = terminal["output"][0].clone();
        terminal["output"] = match case {
            "consecutive" => json!([thinking, thinking]),
            "wrong_role" => {
                json!([thinking,{"type":"message","role":"user","content":"wrong attachment"}])
            }
            _ => json!([thinking]),
        };
        let mut fixture = Fixture::start(vec![Reply::json(terminal.clone())]).await;
        let (broker, reads) = broker();
        let provider = fixture.provider(broker, true).with_native_history();
        let original = json!({"role":"user","content":"hello"});
        let response = provider
            .create_response(
                request(json!({"model":"fixture","input":[original]})),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .response;
        fixture.request().await;
        let mut input = vec![original.clone()];
        input.extend(response.output().iter().cloned());
        input.push(json!({"role":"user","content":"next question"}));
        let response = provider
            .create_response(
                request(json!({"model":"fixture","input":input})),
                RequestContext::default(),
            )
            .await;
        if case == "trailing" {
            response.unwrap();
            let expected = json!([original,terminal["output"][0],{"role":"assistant","content":""},{"role":"user","content":"next question"}]);
            assert_eq!(fixture.request().await.body.unwrap()["input"], expected);
            assert_eq!(reads.load(Ordering::SeqCst), 2);
        } else {
            assert_eq!(response.err().unwrap().http_status, 400);
            assert_eq!(reads.load(Ordering::SeqCst), 1);
            assert_eq!(fixture.accepted.load(Ordering::SeqCst), 1);
        }
    }
}
fn profile() -> OllamaConfig {
    OllamaConfig::new("https://fixture.invalid/proxy/v1", Some(reference())).unwrap()
}
fn seed() -> CanonicalRequest {
    request(
        json!({"model":"native-fixture","input":[{"role":"user","content":"hello"}],
        "instructions":"original instruction","tools":[{"type":"function","name":"echo","parameters":{"type":"object"}}],
        "think":"high","stream":false}),
    )
}
fn native() -> CanonicalResponse {
    let mut wire = response_wire();
    wire["model"] = "native-fixture".into();
    wire["output"].as_array_mut().unwrap().insert(0,json!({"type":"reasoning","id":"native-rs","status":"completed",
        "summary":[{"type":"summary_text","text":"private thought🙂","future":{"n":18446744073709551616_u128}}],
        "encrypted_content":"private thought🙂","future_signature":"retain verbatim+/="}));
    wire["output"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"future_output","future":18446744073709551616_u128}));
    CanonicalResponse::new(wire).unwrap()
}
fn record() -> NativeHistory {
    NativeHistory::from_response(&profile(), "native-fixture", &seed(), &native(), BUDGET).unwrap()
}
fn replay(
    input: &[Value],
    expected: &CanonicalRequest,
) -> caidex_model_core::ProviderResult<(NativeHistory, usize)> {
    NativeHistory::from_responses_prefix(input, &profile(), "native-fixture", expected, BUDGET)
}

#[test]
fn json_capsule_roundtrips_native_reasoning_order_extensions_and_precise_payloads() {
    let history = record();
    assert_eq!(history.native_response(), native().wire());
    assert_eq!(history.request(), seed().wire());
    assert!(!format!("{history:?}").contains("private thought"));
    let projection = history.to_responses(BUDGET).unwrap();
    assert_eq!(projection.output().len(), 3);
    assert_eq!(
        projection.output()[0]["summary"],
        native().output()[0]["summary"]
    );
    assert!(
        projection.output()[0]["encrypted_content"]
            .as_str()
            .unwrap()
            .starts_with(PREFIX)
    );
    assert_eq!(&projection.output()[1..], &native().output()[1..]);
    let mut input = projection.output().to_vec();
    input.push(json!({"type":"function_call_output","call_id":"c1","output":"tool result"}));
    let (restored, count) = replay(&input, &seed()).unwrap();
    assert_eq!(count, projection.output().len());
    assert_eq!(restored.native_response(), native().wire());
    assert_eq!(
        restored.native_response()["output"][1]["arguments"],
        " { \"n\": 1.00 } "
    );
    let mut drop_identity = projection.output().to_vec();
    for item in &mut drop_identity {
        item.as_object_mut().unwrap().remove("id");
        item.as_object_mut().unwrap().remove("status");
    }
    assert_eq!(
        replay(&drop_identity, &seed()).unwrap().0.native_response(),
        native().wire()
    );
}

#[test]
fn scope_model_prefix_and_complete_display_group_are_checked_against_executor_state() {
    let projected = record().to_responses(BUDGET).unwrap();
    for (config, model) in [
        (
            OllamaConfig::new("https://different.invalid/proxy/v1", Some(reference())).unwrap(),
            "native-fixture",
        ),
        (
            OllamaConfig::new("https://fixture.invalid/another/v1", Some(reference())).unwrap(),
            "native-fixture",
        ),
        (
            OllamaConfig::new("https://fixture.invalid/proxy/v1", None).unwrap(),
            "native-fixture",
        ),
        (profile(), "different-native"),
    ] {
        assert_eq!(
            NativeHistory::from_responses_prefix(
                projected.output(),
                &config,
                model,
                &seed(),
                BUDGET
            )
            .err()
            .unwrap()
            .code,
            "ollama_history_model_mismatch"
        );
    }
    let mut other = reference();
    other.profile = Id::new("another-profile").unwrap();
    let config = OllamaConfig::new("https://fixture.invalid/proxy/v1", Some(other)).unwrap();
    assert_eq!(
        NativeHistory::from_responses_prefix(
            projected.output(),
            &config,
            "native-fixture",
            &seed(),
            BUDGET
        )
        .err()
        .unwrap()
        .code,
        "ollama_history_model_mismatch"
    );
    for (field, value) in [
        ("model", json!("other-alias")),
        ("instructions", json!("changed")),
        ("tools", json!([])),
        ("input", json!([{"role":"user","content":"another branch"}])),
    ] {
        let mut expected = seed().wire().clone();
        expected[field] = value;
        assert_eq!(
            replay(projected.output(), &request(expected))
                .err()
                .unwrap()
                .code,
            "ollama_history_prefix_mismatch"
        );
    }
    for (index, field, value) in [
        (0, "summary", json!([])),
        (1, "call_id", json!("other")),
        (1, "arguments", json!("{}")),
        (1, "name", json!("changed")),
        (2, "future", json!(1)),
    ] {
        let mut input = projected.output().to_vec();
        input[index][field] = value;
        assert_eq!(
            replay(&input, &seed()).err().unwrap().code,
            "ollama_invalid_history"
        );
    }
    assert!(replay(&projected.output()[..2], &seed()).is_err());
    let mut reordered = projected.output().to_vec();
    reordered.swap(1, 2);
    assert!(replay(&reordered, &seed()).is_err());
    let mut controls = seed().wire().clone();
    controls["stream"] = true.into();
    controls["think"] = false.into();
    controls["temperature"] = 0.1.into();
    assert!(replay(projected.output(), &request(controls)).is_ok());
}

#[test]
fn stream_capsule_reconstructs_complete_native_wire_and_rejects_fabricated_or_truncated_chunks() {
    let response = native();
    let created: Value = serde_json::from_str(
        CREATED
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap(),
    )
    .unwrap();
    let delta = json!({"type":"response.reasoning_summary_text.delta","sequence_number":1,"delta":"private thought🙂","future":18446744073709551616_u128});
    let terminal =
        json!({"type":"response.completed","sequence_number":2,"response":response.wire()});
    let chunks = vec![created, delta, terminal];
    let history = NativeHistory::from_stream(
        &profile(),
        "native-fixture",
        &seed(),
        &response,
        &chunks,
        BUDGET,
    )
    .unwrap();
    assert_eq!(history.wire()["chunks"], json!(chunks));
    let projection = history.to_responses(BUDGET).unwrap();
    assert_eq!(
        replay(projection.output(), &seed()).unwrap().0.wire(),
        history.wire()
    );
    for bad in [
        chunks[..2].to_vec(),
        vec![chunks[2].clone(), chunks[2].clone()],
        vec![
            chunks[0].clone(),
            json!({"type":"response.completed","sequence_number":2,"response":response_wire()}),
        ],
    ] {
        assert_eq!(
            NativeHistory::from_stream(
                &profile(),
                "native-fixture",
                &seed(),
                &response,
                &bad,
                BUDGET
            )
            .err()
            .unwrap()
            .code,
            "ollama_invalid_history"
        );
    }
}

#[test]
fn carrier_limits_foreign_versions_malformed_native_reasoning_and_terminal_states_are_explicit() {
    let history = record();
    assert_eq!(
        history.to_responses(1).err().unwrap().code,
        "ollama_history_too_large"
    );
    assert_eq!(
        NativeHistory::from_response(&profile(), "native-fixture", &seed(), &native(), 0)
            .err()
            .unwrap()
            .code,
        "ollama_history_too_large"
    );
    let mut projected = history.to_responses(BUDGET).unwrap().output().to_vec();
    for capsule in [
        "other-provider+/=",
        "caidex.google.native-history.v1:{}",
        "caidex.ollama.native-history.v2:{}",
        "caidex.ollama.native-history.v1:{",
    ] {
        projected[0]["encrypted_content"] = capsule.into();
        assert_eq!(
            replay(&projected, &seed()).err().unwrap().code,
            "ollama_invalid_history"
        );
    }
    for (field, value) in [
        ("encrypted_content", Value::Null),
        ("summary", json!([{"type":"future_summary","text":"x"}])),
    ] {
        let mut wire = native().wire().clone();
        wire["output"][0][field] = value;
        assert_eq!(
            NativeHistory::from_response(
                &profile(),
                "native-fixture",
                &seed(),
                &CanonicalResponse::new(wire).unwrap(),
                BUDGET
            )
            .err()
            .unwrap()
            .code,
            "ollama_invalid_history"
        );
    }
    for status in ["failed", "incomplete"] {
        let mut wire = native().wire().clone();
        wire["status"] = status.into();
        if status == "incomplete" {
            wire["incomplete_details"] = json!({"reason":"max_output_tokens"});
        } else {
            wire["error"] = json!({"code":"native_safe_failure"});
        }
        let response = CanonicalResponse::new(wire).unwrap();
        let history =
            NativeHistory::from_response(&profile(), "native-fixture", &seed(), &response, BUDGET)
                .unwrap();
        let projection = history.to_responses(BUDGET).unwrap();
        assert_eq!(projection.state(), response.state());
        assert_eq!(
            replay(projection.output(), &seed())
                .unwrap()
                .0
                .native_response(),
            response.wire()
        );
    }
}

#[tokio::test]
async fn json_provider_replays_three_turns_from_bound_capsules_and_rejects_plain_or_tampered_history_before_keys()
 {
    let mut terminal = native().wire().clone();
    terminal["output"].as_array_mut().unwrap().pop(); // Future item kept in codec tests; no native input mapping claimed.
    let mut fixture = Fixture::start(vec![Reply::json(terminal.clone())]).await;
    let (broker, reads) = broker();
    let provider = fixture.provider(broker, true).with_native_history();
    let original = json!({"role":"user","content":"hello"});
    let mut input = vec![original.clone()];
    let mut native_input = input.clone();
    let mut first_projection = Vec::new();
    for turn in 0..3 {
        let response = provider
            .create_response(
                request(
                    json!({"model":"fixture","input":input,"instructions":"original instruction",
            "tools":[{"type":"function","name":"echo","parameters":{"type":"object"}}]}),
                ),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .response;
        let captured = fixture.request().await.body.unwrap();
        assert_eq!(captured["model"], "native-fixture");
        assert_eq!(captured["input"], json!(native_input));
        assert!(!captured.to_string().contains(PREFIX));
        if turn == 0 {
            first_projection = response.output().to_vec();
        }
        let result = json!({"type":"function_call_output","call_id":"c1","output":format!("result {turn}🙂")});
        input.extend(response.output().iter().cloned());
        input.push(result.clone());
        native_input.extend(terminal["output"].as_array().unwrap().iter().cloned());
        native_input.push(result);
    }
    for tamper in ["summary", "call_id", "prefix", "plain"] {
        let mut projected = first_projection.clone();
        let mut user = original.clone();
        match tamper {
            "summary" => projected[0]["summary"] = json!([]),
            "call_id" => projected[1]["call_id"] = "changed".into(),
            "prefix" => user["content"] = "another branch".into(),
            _ => projected[0]["encrypted_content"] = "private thought🙂".into(),
        }
        let mut input = vec![user];
        input.extend(projected);
        input.push(json!({"type":"function_call_output","call_id":"c1","output":"result"}));
        let error = provider
            .create_response(
                request(
                    json!({"model":"fixture","input":input,"instructions":"original instruction",
            "tools":[{"type":"function","name":"echo","parameters":{"type":"object"}}]}),
                ),
                RequestContext::default(),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.http_status, 400);
    }
    let mut mixed = vec![original];
    mixed.extend(first_projection);
    mixed.push(json!({"type":"function_call_output","call_id":"c1","output":"result"}));
    mixed.push(json!({"type":"reasoning","summary":[],"encrypted_content":"other-provider"}));
    assert_eq!(
        provider
            .create_response(
                request(
                    json!({"model":"fixture","input":mixed,"instructions":"original instruction",
        "tools":[{"type":"function","name":"echo","parameters":{"type":"object"}}]})
                ),
                RequestContext::default()
            )
            .await
            .err()
            .unwrap()
            .http_status,
        400
    );
    assert_eq!(reads.load(Ordering::SeqCst), 3);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 3);
}

#[test]
fn canonical_reasoning_null_content_is_absence_without_loosening_meaningful_display_binding() {
    let history = record();
    let mut input = history.to_responses(BUDGET).unwrap().output().to_vec();
    input[0]["content"] = Value::Null;
    let (restored, _) = replay(&input, &seed()).unwrap();
    assert_eq!(restored.native_response(), native().wire());
    for bad in [
        json!([]),
        json!([{"type":"reasoning_text","text":"changed"}]),
        json!("changed"),
    ] {
        let mut altered = input.clone();
        altered[0]["content"] = bad;
        assert_eq!(
            replay(&altered, &seed()).err().unwrap().code,
            "ollama_invalid_history"
        );
    }
    let mut altered = input;
    altered[1]["content"] = Value::Null;
    assert_eq!(
        replay(&altered, &seed()).err().unwrap().code,
        "ollama_invalid_history",
        "null exception belongs only to the synthetic reasoning carrier"
    );
}
