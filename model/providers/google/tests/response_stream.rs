use caidex_model_core::{ResponsesStream, StreamEvent, StreamState};
use caidex_provider_google::{
    ContentStream, NativeHistory, NativeStreamEvent, ResponsesProjection, ToolMap,
};
use serde_json::{Value, json};

const MODEL: &str = "models/fixture-projection";
const LIMIT: usize = 128 * 1024;
fn tools() -> ToolMap {
    ToolMap::new(
        &[json!({"type":"namespace","name":"local","tools":[
        {"type":"function","name":"echo","parameters":{"type":"object"}},
        {"type":"custom","name":"raw","format":{"type":"text"}}]})],
        8,
    )
    .unwrap()
}
fn request(tools: &ToolMap) -> Value {
    json!({"generationConfig":{"maxOutputTokens":128},"contents":[{"role":"user","parts":[{"text":"question"}]}],"tools":[{"functionDeclarations":tools.native_tools()}]})
}
fn chunk(parts: Value, reason: Option<&str>) -> Value {
    let mut wire = json!({"candidates":[{"index":0,"content":{"role":"model","parts":parts}}]});
    if let Some(reason) = reason {
        wire["candidates"][0]["finishReason"] = reason.into();
    }
    wire
}
fn frames(chunks: &[Value]) -> Vec<u8> {
    chunks
        .iter()
        .flat_map(|v| format!("data: {v}\n\n").into_bytes())
        .collect()
}
fn projection(limit: usize) -> ResponsesProjection {
    let tools = tools();
    ResponsesProjection::new(
        MODEL.into(),
        request(&tools),
        tools,
        "fixture".into(),
        limit,
    )
    .unwrap()
}
fn run(
    chunks: &[Value],
    split: usize,
) -> (
    Vec<StreamEvent>,
    caidex_provider_google::NativeStreamResponse,
) {
    let mut parser = ContentStream::new(LIMIT, LIMIT, 1).unwrap();
    let mut projection = projection(LIMIT);
    let mut output = Vec::new();
    for bytes in frames(chunks).chunks(split) {
        for event in parser.push(bytes).unwrap() {
            output.extend(projection.push(NativeStreamEvent::Event(event)).unwrap());
        }
    }
    parser.finish().unwrap();
    let native = parser.completed_response().unwrap().clone();
    output.extend(
        projection
            .push(NativeStreamEvent::Completed(native.clone()))
            .unwrap(),
    );
    (output, native)
}

// Catches buffering ordinary text, early executable calls/signatures, misplaced
// indices after opaque Parts, and loss of late native metadata or signed chunks.
#[test]
fn text_and_summaries_stream_before_eof_and_tools_wait_for_validated_history() {
    let tools = tools();
    let chunks = vec![
        chunk(
            json!([{"thought":true,"text":"summary","thoughtSignature":"PRIVATE_THOUGHT"},{"text":"Hi ","thoughtSignature":"PRIVATE_TEXT"}]),
            None,
        ),
        chunk(
            json!([
            {"functionCall":{"name":tools.native_tools()[0]["name"],"args":{"n":18446744073709551616_u128}},"thoughtSignature":"PRIVATE_CALL"},
            {"futurePart":{"keep":true}},
            {"functionCall":{"name":tools.native_tools()[1]["name"],"id":"native-custom","args":{"input":"  原文🙂\n  "}}}]),
            None,
        ),
        chunk(
            json!([{"text":"after","thoughtSignature":"PRIVATE_AFTER"}]),
            Some("STOP"),
        ),
        json!({"responseId":"late-id","modelVersion":"actual-version","usageMetadata":{"promptTokenCount":8,"candidatesTokenCount":3,"totalTokenCount":11,"future":{"keep":true}}}),
    ];
    let mut parser = ContentStream::new(LIMIT, LIMIT, 1).unwrap();
    let mut projection = projection(LIMIT);
    let mut before = Vec::new();
    for (i, wire) in chunks.iter().enumerate() {
        for event in parser.push(&frames(std::slice::from_ref(wire))).unwrap() {
            let emitted = projection.push(NativeStreamEvent::Event(event)).unwrap();
            if i == 0 {
                assert_eq!(
                    emitted
                        .iter()
                        .filter_map(|e| e.response.text_delta())
                        .collect::<String>(),
                    "Hi "
                );
                assert!(
                    emitted
                        .iter()
                        .any(|e| e.response.kind() == "response.reasoning_summary_text.delta")
                );
            } else {
                assert!(
                    !emitted
                        .iter()
                        .any(|e| e.response.kind() == "response.output_text.delta")
                );
            }
            before.extend(emitted);
        }
    }
    assert!(!before.is_empty());
    assert!(before.iter().all(|e| !e.response.kind().ends_with(".done")
        && e.response.terminal().is_none()
        && !e.frame.data.contains("PRIVATE_")
        && !e.frame.data.contains("encrypted_content")
        && !e.frame.data.contains("function_call")));
    parser.finish().unwrap();
    let native = parser.completed_response().unwrap().clone();
    let mut all = before;
    all.extend(
        projection
            .push(NativeStreamEvent::Completed(native.clone()))
            .unwrap(),
    );
    let terminal = all.last().unwrap().response.wire();
    assert_eq!(terminal["type"], "response.completed");
    assert_eq!(terminal["response"]["id"], "fixture");
    assert_eq!(terminal["response"]["caidex_native_response_id"], "late-id");
    assert_eq!(
        terminal["response"]["caidex_native_model_version"],
        "actual-version"
    );
    let output = terminal["response"]["output"].as_array().unwrap();
    assert_eq!(
        output
            .iter()
            .map(|v| v["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "rs_fixture_native",
            "msg_fixture_0_1",
            "fc_fixture_0_2",
            "fc_fixture_0_4",
            "msg_fixture_0_5"
        ]
    );
    assert_eq!(
        output[0]["summary"],
        json!([{"type":"summary_text","text":"summary"}])
    );
    assert_eq!(output[2]["type"], "function_call");
    assert_eq!(output[2]["name"], "echo");
    assert_eq!(output[2]["namespace"], "local");
    assert_eq!(output[2]["call_id"], "call_fixture_0_2");
    assert_eq!(output[2]["arguments"], r#"{"n":18446744073709551616}"#);
    assert_eq!(output[3]["type"], "custom_tool_call");
    assert_eq!(output[3]["call_id"], "native-custom");
    assert_eq!(output[3]["input"], "  原文🙂\n  ");
    let history =
        NativeHistory::from_responses_output(output, MODEL, &request(&tools), LIMIT).unwrap();
    assert_eq!(history.chunks().unwrap(), chunks);
    assert_eq!(history.native_response(), native.response().wire());
    assert_eq!(
        all.iter()
            .filter_map(|e| e.response.text_delta())
            .collect::<String>(),
        "Hi after"
    );
    let mut validation = ResponsesStream::new(LIMIT).unwrap();
    for (i, event) in all.iter().enumerate() {
        assert_eq!(event.response.wire()["sequence_number"], i as u64);
        validation
            .push(
                format!(
                    "event: {}\ndata: {}\n\n",
                    event.frame.event, event.frame.data
                )
                .as_bytes(),
            )
            .unwrap();
        if event.response.kind() == "response.output_item.done" {
            assert_eq!(
                &output[event.response.wire()["output_index"].as_u64().unwrap() as usize],
                &event.response.wire()["item"]
            );
        }
    }
    assert_eq!(validation.finish().unwrap(), StreamState::Completed);
    for split in [1, 2, 17, 73, 512] {
        let (actual, _) = run(&chunks, split);
        assert_eq!(
            actual.iter().map(|e| &e.frame).collect::<Vec<_>>(),
            all.iter().map(|e| &e.frame).collect::<Vec<_>>()
        );
    }
    assert!(
        projection
            .push(NativeStreamEvent::Completed(native))
            .is_err()
    );
}

// Catches releasing calls for unsuccessful finish reasons or reporting success
// for blocked prompts; texts after a suppressed call still need correct indices.
#[test]
fn unsuccessful_or_blocked_generations_keep_native_status_without_executable_calls() {
    let tools = tools();
    for (reason, status, kind, detail) in [
        (
            "MAX_TOKENS",
            "incomplete",
            "response.incomplete",
            Some("max_output_tokens"),
        ),
        (
            "SAFETY",
            "incomplete",
            "response.incomplete",
            Some("content_filter"),
        ),
        (
            "FUTURE_REASON",
            "incomplete",
            "response.incomplete",
            Some("unknown_provider_stop_reason"),
        ),
        ("MALFORMED_FUNCTION_CALL", "failed", "response.failed", None),
    ] {
        let chunks = vec![chunk(
            json!([
            {"functionCall":{"name":tools.native_tools()[0]["name"],"args":{}}},
            {"text":"partial"},{"thought":true,"text":"summary"}]),
            Some(reason),
        )];
        let (all, _) = run(&chunks, 17);
        assert!(all.iter().all(|e| !matches!(
            e.response.wire()["item"]["type"].as_str(),
            Some("function_call" | "custom_tool_call")
        )));
        let final_wire = all.last().unwrap().response.wire();
        assert_eq!(final_wire["type"], kind);
        assert_eq!(final_wire["response"]["status"], status);
        assert_eq!(
            final_wire["response"]["incomplete_details"]["reason"].as_str(),
            detail
        );
        assert_eq!(
            final_wire["response"]["output"].as_array().unwrap().len(),
            2
        );
        assert_eq!(final_wire["response"]["output"][1]["id"], "msg_fixture_0_1");
    }
    let blocked = json!({"promptFeedback":{"blockReason":"SAFETY"},"usageMetadata":{"promptTokenCount":8,"totalTokenCount":8}});
    let (all, native) = run(std::slice::from_ref(&blocked), 1);
    assert_eq!(all.last().unwrap().response.kind(), "response.incomplete");
    let output = all.last().unwrap().response.wire()["response"]["output"]
        .as_array()
        .unwrap();
    assert_eq!(output.len(), 1);
    let history =
        NativeHistory::from_responses_output(output, MODEL, &request(&tools), LIMIT).unwrap();
    assert_eq!(history.native_response(), &blocked);
    assert_eq!(native.response().blocked_prompt(), Some("SAFETY"));
}

// Catches accepting a completion from another stream or losing unknown/signature
// fields when the visible text is identical; failure permanently closes projection.
#[test]
fn completion_is_bound_to_all_observed_native_chunks() {
    let first = chunk(
        json!([{"text":"same","thoughtSignature":"ORIGINAL","future":{"keep":true}}]),
        Some("STOP"),
    );
    for changed in [
        chunk(
            json!([{"text":"same","thoughtSignature":"EDITED","future":{"keep":true}}]),
            Some("STOP"),
        ),
        chunk(
            json!([{"text":"same","thoughtSignature":"ORIGINAL","future":{"keep":false}}]),
            Some("STOP"),
        ),
    ] {
        let mut a = ContentStream::new(LIMIT, LIMIT, 1).unwrap();
        let mut projection = projection(LIMIT);
        for event in a.push(&frames(std::slice::from_ref(&first))).unwrap() {
            projection.push(NativeStreamEvent::Event(event)).unwrap();
        }
        let mut b = ContentStream::new(LIMIT, LIMIT, 1).unwrap();
        b.push(&frames(&[changed])).unwrap();
        b.finish().unwrap();
        let error = projection
            .push(NativeStreamEvent::Completed(
                b.completed_response().unwrap().clone(),
            ))
            .unwrap_err();
        assert_eq!(error.code, "google_invalid_projection_stream");
        assert!(!format!("{error:?}").contains("ORIGINAL"));
        assert!(
            projection
                .push(NativeStreamEvent::Completed(
                    b.completed_response().unwrap().clone()
                ))
                .is_err()
        );
    }
}

// Catches unchecked routes/tool bindings, candidate selection and projection
// expansion beyond the bound; neither failure may synthesize a terminal event.
#[test]
fn invalid_configuration_and_byte_limits_reject_without_completed_output() {
    for (model, id, bytes) in [
        ("bad/route", "fixture", LIMIT),
        (MODEL, "bad\nID", LIMIT),
        (MODEL, "", LIMIT),
        (MODEL, "fixture", 0),
    ] {
        let tools = tools();
        assert!(
            ResponsesProjection::new(model.into(), request(&tools), tools, id.into(), bytes)
                .is_err()
        );
    }
    for edit in 0..3 {
        let tools = tools();
        let mut wire = request(&tools);
        match edit {
            0 => wire["tools"] = json!([]),
            1 => wire["generationConfig"]["candidateCount"] = 2.into(),
            _ => wire["systemInstruction"] = json!({"parts":[{"text":"x".repeat(LIMIT)}]}),
        }
        assert!(
            ResponsesProjection::new(MODEL.into(), wire, tools, "fixture".into(), LIMIT).is_err()
        );
    }
    let mut projection = projection(1024);
    let mut parser = ContentStream::new(LIMIT, LIMIT, 1).unwrap();
    let events = parser
        .push(&frames(&[chunk(
            json!([{"text":"x".repeat(2048)}]),
            Some("STOP"),
        )]))
        .unwrap();
    assert!(
        projection
            .push(NativeStreamEvent::Event(events.into_iter().next().unwrap()))
            .is_err()
    );
}
