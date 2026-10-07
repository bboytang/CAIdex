use caidex_model_core::{CanonicalRequest, ResponsesDialect, StreamState};
use caidex_provider_google::{ContentStream, NativeHistory, NativeResponse};
use serde_json::{Value, json};

const LIMIT: usize = 128 * 1024;
const MODEL: &str = "models/fixture-001";
const PREFIX: &str = "caidex.google.native-history.v1:";
fn request(count: u64) -> Value {
    json!({"contents":[{"role":"user","parts":[{"text":"start"}]}],
        "systemInstruction":{"parts":[{"text":"fixed"}]},
        "tools":[{"functionDeclarations":[{"name":"echo","parameters":{"type":"object"}}]}],
        "generationConfig":{"candidateCount":count,"maxOutputTokens":128},"future":{"keep":true}})
}
fn reply(reason: &str) -> Value {
    serde_json::from_str(&format!(r#"{{"responseId":"native-id","modelVersion":"served-version","candidates":[{{"content":{{"role":"model","parts":[{{"text":"思考🙂","thought":true,"thoughtSignature":"c2ln+/==","future":true}},{{"text":"answer","thoughtSignature":"c2lnMg=="}},{{"functionCall":{{"name":"echo","args":{{"big":18446744073709551616}}}},"thoughtSignature":"c2lnMw=="}},{{"toolCall":{{"server":"opaque"}}}},{{"inlineData":{{"mimeType":"image/png","data":"AA=="}}}},{{"futurePart":{{"keep":null}}}}]}},"finishReason":"{reason}"}}],"usageMetadata":{{"promptTokenCount":10,"cachedContentTokenCount":4,"candidatesTokenCount":7,"thoughtsTokenCount":3,"toolUsePromptTokenCount":2,"totalTokenCount":20,"future":{{"cost":"0.001"}}}},"future":{{"big":18446744073709551616}}}}"#)).unwrap()
}
fn history() -> NativeHistory {
    NativeHistory::from_response(
        &NativeResponse::parse(reply("STOP")).unwrap(),
        MODEL,
        &request(1),
        Some(0),
        "run-fixture",
        LIMIT,
    )
    .unwrap()
}
fn envelope(output: &[Value]) -> Value {
    serde_json::from_str(
        output[0]["encrypted_content"]
            .as_str()
            .unwrap()
            .strip_prefix(PREFIX)
            .unwrap(),
    )
    .unwrap()
}
fn replace_envelope(output: &mut [Value], value: Value) {
    output[0]["encrypted_content"] = format!("{PREFIX}{value}").into();
}

#[test]
fn json_history_roundtrips_native_parts_and_binding_through_classic_and_lite() {
    let history = history();
    let projected = history.to_responses(LIMIT).unwrap();
    assert_eq!(projected.id(), "run-fixture");
    assert_eq!(projected.wire()["model"], MODEL);
    assert_eq!(projected.wire()["caidex_native_response_id"], "native-id");
    assert_eq!(
        projected.wire()["caidex_native_model_version"],
        "served-version"
    );
    assert_eq!(projected.output().len(), 3); // carrier, visible text, client function
    assert_eq!(
        projected.output()[0]["summary"],
        json!([{"type":"summary_text","text":"思考🙂"}])
    );
    assert_eq!(projected.output()[1]["phase"], "commentary");
    assert_eq!(projected.output()[2]["call_id"], "call_run-fixture_0_2");
    assert_eq!(
        projected.output()[2]["arguments"],
        r#"{"big":18446744073709551616}"#
    );
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let canonical =
            CanonicalRequest::new(json!({"model":"alias","input":projected.output()}), dialect)
                .unwrap();
        let stored = serde_json::to_vec(&canonical).unwrap();
        let restored: Value = serde_json::from_slice(&stored).unwrap();
        let native = NativeHistory::from_responses_output(
            restored["input"].as_array().unwrap(),
            MODEL,
            &request(1),
            LIMIT,
        )
        .unwrap();
        assert_eq!(native.native_response(), &reply("STOP"));
        assert_eq!(native.request(), &request(1));
        assert_eq!(
            native.replay_content().unwrap(),
            &reply("STOP")["candidates"][0]["content"]
        );
        assert_eq!(
            native.native_response()["future"]["big"].to_string(),
            "18446744073709551616"
        );
        assert!(!format!("{native:?} {canonical:?}").contains("c2ln"));
    }
    let mut missing_ids = reply("STOP");
    missing_ids.as_object_mut().unwrap().remove("responseId");
    missing_ids.as_object_mut().unwrap().remove("modelVersion");
    let view = NativeHistory::from_response(
        &NativeResponse::parse(missing_ids).unwrap(),
        MODEL,
        &request(1),
        Some(0),
        "local-id",
        LIMIT,
    )
    .unwrap()
    .to_responses(LIMIT)
    .unwrap();
    assert_eq!(view.id(), "local-id");
    assert!(
        view.wire()["caidex_native_response_id"].is_null()
            && view.wire()["caidex_native_model_version"].is_null()
    );
}

#[test]
fn stream_history_retains_raw_chunks_and_validates_the_derived_response_on_restore() {
    let chunks = vec![
        json!({"responseId":"stream-id","candidates":[{"content":{"role":"model","parts":[{"text":"think","thought":true,"thoughtSignature":"c2lnMQ=="}]} }],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":1},"future":true}),
        json!({"candidates":[{"content":{"role":"model","parts":[{"text":"final","thoughtSignature":"c2lnMg=="}]},"finishReason":"STOP"}],"usageMetadata":{"candidatesTokenCount":7,"thoughtsTokenCount":3},"future":null}),
        json!({"usageMetadata":{"totalTokenCount":20,"cachedContentTokenCount":4},"future":{"last":true}}),
    ];
    let mut parser = ContentStream::new(LIMIT, LIMIT, 1).unwrap();
    for chunk in &chunks {
        parser
            .push(format!("data: {chunk}\n\n").as_bytes())
            .unwrap();
    }
    parser.finish().unwrap();
    let stream = parser.completed_response().unwrap();
    let native =
        NativeHistory::from_stream(stream, MODEL, &request(1), Some(0), "run-stream", LIMIT)
            .unwrap();
    let projected = native.to_responses(LIMIT).unwrap();
    let restored =
        NativeHistory::from_responses_output(projected.output(), MODEL, &request(1), LIMIT)
            .unwrap();
    assert_eq!(restored.chunks().unwrap(), chunks);
    assert_eq!(restored.native_response(), stream.response().wire());
    assert_eq!(
        restored.replay_content().unwrap()["parts"],
        json!([
        {"text":"think","thought":true,"thoughtSignature":"c2lnMQ=="},
        {"text":"final","thoughtSignature":"c2lnMg=="}])
    );
    let usage = projected.usage().unwrap().unwrap();
    assert_eq!(
        (usage.input_tokens, usage.output_tokens, usage.total_tokens),
        (Some(10), Some(10), Some(20))
    );
    for edit in ["derived", "chunk", "truncated", "tail"] {
        let mut output = projected.output().to_vec();
        let mut capsule = envelope(&output);
        match edit {
            "derived" => {
                capsule["response"]["candidates"][0]["content"]["parts"][0]["thoughtSignature"] =
                    "edited".into()
            }
            "chunk" => {
                capsule["chunks"][0]["candidates"][0]["content"]["parts"][0]["text"] =
                    "edited".into()
            }
            "truncated" => {
                capsule["chunks"].as_array_mut().unwrap().remove(1);
            }
            "tail" => capsule["chunks"]
                .as_array_mut()
                .unwrap()
                .push(json!({"candidates":[{"content":{"parts":[{"text":"after stop"}]}}]})),
            _ => unreachable!(),
        }
        replace_envelope(&mut output, capsule);
        assert_eq!(
            NativeHistory::from_responses_output(&output, MODEL, &request(1), LIMIT)
                .unwrap_err()
                .code,
            "invalid_google_replay",
            "{edit}"
        );
    }
}

#[test]
fn only_the_selected_candidate_projects_calls_and_incomplete_outcomes_never_execute() {
    let mut wire = reply("STOP");
    wire["candidates"].as_array_mut().unwrap().push(json!({"index":1,"content":{"role":"model","parts":[{"text":"alternate"},{"functionCall":{"name":"other","id":"alternate-call","args":{}}}]},"finishReason":"STOP"}));
    let native = NativeResponse::parse(wire.clone()).unwrap();
    assert!(
        NativeHistory::from_response(&native, MODEL, &request(2), None, "selected", LIMIT).is_err()
    );
    for (index, text, name) in [(0, "answer", "echo"), (1, "alternate", "other")] {
        let selected = NativeHistory::from_response(
            &native,
            MODEL,
            &request(2),
            Some(index),
            "selected",
            LIMIT,
        )
        .unwrap();
        let projected = selected.to_responses(LIMIT).unwrap();
        assert_eq!(projected.output_text().collect::<Vec<_>>(), [text]);
        assert_eq!(projected.output()[2]["name"], name);
        let restored =
            NativeHistory::from_responses_output(projected.output(), MODEL, &request(2), LIMIT)
                .unwrap();
        assert_eq!(restored.native_response(), &wire); // both raw alternatives remain
        assert_eq!(projected.wire()["caidex_native_usage_scope"], "generation");
    }
    for (reason, state, detail) in [
        ("MAX_TOKENS", StreamState::Incomplete, "max_output_tokens"),
        ("SAFETY", StreamState::Incomplete, "content_filter"),
        ("MALFORMED_FUNCTION_CALL", StreamState::Failed, ""),
        (
            "FUTURE_REASON",
            StreamState::Incomplete,
            "unknown_provider_stop_reason",
        ),
    ] {
        let projected = NativeHistory::from_response(
            &NativeResponse::parse(reply(reason)).unwrap(),
            MODEL,
            &request(1),
            Some(0),
            "outcome",
            LIMIT,
        )
        .unwrap()
        .to_responses(LIMIT)
        .unwrap();
        assert_eq!(projected.state(), state);
        assert!(
            projected
                .output()
                .iter()
                .all(|item| item["type"] != "function_call")
        );
        assert_eq!(
            projected.wire()["incomplete_details"]["reason"]
                .as_str()
                .unwrap_or(""),
            detail
        );
    }
    let blocked =
        NativeResponse::parse(json!({"promptFeedback":{"blockReason":"SAFETY","future":true}}))
            .unwrap();
    assert!(
        NativeHistory::from_response(&blocked, MODEL, &request(1), Some(0), "blocked", LIMIT)
            .is_err()
    );
    let projected =
        NativeHistory::from_response(&blocked, MODEL, &request(1), None, "blocked", LIMIT)
            .unwrap()
            .to_responses(LIMIT)
            .unwrap();
    assert_eq!(projected.state(), StreamState::Incomplete);
    assert_eq!(projected.output().len(), 1);
    assert!(
        NativeHistory::from_responses_output(projected.output(), MODEL, &request(1), LIMIT)
            .unwrap()
            .replay_content()
            .is_none()
    );
    let mut duplicate = reply("STOP");
    duplicate["candidates"][0]["content"]["parts"] = json!([
        {"functionCall":{"name":"echo","id":"same","args":{}}},
        {"functionCall":{"name":"echo","id":"same","args":{}}}]);
    assert_eq!(
        NativeHistory::from_response(
            &NativeResponse::parse(duplicate).unwrap(),
            MODEL,
            &request(1),
            Some(0),
            "duplicate",
            LIMIT
        )
        .unwrap()
        .to_responses(LIMIT)
        .unwrap_err()
        .code,
        "google_invalid_projection"
    );
}

#[test]
fn usage_accounts_for_thoughts_once_without_adding_cached_or_tool_prompt_tokens() {
    for (raw, want) in [
        (
            json!({"promptTokenCount":10,"candidatesTokenCount":7,"thoughtsTokenCount":3,"totalTokenCount":20,"cachedContentTokenCount":4,"toolUsePromptTokenCount":2}),
            Some((10, 10, 20)),
        ),
        (
            json!({"promptTokenCount":10,"candidatesTokenCount":7,"totalTokenCount":20}),
            Some((10, 10, 20)),
        ),
        (
            json!({"promptTokenCount":10,"candidatesTokenCount":7,"thoughtsTokenCount":3}),
            Some((10, 10, 20)),
        ),
        (
            json!({"promptTokenCount":10,"candidatesTokenCount":7}),
            None,
        ),
        (json!({"totalTokenCount":20}), None),
        (Value::Null, None),
    ] {
        let mut wire = reply("STOP");
        wire["usageMetadata"] = raw.clone();
        let history = NativeHistory::from_response(
            &NativeResponse::parse(wire).unwrap(),
            MODEL,
            &request(1),
            Some(0),
            "usage",
            LIMIT,
        )
        .unwrap();
        let projected = history.to_responses(LIMIT).unwrap();
        let actual = projected.usage().unwrap().map(|u| {
            (
                u.input_tokens.unwrap(),
                u.output_tokens.unwrap(),
                u.total_tokens.unwrap(),
            )
        });
        assert_eq!(actual, want);
        assert_eq!(projected.wire()["caidex_native_usage"], raw);
        assert_eq!(
            NativeHistory::from_responses_output(projected.output(), MODEL, &request(1), LIMIT)
                .unwrap()
                .native_response()["usageMetadata"],
            raw
        );
    }
    for (raw, code) in [
        (
            json!({"promptTokenCount":10,"totalTokenCount":9}),
            "google_usage_inconsistent",
        ),
        (
            json!({"promptTokenCount":10,"candidatesTokenCount":7,"thoughtsTokenCount":3,"totalTokenCount":21}),
            "google_usage_inconsistent",
        ),
        (
            json!({"promptTokenCount":10,"totalTokenCount":20,"cachedContentTokenCount":11}),
            "google_usage_inconsistent",
        ),
        (
            json!({"promptTokenCount":1,"candidatesTokenCount":u64::MAX,"thoughtsTokenCount":1}),
            "google_usage_overflow",
        ),
    ] {
        let mut wire = reply("STOP");
        wire["usageMetadata"] = raw;
        assert_eq!(
            NativeHistory::from_response(
                &NativeResponse::parse(wire).unwrap(),
                MODEL,
                &request(1),
                Some(0),
                "usage",
                LIMIT
            )
            .unwrap()
            .to_responses(LIMIT)
            .unwrap_err()
            .code,
            code
        );
    }
}

#[test]
fn edited_partial_cross_model_or_cross_request_groups_cannot_restore_stale_native_data() {
    let projected = history().to_responses(LIMIT).unwrap();
    assert_eq!(
        NativeHistory::from_responses_output(
            projected.output(),
            "models/other",
            &request(1),
            LIMIT
        )
        .unwrap_err()
        .code,
        "google_history_model_mismatch"
    );
    let mut changed = request(1);
    changed["systemInstruction"]["parts"][0]["text"] = "other".into();
    assert_eq!(
        NativeHistory::from_responses_output(projected.output(), MODEL, &changed, LIMIT)
            .unwrap_err()
            .code,
        "google_history_request_mismatch"
    );
    for edit in [
        "text",
        "summary",
        "role",
        "phase",
        "arguments",
        "call_id",
        "name",
        "namespace",
        "removed",
        "order",
        "duplicate",
        "provider",
        "version",
        "request",
        "selection",
    ] {
        let mut output = projected.output().to_vec();
        match edit {
            "text" => output[1]["content"][0]["text"] = "edited".into(),
            "summary" => output[0]["summary"][0]["text"] = "edited".into(),
            "role" => output[1]["role"] = "user".into(),
            "phase" => output[1]["phase"] = "final_answer".into(),
            "arguments" => output[2]["arguments"] = "{}".into(),
            "call_id" => output[2]["call_id"] = "other".into(),
            "name" => output[2]["name"] = "other".into(),
            "namespace" => output[2]["namespace"] = "other".into(),
            "removed" => {
                output.pop();
            }
            "order" => output.swap(1, 2),
            "duplicate" => output.push(output[1].clone()),
            key => {
                let mut v = envelope(&output);
                match key {
                    "provider" => v[key] = "anthropic".into(),
                    "version" => v[key] = 2.into(),
                    "request" => v[key] = changed.clone(),
                    "selection" => v["candidate_index"] = 99.into(),
                    _ => unreachable!(),
                };
                replace_envelope(&mut output, v);
            }
        }
        assert!(
            NativeHistory::from_responses_output(&output, MODEL, &request(1), LIMIT).is_err(),
            "{edit}"
        );
    }
    let mut presentation = projected.output().to_vec();
    for item in &mut presentation {
        item.as_object_mut().unwrap().remove("id");
        item["status"] = "completed".into();
    }
    presentation[2]["arguments"] = " { \"big\" : 18446744073709551616 } ".into();
    assert_eq!(
        NativeHistory::from_responses_output(&presentation, MODEL, &request(1), LIMIT)
            .unwrap()
            .native_response(),
        &reply("STOP")
    );
}

#[test]
fn invalid_capsules_requests_selection_and_limits_return_only_safe_errors() {
    let projected = history().to_responses(LIMIT).unwrap();
    for capsule in [
        "private secret",
        "caidex.anthropic.native-message.v1:{}",
        "caidex.google.native-history.v2:{}",
        "caidex.google.native-history.v1:{private secret",
    ] {
        let mut output = projected.output().to_vec();
        output[0]["encrypted_content"] = capsule.into();
        let error =
            NativeHistory::from_responses_output(&output, MODEL, &request(1), LIMIT).unwrap_err();
        assert_eq!(error.code, "invalid_google_replay");
        assert!(!format!("{error:?}").contains("private secret"));
    }
    assert!(NativeHistory::from_responses_output(&[], MODEL, &request(1), LIMIT).is_err());
    assert!(
        NativeHistory::from_responses_output(projected.output(), MODEL, &request(1), 1).is_err()
    );
    assert!(history().to_responses(0).is_err());
    assert!(history().to_responses(1).is_err());
    let native = NativeResponse::parse(reply("STOP")).unwrap();
    for (name, body, index, id) in [
        ("../other", request(1), Some(0), "id"),
        (MODEL, json!({"contents":[]}), Some(0), "id"),
        (MODEL, request(1), Some(1), "id"),
        (MODEL, request(1), Some(0), ""),
        (MODEL, request(1), Some(0), "id\n"),
    ] {
        assert_eq!(
            NativeHistory::from_response(&native, name, &body, index, id, LIMIT)
                .unwrap_err()
                .code,
            "google_invalid_history"
        );
    }
    assert_eq!(
        NativeHistory::from_response(&native, MODEL, &request(1), Some(0), "id", 1)
            .unwrap_err()
            .code,
        "google_history_too_large"
    );
}
