use caidex_model_core::{CanonicalRequest, ResponseItem, ResponsesDialect};
use caidex_provider_anthropic::{MessagesRequest, RequestOptions, ToolMap};
use serde_json::{Value, json};

fn search() -> Value {
    json!({"type":"tool_search","execution":"client","parameters":{"type":"object","properties":{"query":{"type":"string"}}}})
}
fn deferred(kind: &str) -> Value {
    json!({"type":"function","name":"known","defer_loading":true,"parameters":{"type":"object","properties":{"title":{"type":kind}}},"future":{"keep":true}})
}
fn call(id: &str) -> Value {
    json!({"type":"tool_search_call","execution":"client","call_id":id,"arguments":{"query":"known"}})
}
fn output(id: &str, tools: Vec<Value>) -> Value {
    json!({"type":"tool_search_output","execution":"client","call_id":id,"status":"completed","tools":tools,"future":{"number":18446744073709551616_u128}})
}
fn compile(
    tools: Vec<Value>,
    history: Vec<Value>,
    max: usize,
) -> caidex_model_core::ProviderResult<MessagesRequest> {
    let mut input = vec![json!({"role":"user","content":"find tools"})];
    input.extend(history);
    MessagesRequest::from_responses_with_options(
        &CanonicalRequest::new(
            json!({"model":"alias","input":input,"tools":tools}),
            ResponsesDialect::Classic,
        )
        .unwrap(),
        "native",
        100,
        128 * 1024,
        max,
        &RequestOptions {
            supports_system_messages: true,
            supports_tool_discovery: true,
            expected_organization: Some("11111111-2222-3333-4444-555555555555"),
            ..Default::default()
        },
    )
}

#[test]
fn discovery_references_deferred_tools_deduplicates_reloads_and_appends_schema_revisions() {
    let initial = vec![search(), deferred("string")];
    let mut history = Vec::new();
    for (id, schema) in [("one", "string"), ("two", "string"), ("three", "integer")] {
        history.push(call(id));
        history.push(output(id, vec![deferred(schema)]));
    }
    history.push(
        json!({"type":"function_call","call_id":"run","name":"known","arguments":"{\"title\":9}"}),
    );
    history.push(json!({"type":"function_call_output","call_id":"run","output":"fixture result"}));
    let compiled = compile(initial.clone(), history.clone(), 2).unwrap();
    assert_eq!(
        compiled.wire()["tools"],
        json!(ToolMap::new(&initial, 2).unwrap().native_tools())
    );
    assert_eq!(compiled.wire()["tools"][1]["defer_loading"], true);
    assert_eq!(compiled.source()["input"][2], history[1]);
    let inline: Vec<_> = compiled.wire()["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "system")
        .collect();
    assert_eq!(inline.len(), 2);
    assert_eq!(inline[0]["content"][0]["tool"]["type"], "tool_reference");
    assert_eq!(
        inline[0]["content"][0]["tool"]["name"],
        compiled.wire()["tools"][1]["name"]
    );
    let revision = &inline[1]["content"][0]["tool"]["definition"];
    assert_eq!(
        revision["input_schema"]["properties"]["title"]["type"],
        "integer"
    );
    assert!(revision.get("defer_loading").is_none());
    let result: Value = serde_json::from_str(
        compiled.wire()["messages"][2]["content"][0]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result, history[1]);
    let native = compiled
        .tools()
        .native_call(&ResponseItem::new(history[6].clone()).unwrap())
        .unwrap();
    assert_eq!(
        compiled.tools().responses_call(&native).unwrap().wire()["name"],
        "known"
    );
}

#[test]
fn discovery_cannot_load_unpaired_server_duplicate_wrong_kind_or_over_budget_tools() {
    let initial = vec![search()];
    let result = output("one", vec![deferred("string")]);
    let mut server = result.clone();
    server["execution"] = "server".into();
    let mut wrong_id = result.clone();
    wrong_id["call_id"] = "other".into();
    let mut failed = result.clone();
    failed["status"] = "failed".into();
    let mut malformed = result.clone();
    malformed["tools"][0]["defer_loading"] = "true".into();
    let unknown_tool = output(
        "one",
        vec![json!({"type":"web_search","external_web_access":false})],
    );
    for history in [
        vec![result.clone()],
        vec![call("one"), server],
        vec![call("one"), wrong_id],
        vec![call("one"), failed],
        vec![call("one"), malformed],
        vec![call("one"), unknown_tool],
        vec![call("one"), output("one", vec![search()])],
        vec![
            call("one"),
            json!({"type":"function_call_output","call_id":"one","output":"wrong kind"}),
        ],
        vec![call("one"), result.clone(), call("one"), result.clone()],
    ] {
        assert!(compile(initial.clone(), history, 10).is_err());
    }
    assert!(compile(initial, vec![call("one"), result], 1).is_err());
    let mut unavailable = call("one");
    unavailable["arguments"] = "{\"query\":\"known\"}".into();
    assert!(compile(vec![search()], vec![unavailable], 10).is_err());
}

#[test]
fn discovery_waits_for_all_parallel_results_before_offering_new_tools() {
    let mut known = deferred("string");
    known["defer_loading"] = false.into();
    let custom =
        json!({"type":"custom","name":"new","format":{"type":"text"},"defer_loading":true});
    let compiled = compile(
        vec![search(), known],
        vec![
            call("find"),
            json!({"type":"function_call","name":"known","call_id":"parallel","arguments":"{\"title\":\"existing\"}"}),
            output("find", vec![custom]),
            json!({"type":"function_call_output","call_id":"parallel","output":"parallel result"}),
            json!({"type":"custom_tool_call","name":"new","call_id":"loaded","input":"你好🙂\nraw input"}),
            json!({"type":"custom_tool_call_output","call_id":"loaded","output":"loaded result"}),
        ],
        3,
    ).unwrap();
    let messages = compiled.wire()["messages"].as_array().unwrap();
    assert_eq!(messages[1]["content"].as_array().unwrap().len(), 2);
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"].as_array().unwrap().len(), 2);
    assert_eq!(messages[3]["role"], "system");
    assert_eq!(messages[3]["content"][0]["type"], "tool_addition");
    let restored = compiled
        .tools()
        .responses_call(&messages[4]["content"][0])
        .unwrap();
    assert_eq!(restored.wire()["type"], "custom_tool_call");
    assert_eq!(restored.wire()["input"], "你好🙂\nraw input");
}

#[test]
fn discovery_accepts_user_continuation_after_results_before_the_next_assistant() {
    let mut history = vec![
        call("find"),
        output("find", vec![deferred("string")]),
        json!({"role":"user","content":"also create a second event"}),
    ];
    for next_assistant in [false, true] {
        if next_assistant {
            history.push(json!({"type":"function_call","name":"known","call_id":"run","arguments":"{\"title\":\"second event\"}"}));
            history.push(
                json!({"type":"function_call_output","call_id":"run","output":"fixture result"}),
            );
        }
        let compiled = compile(vec![search()], history.clone(), 2).unwrap();
        let messages = compiled.wire()["messages"].as_array().unwrap();
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"][0]["type"], "tool_result");
        assert_eq!(
            messages[2]["content"][1]["text"],
            "also create a second event"
        );
        assert_eq!(messages[3]["role"], "system");
        assert_eq!(messages[3]["content"][0]["type"], "tool_addition");
        if next_assistant {
            assert_eq!(messages[4]["role"], "assistant");
        }
    }
}
