use caidex_model_core::{CanonicalRequest, ResponsesDialect};
use caidex_provider_google::{GenerateContentRequest, NativeHistory, NativeResponse, ToolMap};
use serde_json::{Value, json};

const MODEL: &str = "models/fixture-001";
const LIMIT: usize = 256 * 1024;
fn declarations() -> Vec<Value> {
    vec![json!({"type":"namespace","name":"local","tools":[
        {"type":"function","name":"echo","parameters":{"type":"object","properties":{"value":{"type":"string"}}}},
        {"type":"custom","name":"raw","format":{"type":"text"}}]})]
}
fn source(dialect: ResponsesDialect, input: Vec<Value>, tools: &[Value]) -> CanonicalRequest {
    let mut wire = json!({"model":"executor-alias","input":input,"stream":true,"store":false,
        "parallel_tool_calls":true,"include":["reasoning.encrypted_content"]});
    if dialect == ResponsesDialect::Lite {
        wire["input"].as_array_mut().unwrap().insert(
            0,
            json!({"type":"additional_tools","role":"developer","id":"at_fixture","tools":tools}),
        );
    } else {
        wire["tools"] = json!(tools);
    }
    CanonicalRequest::new(wire, dialect).unwrap()
}
fn compile(source: &CanonicalRequest) -> GenerateContentRequest {
    GenerateContentRequest::from_responses(source, MODEL, 128, LIMIT, 8).unwrap()
}
fn start() -> Vec<Value> {
    vec![
        json!({"role":"developer","content":[{"type":"input_text","text":"fixed"}]}),
        json!({"type":"message","role":"user","content":"start"}),
    ]
}
fn native_reply(tools: &ToolMap, reason: &str) -> Value {
    json!({"candidates":[{"finishReason":reason,"content":{"parts":[
        {"thought":true,"text":"private","thoughtSignature":"signed-thinking"},
        {"text":"working","thoughtSignature":"signed-text"},
        {"functionCall":{"name":tools.native_tools()[0]["name"],"args":{"value":"first"}},"thoughtSignature":"signed-call"},
        {"functionCall":{"name":tools.native_tools()[1]["name"],"id":"native-custom","args":{"input":"  原文\n🙂  "}},"futurePart":{"keep":true}}
    ],"futureContent":{"keep":true}}}]})
}
fn projected(wire: Value, request: &Value, tools: &ToolMap, id: &str) -> Vec<Value> {
    NativeHistory::from_response(
        &NativeResponse::parse(wire).unwrap(),
        MODEL,
        request,
        Some(0),
        id,
        LIMIT,
    )
    .unwrap()
    .with_tools(tools, LIMIT)
    .unwrap()
    .to_responses(LIMIT)
    .unwrap()
    .output()
    .to_vec()
}
fn results() -> Vec<Value> {
    vec![
        json!({"type":"custom_tool_call_output","call_id":"native-custom","output":"  原文\n🙂  "}),
        json!({"type":"function_call_output","call_id":"call_first_0_2","name":"echo","namespace":"local","output":"{\"big\":18446744073709551616}"}),
    ]
}

// Catch role/system demotion, Lite declarations ignored, model route injection
// and request-only fields leaked into the native body.
#[test]
fn classic_and_lite_compile_equal_system_messages_and_tools() {
    let tools = declarations();
    let classic = source(ResponsesDialect::Classic, start(), &tools);
    let lite = source(ResponsesDialect::Lite, start(), &tools);
    let a = compile(&classic);
    let b = compile(&lite);
    assert_eq!(a.wire(), b.wire());
    assert_eq!(
        a.wire()["systemInstruction"],
        json!({"parts":[{"text":"fixed"}]})
    );
    assert_eq!(
        a.wire()["contents"],
        json!([{"role":"user","parts":[{"text":"start"}]}])
    );
    assert_eq!(a.wire()["generationConfig"], json!({"maxOutputTokens":128}));
    assert_eq!(
        a.wire()["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"],
        json!({"type":"object","properties":{"value":{"type":"string"}}})
    );
    for key in ["model", "stream", "include", "store", "parallel_tool_calls"] {
        assert!(a.wire().get(key).is_none());
    }
    assert_eq!(a.source(), classic.wire());
    assert!(!format!("{a:?}").contains("fixed"));
    let string = CanonicalRequest::new(
        json!({"model":"alias","input":"hello","instructions":"instruct"}),
        ResponsesDialect::Classic,
    )
    .unwrap();
    let string = compile(&string);
    assert_eq!(
        string.wire()["systemInstruction"]["parts"],
        json!([{"text":"instruct"}])
    );
    assert_eq!(
        string.wire()["contents"],
        json!([{"role":"user","parts":[{"text":"hello"}]}])
    );
}

// Catch reconstruction from display, local IDs inserted into signed Parts,
// result reordering/dropped scope, and prefix snapshots not covering three turns.
#[test]
fn complete_signed_groups_replay_exact_content_and_pair_parallel_results() {
    let declarations = declarations();
    let tools = ToolMap::new(&declarations, 8).unwrap();
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let first = compile(&source(dialect, start(), &declarations));
        let reply = native_reply(&tools, "STOP");
        let mut input = start();
        input.extend(projected(reply.clone(), first.wire(), &tools, "first"));
        input.extend(results());
        input.push(json!({"role":"user","content":[{"type":"input_text","text":"continue"}]}));
        let second = compile(&source(dialect, input.clone(), &declarations));
        let content = &second.wire()["contents"];
        let mut raw = reply["candidates"][0]["content"].clone();
        raw["role"] = "model".into(); // missing native role normalized only on replay
        assert_eq!(content[1], raw);
        assert!(content[1]["parts"][2]["functionCall"].get("id").is_none());
        assert_eq!(content[2]["role"], "user");
        let parts = content[2]["parts"].as_array().unwrap();
        assert_eq!(
            parts[1],
            json!({"functionResponse":{"name":tools.native_tools()[1]["name"],
            "id":"native-custom","response":{"output":"  原文\n🙂  "}}})
        );
        assert_eq!(
            parts[0],
            json!({"functionResponse":{"name":tools.native_tools()[0]["name"],
            "response":{"output":"{\"big\":18446744073709551616}"}}})
        );
        assert_eq!(parts[2], json!({"text":"continue"}));
        input.extend(projected(
            json!({"candidates":[{"finishReason":"STOP","content":{
            "role":"model","parts":[{"text":"done","thoughtSignature":"second-signature"}]}}]}),
            second.wire(),
            &tools,
            "second",
        ));
        input.push(json!({"role":"user","content":"third"}));
        let third = compile(&source(dialect, input, &declarations));
        assert_eq!(third.wire()["contents"][1], raw);
        assert_eq!(
            third.wire()["contents"][3],
            json!({"role":"model","parts":[{"text":"done","thoughtSignature":"second-signature"}]})
        );
        assert_eq!(
            third.wire()["contents"][4],
            json!({"role":"user","parts":[{"text":"third"}]})
        );
    }
}

// Catch trusting the capsule's request as the expected prefix or allowing
// missing/edited display items, changed declarations, or cross-model signatures.
#[test]
fn replay_rejects_changed_prefix_model_tools_and_incomplete_groups() {
    let declarations = declarations();
    let tools = ToolMap::new(&declarations, 8).unwrap();
    let first = compile(&source(ResponsesDialect::Classic, start(), &declarations));
    let group = projected(native_reply(&tools, "STOP"), first.wire(), &tools, "first");
    let mut input = start();
    input.extend(group.clone());
    input.extend(results());
    for mutation in 0..6 {
        let mut altered = input.clone();
        match mutation {
            0 => altered[1]["content"] = "edited".into(),
            1 => {
                altered.remove(3);
            }
            2 => altered[3]["content"][0]["text"] = "edited".into(),
            3 => altered[2]["summary"][0]["text"] = "edited".into(),
            4 => {
                altered.remove(0);
            }
            5 => {
                altered.remove(1);
            }
            _ => unreachable!(),
        }
        let request = source(ResponsesDialect::Classic, altered, &declarations);
        assert!(
            GenerateContentRequest::from_responses(&request, MODEL, 128, LIMIT, 8).is_err(),
            "mutation {mutation}"
        );
    }
    let request = source(ResponsesDialect::Classic, input.clone(), &declarations);
    assert_eq!(
        GenerateContentRequest::from_responses(&request, "models/other", 128, LIMIT, 8)
            .unwrap_err()
            .code,
        "google_history_model_mismatch"
    );
    assert_eq!(
        GenerateContentRequest::from_responses(&request, MODEL, 129, LIMIT, 8)
            .unwrap_err()
            .code,
        "google_history_request_mismatch"
    );
    let mut changed = declarations.clone();
    changed[0]["tools"][0]["parameters"]["properties"]["extra"] = json!({"type":"string"});
    assert!(
        GenerateContentRequest::from_responses(
            &source(ResponsesDialect::Classic, input, &changed),
            MODEL,
            128,
            LIMIT,
            8
        )
        .is_err()
    );
    let mut adjacent = start();
    adjacent.extend(group); // no results before the next assistant group
    adjacent.push(json!({"role":"assistant","content":"not a result"}));
    assert!(
        GenerateContentRequest::from_responses(
            &source(ResponsesDialect::Classic, adjacent, &declarations),
            MODEL,
            128,
            LIMIT,
            8
        )
        .is_err()
    );
}

// Catch duplicate/orphan/wrong-kind result execution and ambiguous legacy
// outputs; valid no-ID legacy output uses the uniquely matching scope.
#[test]
fn standalone_calls_pair_by_id_or_unique_legacy_identity() {
    let declarations = declarations();
    let call = json!({"type":"function_call","call_id":"one","name":"echo","namespace":"local","arguments":"{\"value\":\"hello\"}"});
    let result = json!({"type":"function_call_output","name":"echo","namespace":"local","output":[{"type":"input_text","text":"ok"},{"type":"input_text","text":"exact"}]});
    let mut input = start();
    input.extend([call.clone(), result.clone()]);
    let compiled = compile(&source(
        ResponsesDialect::Classic,
        input.clone(),
        &declarations,
    ));
    assert_eq!(
        compiled.wire()["contents"][1]["parts"][0]["functionCall"]["id"],
        "one"
    );
    assert_eq!(
        compiled.wire()["contents"][2]["parts"][0]["functionResponse"]["id"],
        "one"
    );
    assert_eq!(
        compiled.wire()["contents"][2]["parts"][0]["functionResponse"]["response"],
        json!({"output":[{"type":"input_text","text":"ok"},{"type":"input_text","text":"exact"}]})
    );
    for mutation in 0..8 {
        let mut altered = input.clone();
        match mutation {
            0 => {
                altered.push(result.clone());
            }
            1 => {
                altered.remove(2);
            }
            2 => altered[3]["call_id"] = "orphan".into(),
            3 => altered[3]["namespace"] = "other".into(),
            4 => altered[3]["type"] = "custom_tool_call_output".into(),
            5 => {
                let mut other = call.clone();
                other["call_id"] = "two".into();
                altered.insert(3, other);
            }
            6 => {
                altered.insert(3, call.clone());
            }
            7 => {
                altered.insert(3, json!({"role":"user","content":"before results"}));
            }
            _ => unreachable!(),
        }
        assert!(
            GenerateContentRequest::from_responses(
                &source(ResponsesDialect::Classic, altered, &declarations),
                MODEL,
                128,
                LIMIT,
                8
            )
            .is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn empty_tool_results_remain_structured_and_malformed_message_types_are_rejected() {
    let declarations = declarations();
    let mut input = start();
    input.extend([
        json!({"type":"custom_tool_call","call_id":"empty","name":"raw","namespace":"local","input":""}),
        json!({"type":"custom_tool_call_output","call_id":"empty","output":[]}),
    ]);
    let compiled = compile(&source(ResponsesDialect::Classic, input, &declarations));
    assert_eq!(
        compiled.wire()["contents"][2]["parts"][0]["functionResponse"]["response"],
        json!({"output":[]})
    );
    for kind in [json!(null), json!(42), json!({})] {
        let request = source(
            ResponsesDialect::Classic,
            vec![json!({"type":kind,"role":"user","content":"bad"})],
            &[],
        );
        assert_eq!(
            GenerateContentRequest::from_responses(&request, MODEL, 128, LIMIT, 8)
                .unwrap_err()
                .code,
            "invalid_google_request"
        );
    }
}

#[test]
fn idless_parallel_results_keep_original_native_call_order() {
    let declarations = declarations();
    let tools = ToolMap::new(&declarations, 8).unwrap();
    let first = compile(&source(ResponsesDialect::Classic, start(), &declarations));
    for mixed_ids in [false, true] {
        let mut native = json!({"candidates":[{"finishReason":"STOP","content":{"parts":[
            {"functionCall":{"name":tools.native_tools()[0]["name"],"args":{"value":"first"}},"thoughtSignature":"first-signature"},
            {"functionCall":{"name":tools.native_tools()[0]["name"],"args":{"value":"second"}},"thoughtSignature":"second-signature"}
        ]}}]});
        if mixed_ids {
            native["candidates"][0]["content"]["parts"][0]["functionCall"]["id"] =
                "native-first".into();
        }
        let group = projected(native.clone(), first.wire(), &tools, "ordered");
        let mut input = start();
        input.extend(group.clone());
        input.extend([
            json!({"type":"function_call_output","call_id":group[2]["call_id"],"output":"SECOND"}),
            json!({"type":"function_call_output","call_id":group[1]["call_id"],"output":"FIRST"}),
        ]);
        let compiled = compile(&source(ResponsesDialect::Classic, input, &declarations));
        assert_eq!(
            compiled.wire()["contents"][2]["parts"][0]["functionResponse"]["response"],
            json!({"output":"FIRST"})
        );
        assert_eq!(
            compiled.wire()["contents"][2]["parts"][1]["functionResponse"]["response"],
            json!({"output":"SECOND"})
        );
        assert_eq!(
            compiled.wire()["contents"][2]["parts"][0]["functionResponse"]
                .get("id")
                .cloned(),
            mixed_ids.then(|| json!("native-first"))
        );
        assert!(
            compiled.wire()["contents"][2]["parts"][1]["functionResponse"]
                .get("id")
                .is_none()
        );
        let mut original = native["candidates"][0]["content"].clone();
        original["role"] = "model".into();
        assert_eq!(compiled.wire()["contents"][1], original);
        // Receiving only one result cannot open another assistant call.
        let mut partial = start();
        partial.extend(group.clone());
        partial.push(
            json!({"type":"function_call_output","call_id":group[1]["call_id"],"output":"FIRST"}),
        );
        partial.push(json!({"type":"function_call","call_id":"third","name":"echo","namespace":"local","arguments":"{}"}));
        partial.push(
            json!({"type":"function_call_output","call_id":group[2]["call_id"],"output":"SECOND"}),
        );
        partial.push(json!({"type":"function_call_output","call_id":"third","output":"THIRD"}));
        assert!(
            GenerateContentRequest::from_responses(
                &source(ResponsesDialect::Classic, partial, &declarations),
                MODEL,
                128,
                LIMIT,
                8
            )
            .is_err()
        );
    }
}

// Catch non-executable native calls becoming pending tools on replay.
#[test]
fn unfinished_or_thought_only_native_calls_cannot_be_automatically_replayed() {
    let declarations = declarations();
    let tools = ToolMap::new(&declarations, 8).unwrap();
    let first = compile(&source(ResponsesDialect::Classic, start(), &declarations));
    for reason in ["MAX_TOKENS", "SAFETY", "MALFORMED_FUNCTION_CALL", "FUTURE"] {
        let mut input = start();
        input.extend(projected(
            native_reply(&tools, reason),
            first.wire(),
            &tools,
            "first",
        ));
        input.push(json!({"role":"user","content":"continue"}));
        let error = GenerateContentRequest::from_responses(
            &source(ResponsesDialect::Classic, input, &declarations),
            MODEL,
            128,
            LIMIT,
            8,
        )
        .unwrap_err();
        assert_eq!(error.code, "unsupported_google_replay_call");
    }
    let reply = json!({"candidates":[{"finishReason":"STOP","content":{"parts":[
        {"thought":true,"functionCall":{"name":tools.native_tools()[0]["name"]}},
        {"text":"done"}]}}]});
    let mut input = start();
    input.extend(projected(reply, first.wire(), &tools, "first"));
    input.push(json!({"role":"user","content":"continue"}));
    assert_eq!(
        GenerateContentRequest::from_responses(
            &source(ResponsesDialect::Classic, input, &declarations),
            MODEL,
            128,
            LIMIT,
            8
        )
        .unwrap_err()
        .code,
        "unsupported_google_replay_call"
    );
}

// Catch silent loss of unsupported native semantics and unbounded compiler input.
#[test]
fn unsupported_parameters_and_invalid_input_fail_before_transport() {
    let declarations = declarations();
    let base = source(ResponsesDialect::Classic, start(), &declarations);
    for (key, value, code) in [
        (
            "parallel_tool_calls",
            json!(false),
            "unsupported_google_parallel_tool_calls",
        ),
        (
            "reasoning",
            json!({"effort":"high"}),
            "unsupported_google_reasoning",
        ),
        (
            "text",
            json!({"format":{"type":"json_object"}}),
            "unsupported_google_output_format",
        ),
        ("store", json!(true), "unsupported_google_request"),
        (
            "include",
            json!(["message.output_text.logprobs"]),
            "unsupported_google_runtime_parameter",
        ),
        ("temperature", json!(0.3), "unsupported_google_request"),
        (
            "tool_choice",
            json!({"type":"function","name":"echo"}),
            "unsupported_google_request",
        ),
    ] {
        let mut wire = base.wire().clone();
        wire[key] = value;
        let source = CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap();
        assert_eq!(
            GenerateContentRequest::from_responses(&source, MODEL, 128, LIMIT, 8)
                .unwrap_err()
                .code,
            code
        );
    }
    for choice in ["auto", "required", "none"] {
        let mut wire = base.wire().clone();
        wire["tool_choice"] = choice.into();
        if choice == "none" {
            wire["parallel_tool_calls"] = false.into();
        }
        let source = CanonicalRequest::new(wire, ResponsesDialect::Classic).unwrap();
        assert_eq!(
            compile(&source).wire()["toolConfig"]["functionCallingConfig"]["mode"],
            match choice {
                "auto" => "AUTO",
                "required" => "ANY",
                _ => "NONE",
            }
        );
    }
    for (model, tokens, bytes, max_tools) in [
        ("bad/route", 128, LIMIT, 8),
        (MODEL, 0, LIMIT, 8),
        (MODEL, 128, 8, 8),
        (MODEL, 128, LIMIT, 0),
    ] {
        assert!(
            GenerateContentRequest::from_responses(&base, model, tokens, bytes, max_tools).is_err()
        );
    }
    for extra in [
        json!({"role":"developer","content":"late instruction"}),
        json!({"role":"user","content":[{"type":"input_image","image_url":"file:///private.png"}]}),
        json!({"type":"additional_tools","role":"developer","tools":[]}),
        json!({"type":"reasoning","summary":[]}),
    ] {
        let mut input = start();
        input.push(extra);
        assert!(
            GenerateContentRequest::from_responses(
                &source(ResponsesDialect::Classic, input, &declarations),
                MODEL,
                128,
                LIMIT,
                8
            )
            .is_err()
        );
    }
}
