use caidex_model_core::{ResponsesStream, StreamEvent};
use caidex_provider_anthropic::{
    MessageStream, NativeMessage, NativeStreamEvent, ResponsesProjection, ToolMap,
};
use serde_json::{Value, json};
const LIMIT: usize = 256 * 1024;
fn frame(wire: &Value) -> String {
    format!(
        "event: {}\ndata: {wire}\n\n",
        wire["type"].as_str().unwrap()
    )
}
fn events(stop: &str, tools: &ToolMap) -> Vec<Value> {
    let aliases: Vec<_> = tools
        .native_tools()
        .iter()
        .map(|t| t["name"].clone())
        .collect();
    let mut events = vec![
        json!({"type":"message_start","message":{"id":"native-stream","type":"message","role":"assistant","model":"native","content":[],"stop_reason":null,"usage":{"input_tokens":1,"output_tokens":0}}}),
    ];
    let blocks = [
        json!({"type":"thinking","thinking":"","signature":""}),
        json!({"type":"text","text":""}),
        json!({"type":"tool_use","id":"function-one","name":aliases[0],"input":{}}),
        json!({"type":"tool_use","id":"custom-one","name":aliases[1],"input":{}}),
        json!({"type":"server_tool_use","id":"server","name":"native_only","input":{}}),
        json!({"type":"redacted_thinking","data":"opaque"}),
        json!({"type":"future","opaque":"retain"}),
    ];
    for (i, block) in blocks.into_iter().enumerate() {
        events.push(json!({"type":"content_block_start","index":i,"content_block":block}));
        let deltas = match i {
            0 => vec![
                json!({"type":"thinking_delta","thinking":"摘要🙂"}),
                json!({"type":"signature_delta","signature":"private-signature"}),
            ],
            1 => vec![
                json!({"type":"text_delta","text":"中文"}),
                json!({"type":"text_delta","text":"🙂"}),
                json!({"type":"citations_delta","citation":{"type":"future","reference":"native"}}),
            ],
            2 => vec![
                json!({"type":"input_json_delta","partial_json":"{ \"n\":18446744073709551616"}),
                json!({"type":"input_json_delta","partial_json":" }"}),
            ],
            3 => vec![
                json!({"type":"input_json_delta","partial_json":"{\"input\":\"\\ncustom🙂\\n\"}"}),
            ],
            _ => vec![],
        };
        for delta in deltas {
            events.push(json!({"type":"content_block_delta","index":i,"delta":delta}));
        }
        events.push(json!({"type":"content_block_stop","index":i}));
    }
    events.push(
        json!({"type":"message_delta","delta":{"stop_reason":stop},"usage":{"output_tokens":7}}),
    );
    events.push(json!({"type":"message_stop"}));
    events
}
fn tools() -> ToolMap {
    ToolMap::new(
        &[
            json!({"type":"function","name":"exec","parameters":{"type":"object"}}),
            json!({"type":"custom","name":"patch"}),
        ],
        10,
    )
    .unwrap()
}
fn run(stop: &str, chunk: usize) -> (Vec<StreamEvent>, NativeMessage) {
    let events = events(stop, &tools());
    let body = events.iter().map(frame).collect::<String>();
    let mut parser = MessageStream::new(LIMIT, LIMIT).unwrap();
    let mut projection = ResponsesProjection::new("native".into(), tools(), LIMIT).unwrap();
    let mut output = Vec::new();
    for chunk in body.as_bytes().chunks(chunk) {
        for event in parser.push(chunk).unwrap() {
            output.extend(projection.push(NativeStreamEvent::Event(event)).unwrap());
        }
    }
    let native = parser.completed_message().unwrap().clone();
    output.extend(
        projection
            .push(NativeStreamEvent::Completed(native.clone()))
            .unwrap(),
    );
    (output, native)
}
#[test]
fn deltas_stream_before_completion_and_native_history_and_tools_roundtrip_at_every_split() {
    let (expected, native) = run("tool_use", usize::MAX);
    for chunk in [1, 2, 3, 17, 73, 512] {
        let (actual, _) = run("tool_use", chunk);
        assert_eq!(
            actual.iter().map(|e| &e.frame).collect::<Vec<_>>(),
            expected.iter().map(|e| &e.frame).collect::<Vec<_>>()
        );
    }
    let terminal = expected.last().unwrap().response.wire();
    assert_eq!(terminal["type"], "response.completed");
    let restored = NativeMessage::from_responses_output(
        terminal["response"]["output"].as_array().unwrap(),
        "native",
        LIMIT,
    )
    .unwrap();
    assert_eq!(restored.wire(), native.wire());
    let calls = terminal["response"]["output"].as_array().unwrap();
    assert_eq!(calls[2]["arguments"], "{ \"n\":18446744073709551616 }");
    assert_eq!(calls[3]["input"], "\ncustom🙂\n");
    assert_eq!(calls.len(), 4); // server tools/redaction/future blocks remain opaque
    let text = expected
        .iter()
        .filter_map(|e| e.response.text_delta())
        .collect::<String>();
    assert_eq!(text, "中文🙂");
    assert!(
        expected
            .iter()
            .filter(|e| e.response.kind().ends_with("delta"))
            .all(|e| !e.frame.data.contains("private-signature"))
    );
    let first_done = expected
        .iter()
        .position(|e| e.response.kind() == "response.output_item.done")
        .unwrap();
    assert_eq!(expected[first_done].response.wire()["output_index"], 0);
    assert!(
        expected[..first_done]
            .iter()
            .any(|e| e.response.kind() == "response.function_call_arguments.delta")
    );
    let mut validation = ResponsesStream::new(LIMIT).unwrap();
    for (i, event) in expected.iter().enumerate() {
        assert_eq!(event.response.wire()["sequence_number"], i as u64);
        validation
            .push(frame(event.response.wire()).as_bytes())
            .unwrap();
    }
    assert_eq!(
        validation.finish().unwrap(),
        caidex_model_core::StreamState::Completed
    );
}
#[test]
fn native_limits_pause_and_refusal_are_not_relabelled_as_successful_tasks() {
    for (stop, status, reason) in [
        ("max_tokens", "incomplete", Some("max_output_tokens")),
        ("pause_turn", "incomplete", Some("provider_pause_turn")),
        (
            "model_context_window_exceeded",
            "incomplete",
            Some("context_window_exceeded"),
        ),
        ("refusal", "completed", None),
    ] {
        let (events, _) = run(stop, 73);
        let response = &events.last().unwrap().response.wire()["response"];
        assert_eq!(response["status"], status);
        assert_eq!(response["caidex_native_stop_reason"], stop);
        assert_eq!(response["incomplete_details"]["reason"].as_str(), reason);
    }
}
#[test]
fn wrong_model_and_mismatched_completion_fail_without_fabricating_done() {
    let values = events("tool_use", &tools());
    let body = values.iter().map(frame).collect::<String>();
    let mut parser = MessageStream::new(LIMIT, LIMIT).unwrap();
    let mut projection = ResponsesProjection::new("wrong".into(), tools(), LIMIT).unwrap();
    let mut parsed = parser.push(body.as_bytes()).unwrap();
    assert!(
        projection
            .push(NativeStreamEvent::Event(parsed.remove(0)))
            .is_err()
    );
    let mut projection = ResponsesProjection::new("native".into(), tools(), LIMIT).unwrap();
    let mut parser = MessageStream::new(LIMIT, LIMIT).unwrap();
    for event in parser.push(body.as_bytes()).unwrap() {
        let outputs = projection.push(NativeStreamEvent::Event(event)).unwrap();
        assert!(
            outputs
                .iter()
                .all(|e| e.response.kind() != "response.output_item.done"
                    && e.response.terminal().is_none())
        );
    }
    let mut edited = parser.completed_message().unwrap().wire().clone();
    edited["content"][1]["text"] = "changed".into();
    assert!(
        projection
            .push(NativeStreamEvent::Completed(
                NativeMessage::parse(edited).unwrap()
            ))
            .is_err()
    );
    assert!(
        projection
            .push(NativeStreamEvent::Completed(
                parser.completed_message().unwrap().clone()
            ))
            .is_err()
    );
    assert!(!format!("{projection:?}").contains("private-signature"));
}

#[test]
fn projection_budgets_and_post_terminal_events_never_emit_false_completion() {
    assert!(ResponsesProjection::new("native".into(), tools(), 0).is_err());
    assert!(ResponsesProjection::new("native".repeat(100), tools(), 100).is_err());
    assert!(ResponsesProjection::new("native".into(), tools(), 10).is_err());
    let values = events("tool_use", &tools());
    let body = values.iter().map(frame).collect::<String>();
    let mut parser = MessageStream::new(LIMIT, LIMIT).unwrap();
    let mut projection = ResponsesProjection::new("native".into(), tools(), 500).unwrap();
    let mut failed = false;
    for event in parser.push(body.as_bytes()).unwrap() {
        match projection.push(NativeStreamEvent::Event(event)) {
            Ok(output) => assert!(output.iter().all(|e| e.response.terminal().is_none()
                && e.response.kind() != "response.output_item.done")),
            Err(_) => {
                failed = true;
                break;
            }
        }
    }
    assert!(failed);
    assert!(
        projection
            .push(NativeStreamEvent::Completed(
                parser.completed_message().unwrap().clone()
            ))
            .is_err()
    );
}
