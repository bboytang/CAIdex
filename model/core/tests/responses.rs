use caidex_model_core::{
    CanonicalRequest, CanonicalResponse, Error, ResponseEvent, ResponseItem, ResponsesDialect,
    ResponsesStream, StreamState, ToolInput, ToolKind, Usage,
};
use serde_json::json;

#[test]
fn classic_request_keeps_tools_images_structured_output_and_extensions() {
    let wire = json!({
        "model": "fixture-classic", "stream": true, "store": false,
        "instructions": "fixture instructions", "tools": [{"type": "custom", "name": "apply_patch", "format": {"type": "grammar", "syntax": "lark", "definition": "opaque-grammar"}}],
        "input": [{"role": "user", "content": [{"type": "input_image", "image_url": "data:image/png;base64,fixture", "detail": "original"}]}],
        "reasoning": {"effort": "high", "future": [1, 2]},
        "include": ["reasoning.encrypted_content"],
        "text": {"format": {"type": "json_schema", "schema": {"type": "object"}}},
        "future": {"integer": 9007199254740993_u64, "signature": "fixture-signature=="}
    });
    let request = CanonicalRequest::new(wire.clone(), ResponsesDialect::Classic).unwrap();
    assert_eq!(request.model(), "fixture-classic");
    assert!(request.is_streaming());
    assert_eq!(request.dialect().lite_header(), None);
    assert_eq!(request.wire(), &wire);
    assert_eq!(serde_json::to_value(request).unwrap(), wire);
}

#[test]
fn lite_request_keeps_developer_prefix_ids_namespaces_and_transport_header() {
    let wire = json!({
        "model": "fixture-lite", "stream": true, "parallel_tool_calls": false,
        "reasoning": {"context": "all_turns"},
        "input": [
            {"id": "at_stable", "type": "additional_tools", "role": "developer", "tools": [{"type": "namespace", "name": "functions", "tools": [{"type": "custom", "name": "exec"}]}]},
            {"id": "msg_stable", "type": "message", "role": "developer", "content": [{"type": "input_text", "text": "fixture instructions"}], "internal_chat_message_metadata_passthrough": {"content_item_kinds": ["model.base_instructions"]}},
            {"type": "configuration_update", "reasoning_effort": "high"}
        ], "client_metadata": {"x-codex-turn-metadata": "opaque-json-string"}
    });
    let request = CanonicalRequest::new(wire.clone(), ResponsesDialect::Lite).unwrap();
    assert_eq!(
        request.dialect().lite_header(),
        Some(("x-openai-internal-codex-responses-lite", "true"))
    );
    assert_eq!(serde_json::to_value(request).unwrap(), wire);
    for field in ["instructions", "tools"] {
        let mut mixed = wire.clone();
        mixed[field] = json!(null);
        assert_eq!(
            CanonicalRequest::new(mixed, ResponsesDialect::Lite).unwrap_err(),
            Error::InvalidRequest
        );
    }
    for invalid in [
        json!(null),
        json!({"model": "", "input": []}),
        json!({"model":"fixture", "input": null}),
        json!({"model":"fixture", "input": [], "stream": "true"}),
    ] {
        assert!(matches!(
            CanonicalRequest::new(invalid, ResponsesDialect::Classic),
            Err(Error::InvalidRequest)
        ));
    }
    assert!(
        CanonicalRequest::new(
            json!({"model":"fixture", "input": "hello"}),
            ResponsesDialect::Classic
        )
        .is_ok()
    );
    assert!(
        CanonicalRequest::new(
            json!({"model":"fixture", "input": "hello"}),
            ResponsesDialect::Lite
        )
        .is_err()
    );
}

#[test]
fn tool_calls_normalize_identity_without_parsing_or_reencoding_arguments() {
    let arguments = "{ \"command\" : \"echo \\\"你好\\\"\", \"number\": 1.00 }";
    let raw = json!({"type": "function_call", "id": "fc_fixture", "call_id": "call_fixture", "namespace": "functions", "name": "shell", "arguments": arguments, "encrypted_function_args": ["fixture-cipher=="], "extension": true});
    let item = ResponseItem::new(raw.clone()).unwrap();
    let call = item.tool_call().unwrap().unwrap();
    assert_eq!(call.kind, ToolKind::Function);
    assert_eq!(call.call_id, "call_fixture");
    assert_eq!(call.namespace, Some("functions"));
    assert!(call.input == ToolInput::JsonArguments(arguments));
    assert_eq!(serde_json::to_value(&item).unwrap(), raw);
    let patch = "*** Begin Patch\n*** Add File: fixture\n+你好\n*** End Patch";
    let item = ResponseItem::new(json!({"type":"custom_tool_call", "call_id":"patch_fixture", "name":"apply_patch", "input":patch})).unwrap();
    let call = item.tool_call().unwrap().unwrap();
    assert_eq!(call.kind, ToolKind::Custom);
    assert!(call.input == ToolInput::Text(patch));
    let incomplete = ResponseItem::new(
        json!({"type":"function_call", "call_id":"partial", "name":"tool", "arguments":"{"}),
    )
    .unwrap();
    assert!(incomplete.tool_call().unwrap().unwrap().input == ToolInput::JsonArguments("{"));
    // Runtime/adapter owns argument parsing, not this wire preservation layer.
    let malformed = ResponseItem::new(
        json!({"type":"function_call", "call_id":"call", "name":"tool", "arguments": {}}),
    )
    .unwrap();
    assert!(matches!(malformed.tool_call(), Err(Error::InvalidItem)));
}

#[test]
fn tool_results_keep_text_multimodal_content_and_legacy_namespace_identity() {
    for kind in ["function_call_output", "custom_tool_call_output"] {
        for output in [
            json!("{\"structuredContent\":{\"ok\":true},\"_meta\":{\"signature\":\"fixture==\"}}"),
            json!([{"type":"input_text", "text":"result"}, {"type":"input_image", "image_url":"data:image/png;base64,fixture"}]),
        ] {
            let raw = json!({"type":kind, "call_id":"call_fixture", "output":output, "future": {"opaque":"fixture=="}});
            let item = ResponseItem::new(raw.clone()).unwrap();
            let result = item.tool_result().unwrap().unwrap();
            assert_eq!(result.call_id, Some("call_fixture"));
            assert_eq!(result.output, &output);
            assert_eq!(serde_json::to_value(item).unwrap(), raw);
        }
    }
    let legacy = ResponseItem::new(json!({"type":"function_call_output", "name":"exec", "namespace":"functions", "output":"legacy result"})).unwrap();
    let result = legacy.tool_result().unwrap().unwrap();
    assert_eq!(result.call_id, None);
    assert_eq!(result.name, Some("exec"));
    assert_eq!(result.namespace, Some("functions"));
    let invalid = ResponseItem::new(
        json!({"type":"custom_tool_call_output", "output":"missing correlation"}),
    )
    .unwrap();
    assert!(matches!(invalid.tool_result(), Err(Error::InvalidItem)));
}

#[test]
fn completed_tool_search_requires_client_correlation_and_wire_fields() {
    let call = json!({"type":"tool_search_call","execution":"client","call_id":"search-1","arguments":{"query":"calendar","limit":1}});
    let output = json!({"type":"tool_search_output","execution":"client","call_id":"search-1","status":"completed","tools":[]});
    for (original, key, replacement) in [
        (&call, "call_id", json!(null)),
        (&call, "call_id", json!("")),
        (&call, "call_id", json!(12)),
        (&call, "execution", json!(null)),
        (&call, "status", json!(12)),
        (&output, "call_id", json!(null)),
        (&output, "status", json!(null)),
        (&output, "tools", json!({})),
    ] {
        let mut malformed = original.clone();
        malformed[key] = replacement;
        assert!(
            CanonicalResponse::new(
                json!({"id":"fixture","status":"completed","output":[malformed]})
            )
            .is_err(),
            "accepted invalid {key}"
        );
        let event = json!({"type":"response.output_item.done","output_index":0,"item":malformed});
        let mut stream = ResponsesStream::new(4096).unwrap();
        assert!(
            stream
                .push(format!("data: {event}\n\n").as_bytes())
                .is_err()
        );
        assert_eq!(stream.state(), StreamState::Invalid);
        for (kind, status) in [
            ("response.completed", "completed"),
            ("response.incomplete", "incomplete"),
            ("response.failed", "failed"),
            ("error", "failed"),
        ] {
            let event = json!({"type":kind,"response":{"id":"fixture","status":status,"output":[malformed]}});
            assert!(
                ResponseEvent::new(event.clone()).is_err(),
                "accepted invalid search in {status}"
            );
            let mut stream = ResponsesStream::new(4096).unwrap();
            assert!(
                stream
                    .push(format!("data: {event}\n\n").as_bytes())
                    .is_err()
            );
            assert_eq!(stream.state(), StreamState::Invalid);
        }
    }
    for (original, key) in [
        (&call, "arguments"),
        (&output, "tools"),
        (&output, "status"),
    ] {
        let mut malformed = original.clone();
        malformed.as_object_mut().unwrap().remove(key);
        assert!(
            CanonicalResponse::new(
                json!({"id":"fixture","status":"completed","output":[malformed]})
            )
            .is_err()
        );
    }
}

#[test]
fn tool_search_views_preserve_json_declarations_and_execution_without_function_coercion() {
    let arguments: serde_json::Value = serde_json::from_str(r#"{"query":"日历🙂","limit":1,"future":{"integer":18446744073709551616,"decimal":0.12345678901234567890123456789}}"#).unwrap();
    let tools = json!([{"type":"namespace","name":"calendar","future":"retain","tools":[{"type":"function","name":"create","defer_loading":true,"parameters":{"type":"object","properties":{"title":{"type":"string"}}}}]}]);
    for (execution, call_id) in [
        ("client", json!("search-1")),
        ("server", json!(null)),
        ("future_executor", json!(null)),
    ] {
        let call = json!({"type":"tool_search_call","execution":execution,"call_id":call_id,"arguments":arguments,"future":"opaque+/=="});
        let output = json!({"type":"tool_search_output","execution":execution,"call_id":call_id,"status":"completed","tools":tools,"future":"opaque+/=="});
        let item = ResponseItem::new(call.clone()).unwrap();
        let view = item.tool_search_call().unwrap().unwrap();
        assert_eq!(view.execution, execution);
        assert_eq!(view.call_id, call_id.as_str());
        assert_eq!(view.status, None);
        assert_eq!(view.arguments, &arguments);
        assert!(item.tool_call().unwrap().is_none());
        assert!(item.tool_search_output().unwrap().is_none());
        let item = ResponseItem::new(output.clone()).unwrap();
        let view = item.tool_search_output().unwrap().unwrap();
        assert_eq!(view.execution, execution);
        assert_eq!(view.call_id, call_id.as_str());
        assert_eq!(view.status, "completed");
        assert_eq!(view.tools, tools.as_array().unwrap());
        assert!(item.tool_result().unwrap().is_none());
        assert!(item.tool_search_call().unwrap().is_none());
        let wire = json!({"id":"fixture","status":"completed","output":[call,output]});
        assert_eq!(
            serde_json::to_value(CanonicalResponse::new(wire.clone()).unwrap()).unwrap(),
            wire
        );
        for (kind, status, state) in [
            ("response.completed", "completed", StreamState::Completed),
            ("response.incomplete", "incomplete", StreamState::Incomplete),
            ("response.failed", "failed", StreamState::Failed),
            ("error", "failed", StreamState::Failed),
        ] {
            let mut response = wire.clone();
            response["status"] = status.into();
            let event = json!({"type":kind,"response":response});
            let parsed = ResponseEvent::new(event.clone()).unwrap();
            assert_eq!(parsed.terminal(), Some(state));
            assert_eq!(parsed.wire(), &event);
        }
        for raw in wire["output"].as_array().unwrap() {
            let event = json!({"type":"response.output_item.done","output_index":0,"item":raw});
            let mut stream = ResponsesStream::new(4096).unwrap();
            let frame = format!("data: {event}\n\n");
            let mut received = Vec::new();
            for byte in frame.bytes() {
                received.extend(stream.push(&[byte]).unwrap());
            }
            assert_eq!(received.len(), 1);
            assert_eq!(received[0].response.item().unwrap().unwrap().wire(), raw);
        }
    }
    // The pinned wire contract uses Value, not a function JSON string parser.
    // Argument/schema and discovered-tool authority checks belong to the adapter.
    let item = ResponseItem::new(json!({"type":"tool_search_call","execution":"client","call_id":"search-2","arguments":"opaque JSON value"})).unwrap();
    assert_eq!(
        item.tool_search_call().unwrap().unwrap().arguments,
        "opaque JSON value"
    );
    // Providers can emit an incomplete added item; completion enforces its fields.
    let added = json!({"type":"response.output_item.added","output_index":0,"item":{"type":"tool_search_call"}});
    assert!(ResponseEvent::new(added).is_ok());
}

#[test]
fn opaque_reasoning_compaction_unknown_items_and_event_fields_round_trip() {
    let large = r#"{"type":"provider.future","large_integer":18446744073709551616,"exact_decimal":0.12345678901234567890123456789}"#;
    let event = ResponseEvent::new(serde_json::from_str(large).unwrap()).unwrap();
    let serialized = serde_json::to_string(&event).unwrap();
    assert!(serialized.contains("18446744073709551616"));
    assert!(serialized.contains("0.12345678901234567890123456789"));
    for raw in [
        json!({"type":"reasoning", "id":"rs_fixture", "summary":[], "encrypted_content":"fixture-cipher+/==", "signature":"fixture-signature", "future":[{"nested":"opaque"}]}),
        json!({"type":"compaction", "encrypted_content":"fixture-compaction==", "future":true}),
        json!({"type":"provider_future_item", "payload":[false, {"nested":"opaque"}]}),
    ] {
        let item = ResponseItem::new(raw.clone()).unwrap();
        assert!(item.tool_call().unwrap().is_none());
        assert!(item.tool_result().unwrap().is_none());
        assert_eq!(serde_json::to_value(item).unwrap(), raw);
        let raw_event = json!({"type":"response.output_item.done", "output_index":9, "item":raw, "unknown_signature":"fixture-event=="});
        let event = ResponseEvent::new(raw_event.clone()).unwrap();
        assert_eq!(event.item().unwrap().unwrap().wire(), &raw_event["item"]);
        assert_eq!(serde_json::to_value(event).unwrap(), raw_event);
    }
}

#[test]
fn usage_counters_preserve_missing_values_cache_reasoning_and_future_details() {
    let raw = json!({"input_tokens":5, "output_tokens":9, "total_tokens":14, "input_tokens_details":{"cached_tokens":2, "cache_write_tokens":1}, "output_tokens_details":{"reasoning_tokens":7}, "provider_cost":{"opaque":true}});
    let usage = Usage::new(&raw).unwrap();
    assert_eq!(
        (usage.input_tokens, usage.output_tokens, usage.total_tokens),
        (Some(5), Some(9), Some(14))
    );
    assert_eq!(usage.raw, &raw);
    let partial = json!({"output_tokens":0});
    let usage = Usage::new(&partial).unwrap();
    assert_eq!(usage.input_tokens, None);
    assert_eq!(usage.total_tokens, None);
    for invalid in [
        json!([]),
        json!({"input_tokens":-1}),
        json!({"output_tokens":"9"}),
        json!({"total_tokens":1.5}),
    ] {
        assert!(matches!(Usage::new(&invalid), Err(Error::InvalidUsage)));
    }
}

#[test]
fn terminal_views_and_debug_do_not_expose_wire_payloads_or_relabel_failure() {
    let cases = [
        (
            "response.completed",
            "completed",
            None,
            StreamState::Completed,
        ),
        (
            "response.incomplete",
            "incomplete",
            Some("interrupted"),
            StreamState::Interrupted,
        ),
        (
            "response.incomplete",
            "incomplete",
            Some("max_output_tokens"),
            StreamState::Incomplete,
        ),
        (
            "response.incomplete",
            "incomplete",
            Some("content_filter"),
            StreamState::Incomplete,
        ),
        ("response.failed", "failed", None, StreamState::Failed),
    ];
    for (kind, status, reason, state) in cases {
        let event = ResponseEvent::new(json!({"type":kind,"response":{"id":"resp_fixture","status":status,"incomplete_details":{"reason":reason},"error":{"message":"fixture-private-payload"}}})).unwrap();
        assert_eq!(event.terminal(), Some(state));
        assert!(!format!("{event:?}").contains("fixture-private-payload"));
    }
    let item = ResponseItem::new(
        json!({"type":"reasoning", "encrypted_content":"fixture-private-payload"}),
    )
    .unwrap();
    assert!(!format!("{item:?}").contains("fixture-private-payload"));
    let request = CanonicalRequest::new(
        json!({"model":"fixture-private-payload","input":"fixture-private-payload"}),
        ResponsesDialect::Classic,
    )
    .unwrap();
    assert!(!format!("{request:?}").contains("fixture-private-payload"));
    assert!(matches!(
        ResponseEvent::new(
            json!({"type":"response.completed", "response":{"id":"resp_fixture", "status":"failed"}})
        ),
        Err(Error::InvalidEvent)
    ));
    assert!(matches!(
        ResponseEvent::new(json!({"type":"response.output_text.delta"})),
        Err(Error::InvalidEvent)
    ));
    assert!(matches!(
        ResponseEvent::new(json!({"type":"response.output_item.done","item":null})),
        Err(Error::InvalidEvent)
    ));
}
