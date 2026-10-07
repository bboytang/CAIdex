//! Native reply validation, never a tool executor or a display reconstruction.
use caidex_provider_google::{CandidateOutcome, NativeResponse};
use serde_json::{Value, json};

fn response(reason: &str) -> Value {
    json!({"responseId":"fixture-id","modelVersion":"fixture-001",
        "candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"你好"}]},"finishReason":reason}],
        "usageMetadata":{"promptTokenCount":8,"cachedContentTokenCount":2,"candidatesTokenCount":3,"thoughtsTokenCount":4,"totalTokenCount":15}})
}

#[test]
fn complete_native_reply_preserves_parts_signatures_usage_and_future_numbers() {
    let mut wire = response("STOP");
    wire["candidates"][0]["content"]["parts"] = json!([
        {"text":"思考","thought":true,"thoughtSignature":"c2lnbmF0dXJl"},
        {"functionCall":{"id":"call-1","name":"echo","args":{"text":"原文"}},"thoughtSignature":"c2ln"},
        {"toolCall":{"futureServerTool":{"opaque":true}}},
        {"futurePart":{"arbitrary":true}}
    ]);
    wire["future"] = serde_json::from_str("{\"number\":18446744073709551616}").unwrap();
    let native = NativeResponse::parse(wire.clone()).unwrap();
    assert_eq!(native.wire(), &wire);
    assert_eq!(
        native.wire()["future"]["number"].to_string(),
        "18446744073709551616"
    );
    assert_eq!(native.outcome(0), Some(CandidateOutcome::ToolCall));
    assert_eq!(native.outcome(1), None);
    assert_eq!(native.blocked_prompt(), None);
    assert!(!format!("{native:?}").contains("c2ln"));
    assert_eq!(serde_json::to_value(&native).unwrap(), wire);
}

#[test]
fn generation_end_is_distinct_from_token_limit_filtering_and_prompt_blocking() {
    for (reason, outcome) in [
        ("STOP", CandidateOutcome::Stop),
        ("MAX_TOKENS", CandidateOutcome::MaxTokens),
        ("SAFETY", CandidateOutcome::Filtered),
        ("RECITATION", CandidateOutcome::Filtered),
        ("IMAGE_SAFETY", CandidateOutcome::Filtered),
        ("MALFORMED_FUNCTION_CALL", CandidateOutcome::InvalidToolCall),
        ("UNEXPECTED_TOOL_CALL", CandidateOutcome::InvalidToolCall),
        ("FUTURE_REASON", CandidateOutcome::Unknown),
    ] {
        let native = NativeResponse::parse(response(reason)).unwrap();
        assert_eq!(native.outcome(0), Some(outcome));
        assert_eq!(native.wire()["candidates"][0]["finishReason"], reason);
    }
    for candidates in [None, Some(json!([])), Some(Value::Null)] {
        let mut wire = json!({"promptFeedback":{"blockReason":"SAFETY","future":true},"usageMetadata":{"promptTokenCount":8}});
        if let Some(candidates) = candidates {
            wire["candidates"] = candidates;
        }
        let native = NativeResponse::parse(wire.clone()).unwrap();
        assert_eq!(native.blocked_prompt(), Some("SAFETY"));
        assert!(native.candidates().is_empty());
        assert_eq!(native.wire(), &wire);
    }
    let native = NativeResponse::parse(json!({"candidates":[{"finishReason":"STOP"}]})).unwrap();
    assert_eq!(native.outcome(0), Some(CandidateOutcome::Stop));
}

#[test]
fn incomplete_malformed_or_conflicting_native_replies_are_rejected_safely() {
    let mut bad = vec![
        json!({}),
        json!([]),
        json!({"error":{"message":"SECRET_ERROR_TEXT"}}),
        json!({"promptFeedback":{"blockReason":"BLOCK_REASON_UNSPECIFIED"}}),
        json!({"candidates":{}}),
    ];
    for (path, value) in [
        ("finishReason", Value::Null),
        ("finishReason", json!("FINISH_REASON_UNSPECIFIED")),
        ("index", json!(-1)),
        ("index", json!(1.5)),
        ("content", json!([])),
    ] {
        let mut wire = response("STOP");
        wire["candidates"][0][path] = value;
        bad.push(wire);
    }
    for part in [
        json!({"thought":"true"}),
        json!({"text":3}),
        json!({"thoughtSignature":{}}),
        json!({"functionCall":{"name":"echo","args":"{}"}}),
        json!({"functionCall":{"name":""}}),
        json!({"text":"x","functionCall":{"name":"echo"}}),
        json!({"functionResponse":{"name":"echo","response":[]}}),
    ] {
        let mut wire = response("STOP");
        wire["candidates"][0]["content"]["parts"] = json!([part]);
        bad.push(wire);
    }
    let mut duplicate = response("STOP");
    let item = duplicate["candidates"][0].clone();
    duplicate["candidates"].as_array_mut().unwrap().push(item);
    bad.push(duplicate);
    let mut wrong_role = response("STOP");
    wrong_role["candidates"][0]["content"]["role"] = "user".into();
    bad.push(wrong_role);
    let mut negative = response("STOP");
    negative["usageMetadata"]["thoughtsTokenCount"] = (-1).into();
    bad.push(negative);
    let mut contradiction = response("STOP");
    contradiction["promptFeedback"] = json!({"blockReason":"SAFETY"});
    bad.push(contradiction);
    for wire in bad {
        let error = NativeResponse::parse(wire).err().unwrap();
        assert_eq!(error.http_status, 502);
        assert!(!format!("{error:?}").contains("SECRET_ERROR_TEXT"));
    }
}
