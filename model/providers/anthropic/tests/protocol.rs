//! Synthetic native wire only: no credentials, sockets or commercial API calls.
use caidex_model_core::{CapabilitySupport, EvidenceSource, ResponsesDialect};
use caidex_provider_anthropic::{
    MessageOutcome, MessageStream, ModelCatalog, ModelsPage, NativeMessage, NativeStreamState,
};
use serde_json::{Value, json};

fn message(content: Value, reason: &str) -> Value {
    json!({"id":"msg_fixture", "type":"message", "role":"assistant", "model":"native-fixture", "content":content,
        "stop_reason":reason,"stop_sequence":null,"usage":{"input_tokens":25,"output_tokens":1,"cache_read_input_tokens":3,"future_usage":{"precise":"0.001"}},
        "future":{"big":18446744073709551616_u128}})
}
fn content() -> Value {
    json!([
        {"type":"thinking", "thinking":"原生思考🙂", "signature":"sig+/=\n==", "future":"retain"},
        {"type":"redacted_thinking", "data":"opaque+/==\n", "future":{"big":18446744073709551616_u128}},
        {"type":"text", "text":"中文🙂", "citations":[{"type":"future_citation", "precise":"1.0001"}]},
        {"type":"tool_use", "id":"toolu_fixture", "name":"data_only", "input":{"number":18446744073709551616_u128,"text":"\"quoted\""}},
        {"type":"future_block", "data":{"retain":"opaque"}}
    ])
}
fn frame(wire: &Value) -> Vec<u8> {
    format!(
        "event: {}\r\ndata: {wire}\r\n\r\n",
        wire["type"].as_str().unwrap()
    )
    .into_bytes()
}
fn stream(events: &[Value]) -> Vec<u8> {
    events.iter().flat_map(frame).collect()
}
fn events() -> Vec<Value> {
    let mut start = message(json!([]), "end_turn");
    start["stop_reason"] = Value::Null;
    vec![
        json!({"type":"message_start", "message":start}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":"","future":"retain"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"原生思考🙂"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig+/="}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"\n=="}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"ping"}),
        json!({"type":"content_block_start","index":1,"content_block":content()[1]}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_start","index":2,"content_block":{"type":"text","text":"","citations":null}}),
        json!({"type":"content_block_delta","index":2,"delta":{"type":"text_delta","text":"中文🙂"}}),
        json!({"type":"content_block_delta","index":2,"delta":{"type":"citations_delta","citation":content()[2]["citations"][0]}}),
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"toolu_fixture","name":"data_only","input":{}}}),
        json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":" {\"number\": 18446744073709551616,"}}),
        json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":" \"text\":\"\\\"quoted\\\"\"} "}}),
        json!({"type":"content_block_stop","index":3}),
        json!({"type":"content_block_start","index":4,"content_block":content()[4]}),
        json!({"type":"content_block_stop","index":4}),
        json!({"type":"future_event","opaque":"retain"}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":3}}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":4,"future_cost":"0.000000000001"}}),
        json!({"type":"message_stop"}),
    ]
}
fn parser() -> MessageStream {
    MessageStream::new(8192, 128 * 1024).unwrap()
}

#[test]
fn native_reply_replay_keeps_order_signatures_redaction_tools_usage_and_future_precision() {
    let wire = message(content(), "tool_use");
    let parsed = NativeMessage::parse(wire.clone()).unwrap();
    assert_eq!(parsed.id(), "msg_fixture");
    assert_eq!(parsed.model(), "native-fixture");
    assert_eq!(parsed.outcome(), MessageOutcome::ToolUse);
    assert_eq!(serde_json::to_value(&parsed).unwrap(), wire);
    assert_eq!(
        parsed.replay_message(),
        json!({"role":"assistant","content":content()})
    );
    assert!(!format!("{parsed:?}").contains("sig+/="));
    let restored = NativeMessage::parse(
        serde_json::from_slice(&serde_json::to_vec(&parsed).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(restored.replay_message(), parsed.replay_message());
}

#[test]
fn native_input_transformations_validate_shape_and_fallback_replaces_initial_report() {
    let initial = json!([{"type":"future_transform","opaque":{"n":18446744073709551616_u128}}]);
    let final_report = json!([{"type":"thinking_dropped","path":"messages.1.content.0",
        "reason":"organization_binding_mismatch","future":"retain"}]);
    for replacement in [
        None,
        Some(final_report.clone()),
        Some(json!([])),
        Some(Value::Null),
    ] {
        let mut events = events();
        events[0]["message"]["input_transformations"] = initial.clone();
        if let Some(value) = &replacement {
            events[21]["input_transformations"] = value.clone();
        }
        let bytes = stream(&events);
        let mut parser = parser();
        for byte in bytes {
            parser.push(&[byte]).unwrap();
        }
        let native = parser.completed_message().unwrap();
        let expected = replacement
            .as_ref()
            .filter(|v| !v.is_null())
            .unwrap_or(&initial);
        assert_eq!(&native.wire()["input_transformations"], expected);
        assert_eq!(
            native.input_transformations(),
            expected.as_array().map(Vec::as_slice)
        );
        // Reporting a drop is native generation data, not a native parse error.
        assert_eq!(native.outcome(), MessageOutcome::ToolUse);
        assert!(!format!("{native:?}").contains("messages.1.content.0"));
    }
    for invalid in [
        json!({}),
        json!(false),
        json!([null]),
        json!([{}]),
        json!([{"type":"thinking_dropped","path":"messages.1.content.0"}]),
        json!([{"type":"thinking_mismatch_allowed","path":false,"reason":"prefix_binding_mismatch"}]),
    ] {
        let mut wire = message(content(), "end_turn");
        wire["input_transformations"] = invalid.clone();
        assert_eq!(
            NativeMessage::parse(wire).unwrap_err().code,
            "anthropic_invalid_message"
        );
        for at_start in [true, false] {
            let mut values = events();
            if at_start {
                values[0]["message"]["input_transformations"] = invalid.clone();
            } else {
                values[21]["input_transformations"] = invalid.clone();
            }
            let mut parser = parser();
            assert_eq!(
                parser.push(&stream(&values)).unwrap_err().code,
                "anthropic_invalid_stream"
            );
            assert!(parser.completed_message().is_none());
        }
    }
}

#[test]
fn native_stop_reasons_are_not_flattened_to_success_or_automatically_retried() {
    for (reason, outcome) in [
        ("end_turn", MessageOutcome::EndTurn),
        ("stop_sequence", MessageOutcome::StopSequence),
        ("tool_use", MessageOutcome::ToolUse),
        ("max_tokens", MessageOutcome::MaxTokens),
        (
            "model_context_window_exceeded",
            MessageOutcome::ContextWindowExceeded,
        ),
        ("pause_turn", MessageOutcome::PauseTurn),
        ("refusal", MessageOutcome::Refusal),
        ("future_reason", MessageOutcome::Unknown),
    ] {
        let wire = message(json!([]), reason);
        assert_eq!(NativeMessage::parse(wire).unwrap().outcome(), outcome);
    }
    let mut wire = message(json!([]), "end_turn");
    wire["stop_details"] = json!({"type":"refusal","future":"retain"});
    assert_eq!(
        NativeMessage::parse(wire).unwrap().outcome(),
        MessageOutcome::Refusal
    );
}

#[test]
fn malformed_native_replies_fail_without_echoing_wire_and_unsigned_thinking_is_not_replayed() {
    let base = message(content(), "end_turn");
    let mut unsigned = base.clone();
    unsigned["content"][0]["signature"] = "".into();
    let mut duplicate = base.clone();
    duplicate["content"]
        .as_array_mut()
        .unwrap()
        .push(base["content"][3].clone());
    let mut usage = base.clone();
    usage["usage"]["output_tokens"] = (-1).into();
    let mut tool = base.clone();
    tool["content"][3]["input"] = json!([]);
    let mut pending = base.clone();
    pending["stop_reason"] = Value::Null;
    for wire in [
        unsigned,
        duplicate,
        usage,
        tool,
        pending,
        json!({"error":"PRIVATE_SYNTHETIC_TEXT"}),
    ] {
        let error = NativeMessage::parse(wire).unwrap_err();
        assert_eq!(error.code, "anthropic_invalid_message");
        assert!(!format!("{error:?}").contains("PRIVATE_SYNTHETIC_TEXT"));
    }
}

#[test]
fn native_stream_is_independent_of_every_byte_boundary_and_rebuilds_the_same_replay() {
    let events = events();
    let bytes = stream(&events);
    let mut expected = message(content(), "tool_use");
    expected["usage"]["output_tokens"] = 4.into();
    expected["usage"]["future_cost"] = "0.000000000001".into();
    for split in 0..=bytes.len() {
        let mut parser = parser();
        let mut decoded = parser.push(&bytes[..split]).unwrap();
        decoded.extend(parser.push(&bytes[split..]).unwrap());
        assert_eq!(decoded.len(), events.len());
        for (decoded, raw) in decoded.iter().zip(&events) {
            assert_eq!(decoded.wire(), raw);
            assert_eq!(decoded.frame().event, decoded.kind());
        }
        assert_eq!(parser.finish().unwrap(), NativeStreamState::Completed);
        assert_eq!(parser.completed_message().unwrap().wire(), &expected);
        assert_eq!(
            parser.completed_message().unwrap().replay_message(),
            json!({"role":"assistant","content":content()})
        );
    }
    let mut parser = parser();
    for byte in bytes {
        parser.push(&[byte]).unwrap();
    }
    assert_eq!(parser.finish().unwrap(), NativeStreamState::Completed);
    assert_eq!(parser.completed_message().unwrap().wire(), &expected);
}

#[test]
fn cumulative_stream_usage_replaces_counters_and_keeps_cache_and_future_fields() {
    let mut parser = parser();
    parser.push(&stream(&events())).unwrap();
    let usage = &parser.completed_message().unwrap().wire()["usage"];
    assert_eq!(usage["output_tokens"], 4);
    assert_eq!(usage["input_tokens"], 25);
    assert_eq!(usage["cache_read_input_tokens"], 3);
    assert_eq!(usage["future_usage"]["precise"], "0.001");
    let mut decreasing = events();
    decreasing[21]["usage"]["output_tokens"] = 2.into();
    assert!(self::parser().push(&stream(&decreasing)).is_err());
}

#[test]
fn fallback_stream_records_serving_model_and_attempt_usage_at_every_byte_boundary() {
    for (initial, hops) in [
        ("primary", vec![("primary", "serving")]),
        (
            "primary",
            vec![("primary", "middle"), ("middle", "serving")],
        ),
        ("serving", vec![("primary", "serving")]),
        ("serving", vec![]), // Sticky routing can have no handoff block.
    ] {
        let mut start = message(json!([]), "end_turn");
        start["model"] = initial.into();
        start["stop_reason"] = Value::Null;
        start["usage"] = if initial == "primary" {
            json!({"input_tokens":90,"output_tokens":10,"cache_creation_input_tokens":20,"cache_read_input_tokens":30,"future":"retain"})
        } else {
            json!({"input_tokens":7,"output_tokens":0,"cache_creation_input_tokens":3,"cache_read_input_tokens":4,"future":"retain"})
        };
        let mut values = vec![json!({"type":"message_start","message":start})];
        let mut blocks = Vec::new();
        if initial == "primary" {
            blocks.push(json!({"type":"text","text":"partial primary"}));
        }
        for (from, to) in hops {
            blocks.push(json!({"type":"fallback","from":{"model":from},"to":{"model":to},"trigger":{"type":"refusal","future":"retain"},"future":18446744073709551616_u128}));
        }
        blocks.push(json!({"type":"text","text":"serving🙂"}));
        for (index, block) in blocks.iter().enumerate() {
            values.push(json!({"type":"content_block_start","index":index,"content_block":block}));
            values.push(json!({"type":"content_block_stop","index":index}));
        }
        let usage = json!({"input_tokens":7,"output_tokens":2,"cache_creation_input_tokens":3,"cache_read_input_tokens":4,
            "iterations":[{"type":"message","model":"primary","input_tokens":90},{"type":"fallback_message","model":"serving","input_tokens":7}],"future":"retain"});
        values
            .push(json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":usage}));
        values.push(json!({"type":"message_stop"}));
        let bytes = stream(&values);
        for chunk in [1, 17, bytes.len()] {
            let mut parser = parser();
            for bytes in bytes.chunks(chunk) {
                parser.push(bytes).unwrap();
            }
            let native = parser.completed_message().unwrap();
            assert_eq!(native.model(), "serving");
            assert_eq!(native.content(), blocks);
            assert_eq!(native.wire()["usage"], usage);
            assert_eq!(
                native
                    .to_responses(128 * 1024)
                    .unwrap()
                    .usage()
                    .unwrap()
                    .unwrap()
                    .total_tokens,
                Some(16)
            );
            assert_eq!(
                NativeMessage::parse(native.wire().clone()).unwrap().wire(),
                native.wire()
            );
        }
        if initial == "primary" {
            let delta = values.len() - 2;
            let mut omitted = values.clone();
            omitted[delta]["usage"] = json!({"output_tokens":2});
            let mut parser = parser();
            parser.push(&stream(&omitted)).unwrap();
            let native = parser.completed_message().unwrap();
            assert!(native.wire()["usage"].get("input_tokens").is_none());
            assert_eq!(native.wire()["usage"]["future"], "retain");
            assert_eq!(
                native
                    .to_responses(128 * 1024)
                    .unwrap()
                    .usage()
                    .unwrap()
                    .unwrap()
                    .input_tokens,
                None
            );
            // Once serving-model usage is established, its counters remain monotonic.
            values.insert(delta + 1, json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}));
            assert!(self::parser().push(&stream(&values)).is_err());
        }
    }
}

#[test]
fn malformed_fallback_shapes_chains_identity_and_deltas_cannot_complete() {
    let fallback = json!({"type":"fallback","from":{"model":"primary"},"to":{"model":"serving"}});
    for bad in [
        json!({"type":"fallback","to":{"model":"serving"}}),
        json!({"type":"fallback","from":{"model":"primary"},"to":{"model":""}}),
        json!({"type":"fallback","from":{"model":"primary"},"to":{"model":42}}),
        json!({"type":"fallback","from":{"model":"primary"},"to":{"model":"serving"},"trigger":"PRIVATE_TRIGGER"}),
    ] {
        let mut wire = message(json!([bad]), "end_turn");
        wire["model"] = "serving".into();
        let error = NativeMessage::parse(wire.clone()).unwrap_err();
        assert_eq!(error.code, "anthropic_invalid_message");
        assert!(!format!("{error:?}").contains("PRIVATE_TRIGGER"));
        wire["content"] = json!([]);
        wire["stop_reason"] = Value::Null;
        let values = [
            json!({"type":"message_start","message":wire}),
            json!({"type":"content_block_start","index":0,"content_block":bad}),
        ];
        assert!(parser().push(&stream(&values)).is_err());
    }
    let mut wrong_identity = message(json!([fallback]), "end_turn");
    wrong_identity["model"] = "primary".into();
    assert!(NativeMessage::parse(wrong_identity).is_err());
    let broken =
        json!([fallback,{"type":"fallback","from":{"model":"unrelated"},"to":{"model":"last"}}]);
    let mut wire = message(broken.clone(), "end_turn");
    wire["model"] = "last".into();
    assert!(NativeMessage::parse(wire).is_err());
    let mut start = message(json!([]), "end_turn");
    start["stop_reason"] = Value::Null;
    let prefix = [
        json!({"type":"message_start","message":start}),
        json!({"type":"content_block_start","index":0,"content_block":fallback}),
    ];
    let mut parser = parser();
    parser.push(&stream(&prefix)).unwrap();
    assert!(
        parser
            .push(&frame(
                &json!({"type":"content_block_delta","index":0,"delta":{"type":"future_delta"}})
            ))
            .is_err()
    );
    assert!(parser.completed_message().is_none());
    let mut values = prefix.to_vec();
    values.push(json!({"type":"content_block_stop","index":0}));
    values.push(json!({"type":"content_block_start","index":1,"content_block":broken[1]}));
    assert!(self::parser().push(&stream(&values)).is_err());
    // A server handoff cannot interrupt an open client/server tool block.
    values[1]["content_block"] =
        json!({"type":"tool_use","id":"open","name":"data_only","input":{}});
    values.remove(2);
    values[2]["content_block"] = fallback;
    assert!(self::parser().push(&stream(&values)).is_err());
}

#[test]
fn stream_invalid_lifecycle_identity_indexes_tools_and_missing_signature_never_complete() {
    let base = events();
    let mut duplicate_start = base.clone();
    duplicate_start.insert(1, base[0].clone());
    let mut index = base.clone();
    index[1]["index"] = 99.into();
    let mut stopped = base.clone();
    stopped.insert(6, base[2].clone());
    let mut wrong_delta = base.clone();
    wrong_delta[2]["delta"] = json!({"type":"text_delta","text":"wrong block"});
    let mut json_array = base.clone();
    json_array[14]["delta"]["partial_json"] = "[".into();
    json_array[15]["delta"]["partial_json"] = "]".into();
    let mut bad_json = base.clone();
    bad_json[15]["delta"]["partial_json"] = "broken".into();
    let mut no_signature = base.clone();
    no_signature.drain(3..5);
    let mut rewrite_id = base.clone();
    rewrite_id[20]["delta"]["id"] = "other".into();
    let mut open_block = base.clone();
    open_block.remove(18);
    let mut no_reason = base.clone();
    no_reason[20]["delta"]["stop_reason"] = Value::Null;
    no_reason[21]["delta"]["stop_reason"] = Value::Null;
    for events in [
        duplicate_start,
        index,
        stopped,
        wrong_delta,
        json_array,
        bad_json,
        no_signature,
        rewrite_id,
        open_block,
        no_reason,
    ] {
        let mut parser = parser();
        assert!(parser.push(&stream(&events)).is_err());
        assert_eq!(parser.state(), NativeStreamState::Invalid);
        assert!(parser.completed_message().is_none());
        assert!(parser.finish().is_err());
    }
    let mut parser = parser();
    assert!(
        parser
            .push(b"event: message_stop\ndata: {\"type\":\"ping\"}\n\n")
            .is_err()
    );
}

#[test]
fn ping_eof_done_error_and_cancel_cannot_synthesize_completed_messages() {
    let mut truncated = parser();
    truncated.push(&stream(&events()[..22])).unwrap();
    assert!(truncated.finish().is_err());
    assert_eq!(truncated.state(), NativeStreamState::Truncated);
    assert!(truncated.completed_message().is_none());
    let mut ping = parser();
    ping.push(&frame(&json!({"type":"ping"}))).unwrap();
    assert!(ping.finish().is_err());
    let mut error = parser();
    let decoded = error.push(&frame(&json!({"type":"error","error":{"type":"overloaded_error","message":"SYNTHETIC_PRIVATE"}}))).unwrap();
    assert_eq!(error.finish().unwrap(), NativeStreamState::Failed);
    assert!(error.completed_message().is_none());
    assert!(!format!("{:?}", decoded[0]).contains("SYNTHETIC_PRIVATE"));
    let mut cancelled = parser();
    cancelled.push(&frame(&events()[0])).unwrap();
    cancelled.cancel();
    assert_eq!(cancelled.finish().unwrap(), NativeStreamState::Cancelled);
    assert!(cancelled.push(&stream(&events())).is_err());
    let mut done = parser();
    assert!(done.push(b"data: [DONE]\n\n").is_err());
    let mut unfinished = stream(&events());
    unfinished.truncate(unfinished.len() - 4);
    let mut parser = parser();
    parser.push(&unfinished).unwrap();
    assert!(parser.finish().is_err());
}

#[test]
fn future_events_and_blocks_remain_opaque_but_unknown_deltas_cannot_claim_lossless_replay() {
    let base = events();
    let mut parser = parser();
    parser.push(&stream(&base[..2])).unwrap();
    let unknown = json!({"type":"content_block_delta","index":0,"delta":{"type":"future_delta","opaque":"retain"}});
    assert_eq!(parser.push(&frame(&unknown)).unwrap()[0].wire(), &unknown);
    let error = parser
        .push(&frame(&json!({"type":"content_block_stop","index":0})))
        .unwrap_err();
    assert_eq!(error.code, "anthropic_unsupported_delta");
    assert!(parser.completed_message().is_none());
}

#[test]
fn whole_stream_and_frame_limits_bad_utf8_and_post_terminal_events_fail_closed() {
    assert!(MessageStream::new(0, 1).is_err());
    assert!(MessageStream::new(1, 0).is_err());
    let bytes = stream(&events());
    let mut small = MessageStream::new(8192, bytes.len() - 1).unwrap();
    assert_eq!(
        small.push(&bytes).unwrap_err().code,
        "anthropic_stream_too_large"
    );
    assert!(
        MessageStream::new(8, 8192)
            .unwrap()
            .push(&frame(&events()[0]))
            .is_err()
    );
    assert!(parser().push(b"data: \xff\n\n").is_err());
    let mut terminal = parser();
    terminal.push(&bytes).unwrap();
    assert!(terminal.push(&frame(&json!({"type":"ping"}))).is_err());
    assert!(terminal.completed_message().is_none());
}

fn native_model(id: &str) -> Value {
    json!({"type":"model","id":id,"created_at":"2026-01-01T00:00:00Z","display_name":"Synthetic native model",
        "capabilities":{"image_input":{"supported":true},"thinking":{"supported":false},"structured_outputs":{"supported":true},"future":{"opaque":"retain"}},
        "max_input_tokens":200000,"max_tokens":8192,"line":"future-line","future":{"big":18446744073709551616_u128}})
}
fn page(models: Vec<Value>, more: bool) -> Value {
    json!({"first_id":models.first().map(|m| m["id"].clone()),"last_id":models.last().map(|m| m["id"].clone()),"data":models,"has_more":more})
}

#[test]
fn native_catalog_declares_only_documented_capabilities_and_never_codex_compatibility() {
    let raw = native_model("native-fixture");
    let page = ModelsPage::parse(page(vec![raw.clone()], false)).unwrap();
    let native = &page.models()[0];
    assert_eq!(native.wire(), &raw);
    let metadata = native
        .metadata("alias", vec![ResponsesDialect::Classic])
        .unwrap();
    assert_eq!(metadata.native_model, "native-fixture");
    assert_eq!(metadata.source, EvidenceSource::ProviderCatalog);
    assert_eq!(metadata.capabilities.vision, CapabilitySupport::Supported);
    assert_eq!(
        metadata.capabilities.reasoning,
        CapabilitySupport::Unsupported
    );
    assert_eq!(
        metadata.capabilities.structured_output,
        CapabilitySupport::Supported
    );
    assert_eq!(metadata.capabilities.streaming, CapabilitySupport::Unknown);
    assert_eq!(
        metadata.capabilities.native_tools,
        CapabilitySupport::Unknown
    );
    assert_eq!(metadata.capabilities.context_window, Some(200000));
    assert_eq!(metadata.capabilities.output_limit, Some(8192));
    assert_eq!(metadata.codex_compatibility, None);
    let mut legacy = raw;
    legacy.as_object_mut().unwrap().remove("capabilities");
    legacy["max_input_tokens"] = Value::Null;
    legacy["max_tokens"] = 0.into();
    let page = ModelsPage::parse(self::page(vec![legacy], false)).unwrap();
    let metadata = page.models()[0]
        .metadata("alias", vec![ResponsesDialect::Classic])
        .unwrap();
    assert_eq!(metadata.capabilities.vision, CapabilitySupport::Unknown);
    assert_eq!(metadata.capabilities.context_window, None);
    assert_eq!(metadata.capabilities.output_limit, None);
}

#[test]
fn catalog_pagination_is_bounded_deterministic_and_rejects_duplicates_and_unfinished_pages() {
    let mut catalog = ModelCatalog::new(2).unwrap();
    let first = ModelsPage::parse(page(vec![native_model("z")], true)).unwrap();
    assert_eq!(first.next_after_id(), Some("z"));
    assert_eq!(catalog.append(first).unwrap(), Some("z".into()));
    assert_eq!(
        catalog
            .append(ModelsPage::parse(page(vec![native_model("a")], false)).unwrap())
            .unwrap(),
        None
    );
    assert_eq!(
        catalog
            .finish()
            .unwrap()
            .iter()
            .map(|m| m.id())
            .collect::<Vec<_>>(),
        ["a", "z"]
    );
    let mut catalog = ModelCatalog::new(2).unwrap();
    catalog
        .append(ModelsPage::parse(page(vec![native_model("a")], true)).unwrap())
        .unwrap();
    assert!(
        catalog
            .append(ModelsPage::parse(page(vec![native_model("a")], false)).unwrap())
            .is_err()
    );
    assert!(catalog.finish().is_err());
    let mut small = ModelCatalog::new(1).unwrap();
    assert!(
        small
            .append(
                ModelsPage::parse(page(vec![native_model("a"), native_model("b")], false)).unwrap()
            )
            .is_err()
    );
    let mut empty = ModelCatalog::new(1).unwrap();
    empty
        .append(ModelsPage::parse(page(vec![], false)).unwrap())
        .unwrap();
    assert!(empty.finish().unwrap().is_empty());
    assert!(ModelCatalog::new(0).is_err());
    assert!(ModelCatalog::new(1).unwrap().finish().is_err());
}

#[test]
fn malformed_catalog_cursors_types_capabilities_and_limits_are_rejected() {
    let mut bad_cursor = page(vec![native_model("a")], true);
    bad_cursor["last_id"] = "different".into();
    let mut bad_capability = native_model("a");
    bad_capability["capabilities"]["thinking"]["supported"] = "true".into();
    let mut bad_limit = native_model("a");
    bad_limit["max_tokens"] = (-1).into();
    for raw in [
        bad_cursor,
        page(vec![], true),
        page(vec![native_model("a"), native_model("a")], false),
        page(vec![bad_capability], false),
        page(vec![bad_limit], false),
        json!({"data":[]}),
    ] {
        assert!(ModelsPage::parse(raw).is_err());
    }
}
