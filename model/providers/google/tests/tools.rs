use caidex_model_core::{CanonicalRequest, ResponseItem, ResponsesDialect};
use caidex_provider_google::{ContentStream, NativeHistory, NativeResponse, ToolMap};
use serde_json::{Value, json};

const LIMIT: usize = 128 * 1024;
const MODEL: &str = "models/fixture-001";
const PREFIX: &str = "caidex.google.native-history.v2:";
fn function(name: &str) -> Value {
    json!({"type":"function","name":name,"description":"original description",
        "parameters":{"type":"object","properties":{"n":{"type":"integer"}},
        "required":["n"],"additionalProperties":false},"strict":false,"future":{"keep":null}})
}
fn declarations() -> Vec<Value> {
    vec![
        function("exec"),
        json!({"type":"namespace","name":"functions","description":"Namespace instructions",
        "tools":[function("exec"),{"type":"custom","name":"patch","format":
        {"type":"grammar","syntax":"lark","definition":"start: text"}}]}),
    ]
}
fn item(value: Value) -> ResponseItem {
    ResponseItem::new(value).unwrap()
}
fn safe(error: caidex_model_core::ProviderError) {
    let diagnostic = format!("{error:?} {}", error.wire());
    for private in [
        "original description",
        "SENSITIVE",
        "start: text",
        "sig-private",
    ] {
        assert!(!diagnostic.contains(private));
    }
}

// Break caught: schema conversion drops native JSON Schema or aliases depend
// on order/schema and silently rebind a namespaced historical tool.
#[test]
fn tool_declarations_preserve_json_schema_and_stable_scoped_aliases() {
    let mut source = declarations();
    source[0]["parameters"]["future"] = serde_json::from_str("18446744073709551616").unwrap();
    let map = ToolMap::new(&source, 10).unwrap();
    assert_eq!(map.source(), source);
    assert_eq!(map.native_tools().len(), 3);
    assert!(
        map.native_tools()[1]["description"]
            .as_str()
            .unwrap()
            .contains("Namespace instructions")
    );
    assert_eq!(
        map.native_tools()[0]["parametersJsonSchema"],
        source[0]["parameters"]
    );
    assert!(map.native_tools()[0].get("parameters").is_none());
    assert_eq!(
        map.native_tools()[0]["parametersJsonSchema"]["future"].to_string(),
        "18446744073709551616"
    );
    let mut reordered = source.clone();
    reordered.reverse();
    reordered.push(function("new"));
    let other = ToolMap::new(&reordered, 10).unwrap();
    for native in map.native_tools() {
        let alias = native["name"].as_str().unwrap();
        assert!(alias.starts_with("ct_") && alias.len() <= 128);
        assert!(
            alias
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        );
        assert!(
            other
                .native_tools()
                .iter()
                .any(|t| t["name"] == native["name"])
        );
    }
    let mut revised = source;
    revised[0]["parameters"]["properties"]["n"]["type"] = "string".into();
    assert_eq!(
        ToolMap::new(&revised, 10).unwrap().native_tools()[0]["name"],
        map.native_tools()[0]["name"]
    );
    assert!(map.has_grammar_tools()); // Format retained as guidance, no claim of native grammar enforcement.
    assert!(!format!("{map:?}").contains("original description"));
}

// Break caught: function/custom kinds, namespaces, native ID and precision are
// lost or a missing native ID is fabricated differently across projection.
#[test]
fn function_and_custom_calls_keep_identity_ids_and_exact_inputs() {
    let map = ToolMap::new(&declarations(), 10).unwrap();
    let mut aliases = Vec::new();
    for (index, namespace) in [None, Some("functions")].into_iter().enumerate() {
        let mut wire = json!({"type":"function_call","name":"exec","call_id":format!("call-{index}"),"arguments":" {\"n\":18446744073709551616} "});
        if let Some(namespace) = namespace {
            wire["namespace"] = namespace.into();
        }
        let native = map.native_call(&item(wire.clone())).unwrap();
        aliases.push(native["name"].clone());
        assert_eq!(native["id"], wire["call_id"]);
        assert_eq!(native["args"]["n"].to_string(), "18446744073709551616");
        let projected = map.responses_call(&native, "fallback").unwrap();
        assert_eq!(projected.wire()["name"], "exec");
        assert_eq!(projected.wire()["namespace"], wire["namespace"]);
        assert_eq!(projected.wire()["call_id"], wire["call_id"]);
        assert_eq!(
            projected.wire()["arguments"],
            "{\"n\":18446744073709551616}"
        );
        let mut no_id = native;
        no_id.as_object_mut().unwrap().remove("id");
        assert_eq!(
            map.responses_call(&no_id, "stable-local-id")
                .unwrap()
                .wire()["call_id"],
            "stable-local-id"
        );
    }
    assert_ne!(aliases[0], aliases[1]);
    let text = "\n*** Begin Patch\n中文🙂\"quoted\"\\path\n*** End Patch\n ";
    let custom = json!({"type":"custom_tool_call","namespace":"functions","name":"patch","call_id":"custom-id","input":text});
    let native = map.native_call(&item(custom.clone())).unwrap();
    assert_eq!(native["args"], json!({"input":text}));
    assert_eq!(
        map.responses_call(&native, "fallback").unwrap().wire(),
        &custom
    );
    assert!(
        map.native_tools()[2]["description"]
            .as_str()
            .unwrap()
            .contains("start: text")
    );
    assert!(map.native_tools()[2].get("strict").is_none());
    for args in [
        json!({"input":1}),
        json!({"input":"x","extra":true}),
        json!({}),
    ] {
        let mut invalid = native.clone();
        invalid["args"] = args;
        safe(map.responses_call(&invalid, "fallback").err().unwrap());
    }
    let empty = json!({"name":aliases[0]});
    assert_eq!(
        map.responses_call(&empty, "no-args").unwrap().wire()["arguments"],
        "{}"
    );
}

// Break caught: unrecognized/server tools or wrong call scope can select a
// Runtime tool; malformed arguments, empty IDs or custom extra keys accepted.
#[test]
fn unbound_names_and_invalid_call_shapes_cannot_select_runtime_tools() {
    let map = ToolMap::new(&declarations(), 10).unwrap();
    for native in [
        json!({"name":"exec","args":{}}),
        json!({"name":map.native_tools()[0]["name"],"args":[]}),
        json!({"name":map.native_tools()[0]["name"],"id":"","args":{}}),
    ] {
        safe(map.responses_call(&native, "fallback").err().unwrap());
    }
    for arguments in ["null", "[1]", "{bad"] {
        safe(
            map.native_call(&item(
                json!({"type":"function_call","call_id":"id","name":"exec","arguments":arguments}),
            ))
            .err()
            .unwrap(),
        );
    }
    for wire in [
        json!({"type":"function_call","call_id":"id","name":"exec","namespace":"wrong","arguments":"{}"}),
        json!({"type":"custom_tool_call","call_id":"id","name":"exec","input":"SENSITIVE"}),
        json!({"type":"tool_search_call","execution":"server","arguments":{}}),
    ] {
        safe(map.native_call(&item(wire)).err().unwrap());
    }
    let native = json!({"name":map.native_tools()[0]["name"],"args":{}});
    safe(map.responses_call(&native, "").err().unwrap());
}

// Break caught: compiler silently drops strict/deferred/builtin semantics,
// unbounded flattened declarations or duplicate identities with different kind.
#[test]
fn unsupported_semantics_and_invalid_definitions_fail_with_safe_errors() {
    assert!(ToolMap::new(&[], 10).unwrap().native_tools().is_empty());
    for max in [0, 2] {
        safe(ToolMap::new(&declarations(), max).err().unwrap());
    }
    let mut strict = function("strict");
    strict["strict"] = true.into();
    assert_eq!(
        ToolMap::new(&[strict], 10).err().unwrap().code,
        "unsupported_google_strict_tools"
    );
    let mut deferred = function("hidden");
    deferred["defer_loading"] = true.into();
    assert_eq!(
        ToolMap::new(&[deferred], 10).err().unwrap().code,
        "unsupported_google_tool_discovery"
    );
    assert_eq!(
        ToolMap::new(&[json!({"type":"tool_search","execution":"client"})], 10)
            .err()
            .unwrap()
            .code,
        "unsupported_google_tool_discovery"
    );
    assert_eq!(
        ToolMap::new(
            &[json!({"type":"web_search","search_context_size":"medium"})],
            10
        )
        .err()
        .unwrap()
        .code,
        "unsupported_google_web_search"
    );
    for source in [
        vec![function("dup"), function("dup")],
        vec![function("dup"), json!({"type":"custom","name":"dup"})],
        vec![
            json!({"type":"namespace","name":"ns","tools":[{"type":"namespace","name":"inner","tools":[]}]}),
        ],
        vec![json!({"type":"function","name":"SENSITIVE","parameters":[]})],
        vec![json!({"type":"function","name":"SENSITIVE","parameters":{"type":"array"}})],
        vec![
            json!({"type":"custom","name":"SENSITIVE","format":{"type":"grammar","syntax":"bad","definition":"SENSITIVE"}}),
        ],
        vec![function("bad\nname")],
    ] {
        safe(ToolMap::new(&source, 10).err().unwrap());
    }
    for (field, value) in [
        ("strict", json!(1)),
        ("defer_loading", json!("yes")),
        ("description", json!([])),
    ] {
        let mut invalid = function("exec");
        invalid[field] = value;
        safe(ToolMap::new(&[invalid], 10).err().unwrap());
    }
}

fn mapped_history(streaming: bool) -> (NativeHistory, Value, Value) {
    let map = ToolMap::new(&declarations(), 10).unwrap();
    let request = json!({"contents":[{"role":"user","parts":[{"text":"start"}]}],"tools":[{"functionDeclarations":map.native_tools()}]});
    let response = json!({"candidates":[{"content":{"role":"model","parts":[
        {"text":"thought","thought":true,"thoughtSignature":"sig-private"},
        {"functionCall":{"name":map.native_tools()[1]["name"],"args":{"n":1}},"thoughtSignature":"sig-call"},
        {"functionCall":{"name":map.native_tools()[2]["name"],"id":"native-custom","args":{"input":"\npatch 中文🙂\n "}},"thoughtSignature":"sig-custom"},
        {"toolCall":{"server":"opaque"}},{"futurePart":{"keep":null}}]},"finishReason":"STOP"}],"future":{"keep":true}});
    let raw = if streaming {
        let mut stream = ContentStream::new(LIMIT, LIMIT, 1).unwrap();
        stream
            .push(format!("data: {response}\n\n").as_bytes())
            .unwrap();
        stream.finish().unwrap();
        NativeHistory::from_stream(
            stream.completed_response().unwrap(),
            MODEL,
            &request,
            Some(0),
            "mapped-id",
            LIMIT,
        )
        .unwrap()
    } else {
        NativeHistory::from_response(
            &NativeResponse::parse(response.clone()).unwrap(),
            MODEL,
            &request,
            Some(0),
            "mapped-id",
            LIMIT,
        )
        .unwrap()
    };
    (raw.with_tools(&map, LIMIT).unwrap(), request, response)
}

// Break caught: current declarations reinterpret history, signatures/unknown
// parts vanish, or v1 history silently migrates to a different call identity.
#[test]
fn mapped_json_and_stream_history_owns_original_declarations_in_both_dialects() {
    for streaming in [false, true] {
        let (history, request, response) = mapped_history(streaming);
        let projected = history.to_responses(LIMIT).unwrap();
        assert_eq!(projected.output()[1]["namespace"], "functions");
        assert_eq!(projected.output()[1]["name"], "exec");
        assert_eq!(projected.output()[1]["call_id"], "call_mapped-id_0_1");
        assert_eq!(projected.output()[2]["type"], "custom_tool_call");
        assert_eq!(projected.output()[2]["input"], "\npatch 中文🙂\n ");
        assert_eq!(projected.output()[2]["call_id"], "native-custom");
        let envelope: Value = serde_json::from_str(
            projected.output()[0]["encrypted_content"]
                .as_str()
                .unwrap()
                .strip_prefix(PREFIX)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(envelope["tools"], json!(declarations()));
        assert_eq!(envelope["request"], request);
        for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
            let wire =
                CanonicalRequest::new(json!({"model":"alias","input":projected.output()}), dialect)
                    .unwrap();
            let stored: Value =
                serde_json::from_slice(&serde_json::to_vec(&wire).unwrap()).unwrap();
            let restored = NativeHistory::from_responses_output(
                stored["input"].as_array().unwrap(),
                MODEL,
                &request,
                LIMIT,
            )
            .unwrap();
            assert_eq!(restored.native_response(), &response);
            assert_eq!(
                restored.replay_content().unwrap(),
                &response["candidates"][0]["content"]
            );
            if streaming {
                assert_eq!(restored.chunks().unwrap(), std::slice::from_ref(&response));
            }
        }
        let raw = NativeHistory::from_response(
            &NativeResponse::parse(response.clone()).unwrap(),
            MODEL,
            &request,
            Some(0),
            "raw-id",
            LIMIT,
        )
        .unwrap();
        let view = raw.to_responses(LIMIT).unwrap();
        assert!(
            view.output()[0]["encrypted_content"]
                .as_str()
                .unwrap()
                .starts_with("caidex.google.native-history.v1:")
        );
        assert_eq!(
            view.output()[1]["name"],
            response["candidates"][0]["content"]["parts"][1]["functionCall"]["name"]
        );
        assert!(view.output()[1].get("namespace").is_none());
        assert_eq!(view.output()[2]["type"], "function_call");
    }
}

// Break caught: altered display/declared schemas or wrong native map
// resurrect signed content; mutable public projection fields hide a rebind.
#[test]
fn mapped_history_rejects_edited_display_or_declarations_and_wrong_request_tools() {
    let (history, request, _) = mapped_history(false);
    let view = history.to_responses(LIMIT).unwrap();
    for (index, key, value) in [
        (1, "namespace", json!("wrong")),
        (1, "call_id", json!("other")),
        (2, "input", json!("trimmed")),
        (2, "type", json!("function_call")),
    ] {
        let mut output = view.output().to_vec();
        output[index][key] = value;
        safe(
            NativeHistory::from_responses_output(&output, MODEL, &request, LIMIT)
                .err()
                .unwrap(),
        );
    }
    let original: Value = serde_json::from_str(
        view.output()[0]["encrypted_content"]
            .as_str()
            .unwrap()
            .strip_prefix(PREFIX)
            .unwrap(),
    )
    .unwrap();
    for declarations in [json!([]), Value::Null, json!([function("other")]), {
        let mut changed = json!(declarations());
        changed[0]["parameters"]["properties"]["n"]["type"] = "string".into();
        changed
    }] {
        let mut envelope = original.clone();
        envelope["tools"] = declarations;
        let mut output = view.output().to_vec();
        output[0]["encrypted_content"] = format!("{PREFIX}{envelope}").into();
        safe(
            NativeHistory::from_responses_output(&output, MODEL, &request, LIMIT)
                .err()
                .unwrap(),
        );
    }
    let wrong = ToolMap::new(&[function("other")], 10).unwrap();
    safe(history.clone().with_tools(&wrong, LIMIT).err().unwrap());
    assert!(history.to_responses(10).is_err());
    let empty = ToolMap::new(&[], 10).unwrap();
    let native =
        json!({"candidates":[{"content":{"parts":[{"text":"ok"}]},"finishReason":"STOP"}]});
    let body = json!({"contents":[{"parts":[{"text":"start"}]}]});
    let raw = NativeHistory::from_response(
        &NativeResponse::parse(native).unwrap(),
        MODEL,
        &body,
        Some(0),
        "empty-map",
        LIMIT,
    )
    .unwrap();
    let mapped = raw
        .with_tools(&empty, LIMIT)
        .unwrap()
        .to_responses(LIMIT)
        .unwrap();
    NativeHistory::from_responses_output(mapped.output(), MODEL, &body, LIMIT).unwrap();
}
