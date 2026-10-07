use caidex_model_core::ResponseItem;
use caidex_provider_anthropic::ToolMap;
use serde_json::{Value, json};
fn function(name: &str) -> Value {
    json!({"type":"function","name":name,"description":"fixture","parameters":{"type":"object","properties":{"n":{"type":"integer"}},"future_schema":true},"strict":false,"future_declaration":"retain"})
}
fn declarations() -> Vec<Value> {
    vec![
        function("exec"),
        json!({"type":"namespace","name":"functions","tools":[function("exec"),{"type":"custom","name":"patch","format":{"type":"grammar","syntax":"lark","definition":"start: text"}}]}),
    ]
}
#[test]
fn declarations_keep_schema_and_source_and_names_are_stable_across_reordering_and_additions() {
    let source = declarations();
    let map = ToolMap::new(&source, 10).unwrap();
    assert_eq!(map.source(), source);
    assert!(map.has_grammar_tools());
    assert_eq!(map.native_tools().len(), 3);
    assert_eq!(
        map.native_tools()[0]["input_schema"],
        source[0]["parameters"]
    );
    assert_eq!(map.native_tools()[0]["strict"], false);
    let mut reordered = source.clone();
    reordered.reverse();
    reordered.push(function("new"));
    let other = ToolMap::new(&reordered, 10).unwrap();
    for tool in map.native_tools() {
        let name = tool["name"].as_str().unwrap();
        assert_eq!(name.len(), 67);
        assert!(name.bytes().all(|v| v.is_ascii_alphanumeric() || v == b'_'));
        assert!(
            other
                .native_tools()
                .iter()
                .any(|other| other["name"] == tool["name"])
        );
    }
    assert!(!format!("{map:?}").contains("start: text"));
}
#[test]
fn same_names_in_different_namespaces_and_parallel_call_ids_roundtrip_without_collision() {
    let map = ToolMap::new(&declarations(), 10).unwrap();
    let mut aliases = Vec::new();
    for (index, namespace) in [None, Some("functions")].into_iter().enumerate() {
        let mut wire = json!({"type":"function_call","call_id":format!("call-{index}"),"name":"exec","arguments":" {\"n\":18446744073709551616} "});
        if let Some(namespace) = namespace {
            wire["namespace"] = namespace.into();
        }
        let call = ResponseItem::new(wire.clone()).unwrap();
        let native = map.native_call(&call).unwrap();
        aliases.push(native["name"].clone());
        let result = map.responses_call(&native).unwrap();
        assert_eq!(result.wire()["call_id"], wire["call_id"]);
        assert_eq!(result.wire()["namespace"], wire["namespace"]);
        let parsed: Value =
            serde_json::from_str(result.wire()["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(parsed, native["input"]);
    }
    assert_ne!(aliases[0], aliases[1]);
}
#[test]
fn custom_input_is_preserved_exactly_including_whitespace_escapes_and_unicode() {
    let map = ToolMap::new(&declarations(), 10).unwrap();
    let text = "\n*** Begin Patch\n中文🙂\"quoted\"\\path\n*** End Patch\n ";
    let wire = json!({"type":"custom_tool_call","call_id":"patch-call","namespace":"functions","name":"patch","input":text});
    let native = map
        .native_call(&ResponseItem::new(wire.clone()).unwrap())
        .unwrap();
    assert_eq!(native["input"]["input"], text);
    assert_eq!(map.responses_call(&native).unwrap().wire(), &wire);
    assert!(
        map.native_tools()[2]["description"]
            .as_str()
            .unwrap()
            .contains("start: text")
    );
    let mut bad = native.clone();
    bad["input"]["extra"] = true.into();
    assert!(map.responses_call(&bad).is_err());
    bad["input"] = json!({"input":1});
    assert!(map.responses_call(&bad).is_err());
    bad["input"] = json!({"wrong_field":"text"});
    assert!(map.responses_call(&bad).is_err());
}
#[test]
fn unknown_native_names_wrong_scope_and_bad_call_inputs_cannot_select_tools() {
    let map = ToolMap::new(&declarations(), 10).unwrap();
    for wire in [
        json!({"type":"tool_use","id":"one","name":"exec","input":{}}),
        json!({"type":"server_tool_use","id":"one","name":map.native_tools()[0]["name"],"input":{}}),
    ] {
        assert!(map.responses_call(&wire).is_err());
    }
    for arguments in ["[1]", "{bad", "null"] {
        let item = ResponseItem::new(
            json!({"type":"function_call","call_id":"one","name":"exec","arguments":arguments}),
        )
        .unwrap();
        assert!(map.native_call(&item).is_err());
    }
    let item=ResponseItem::new(json!({"type":"function_call","call_id":"one","namespace":"wrong","name":"exec","arguments":"{}"})).unwrap();
    assert!(map.native_call(&item).is_err());
}
#[test]
fn duplicates_limits_nested_namespaces_and_malformed_definitions_are_rejected() {
    assert!(ToolMap::new(&declarations(), 0).is_err());
    assert!(ToolMap::new(&declarations(), 2).is_err());
    assert!(ToolMap::new(&[function("exec"), function("exec")], 10).is_err());
    assert!(ToolMap::new(&[json!({"type":"namespace","name":"ns","tools":[{"type":"namespace","name":"nested","tools":[]}]})],10).is_err());
    for wire in [
        json!({"type":"web_search","name":"search"}),
        json!({"type":"function","name":"exec","parameters":[]}),
        json!({"type":"custom","name":"patch","format":{"type":"grammar","syntax":"unknown","definition":"opaque"}}),
        function("bad\nname"),
    ] {
        assert!(ToolMap::new(&[wire], 10).is_err());
    }
    assert!(ToolMap::new(&[], 10).unwrap().native_tools().is_empty());
}

fn mapped_reply(map: &ToolMap) -> caidex_provider_anthropic::NativeMessage {
    let calls = [
        json!({"type":"function_call","name":"exec","call_id":"bare","arguments":"{}"}),
        json!({"type":"function_call","namespace":"functions","name":"exec","call_id":"ns","arguments":"{\"n\":18446744073709551616}"}),
        json!({"type":"custom_tool_call","namespace":"functions","name":"patch","call_id":"custom","input":"\npatch 中文🙂\n "}),
    ];
    let mut content = vec![json!({"type":"thinking","thinking":"native","signature":"sig+/=="})];
    for wire in calls {
        content.push(map.native_call(&ResponseItem::new(wire).unwrap()).unwrap());
    }
    content.push(json!({"type":"future_block","data":"opaque"}));
    caidex_provider_anthropic::NativeMessage::parse(json!({"type":"message","id":"mapped","model":"native","role":"assistant","content":content,"stop_reason":"tool_use","usage":{"input_tokens":1,"output_tokens":2}})).unwrap()
}
#[test]
fn mapped_history_owns_original_declarations_and_roundtrips_both_dialects() {
    use caidex_model_core::{CanonicalRequest, ResponsesDialect};
    use caidex_provider_anthropic::NativeMessage;
    let map = ToolMap::new(&declarations(), 10).unwrap();
    let native = mapped_reply(&map);
    let projected = native.to_responses_with_tools(&map, 128 * 1024).unwrap();
    assert_eq!(projected.output()[1]["name"], "exec");
    assert_eq!(projected.output()[2]["namespace"], "functions");
    assert_eq!(projected.output()[3]["type"], "custom_tool_call");
    assert_eq!(projected.output()[3]["input"], "\npatch 中文🙂\n ");
    let capsule = projected.output()[0]["encrypted_content"].as_str().unwrap();
    let envelope: Value = serde_json::from_str(
        capsule
            .strip_prefix("caidex.anthropic.native-message.v2:")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(envelope["tools"], json!(declarations()));
    // Restoration accepts no current ToolMap: new or removed declarations cannot
    // silently rebind this historical reply.
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let request = CanonicalRequest::new(
            json!({"model":"alias","input":projected.output(),"stream":true}),
            dialect,
        )
        .unwrap();
        let stored: Value = serde_json::from_slice(&serde_json::to_vec(&request).unwrap()).unwrap();
        let restored = NativeMessage::from_responses_output(
            stored["input"].as_array().unwrap(),
            "native",
            128 * 1024,
        )
        .unwrap();
        assert_eq!(restored.wire(), native.wire());
    }
    assert!(
        native
            .to_responses_with_tools(&map, capsule.len() - 1)
            .is_err()
    );
}
#[test]
fn mapped_history_rejects_changed_tool_identity_custom_text_and_capsule_bindings() {
    use caidex_provider_anthropic::NativeMessage;
    let map = ToolMap::new(&declarations(), 10).unwrap();
    let native = mapped_reply(&map);
    let projected = native.to_responses_with_tools(&map, 128 * 1024).unwrap();
    for (index, key, value) in [
        (2, "namespace", json!("wrong")),
        (1, "call_id", json!("wrong")),
        (3, "input", json!("patch 中文🙂")),
        (3, "type", json!("function_call")),
    ] {
        let mut output = projected.output().to_vec();
        output[index][key] = value;
        assert!(NativeMessage::from_responses_output(&output, "native", 128 * 1024).is_err());
    }
    let original = projected.output()[0]["encrypted_content"].as_str().unwrap();
    let envelope: Value = serde_json::from_str(
        original
            .strip_prefix("caidex.anthropic.native-message.v2:")
            .unwrap(),
    )
    .unwrap();
    for replacement in [json!([]), Value::Null, json!([function("other")])] {
        let mut altered = envelope.clone();
        altered["tools"] = replacement;
        let mut output = projected.output().to_vec();
        output[0]["encrypted_content"] =
            format!("caidex.anthropic.native-message.v2:{altered}").into();
        assert!(NativeMessage::from_responses_output(&output, "native", 128 * 1024).is_err());
    }
    let wrong = ToolMap::new(&[function("other")], 10).unwrap();
    assert!(native.to_responses_with_tools(&wrong, 128 * 1024).is_err());
}
