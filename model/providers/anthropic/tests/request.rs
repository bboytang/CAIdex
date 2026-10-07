use caidex_model_core::{CanonicalRequest, ResponseItem, ResponsesDialect};
use caidex_provider_anthropic::{MessagesRequest, NativeMessage, ToolMap};
use serde_json::{Value, json};
const LIMIT: usize = 128 * 1024;
fn tools() -> Vec<Value> {
    vec![
        json!({"type":"namespace","name":"functions","tools":[{"type":"function","name":"exec","parameters":{"type":"object"}},{"type":"custom","name":"patch"}]}),
    ]
}
fn compile(
    wire: Value,
    dialect: ResponsesDialect,
) -> caidex_model_core::ProviderResult<MessagesRequest> {
    MessagesRequest::from_responses(
        &CanonicalRequest::new(wire, dialect).unwrap(),
        "native",
        4096,
        LIMIT,
        20,
    )
}
fn history() -> (NativeMessage, Vec<Value>) {
    let map = ToolMap::new(&tools(), 20).unwrap();
    let call = |wire| map.native_call(&ResponseItem::new(wire).unwrap()).unwrap();
    let native = NativeMessage::parse(json!({"type":"message","id":"one","model":"native","role":"assistant","content":[
        {"type":"thinking","thinking":"native","signature":"sig+/=="},
        call(json!({"type":"function_call","name":"exec","namespace":"functions","call_id":"exec-1","arguments":"{}"})),
        call(json!({"type":"custom_tool_call","name":"patch","namespace":"functions","call_id":"patch-1","input":"\npatch🙂\n"})),
        {"type":"future_block","opaque":18446744073709551616_u128}],"stop_reason":"tool_use","usage":{"input_tokens":1,"output_tokens":2}})).unwrap();
    let output = native
        .to_responses_with_tools(&map, LIMIT)
        .unwrap()
        .output()
        .to_vec();
    (native, output)
}
#[test]
fn classic_and_lite_compile_equivalent_instructions_tools_and_conversation() {
    let declarations = tools();
    let common =
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"中文🙂"}]});
    let classic = compile(json!({"model":"alias","instructions":"system","input":[common],"tools":declarations,"stream":true,"store":false,"parallel_tool_calls":false,"tool_choice":"required"}), ResponsesDialect::Classic).unwrap();
    let lite = compile(json!({"model":"alias","input":[{"type":"additional_tools","role":"developer","tools":declarations},{"type":"message","role":"developer","content":[{"type":"input_text","text":"system"}]},common],"stream":true,"store":false,"parallel_tool_calls":false,"tool_choice":"required"}),ResponsesDialect::Lite).unwrap();
    assert_eq!(classic.wire(), lite.wire());
    assert_eq!(classic.wire()["model"], "native");
    assert_eq!(classic.wire()["max_tokens"], 4096);
    assert_eq!(
        classic.wire()["tool_choice"],
        json!({"type":"any","disable_parallel_tool_use":true})
    );
    assert!(classic.source().get("instructions").is_some());
    assert!(!format!("{classic:?}").contains("system"));
}
#[test]
fn signed_history_and_custom_results_survive_removed_current_tools_and_both_dialects() {
    let (native, output) = history();
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let mut input = vec![json!({"role":"user","content":"start"})];
        input.extend(output.clone());
        input.extend([
            json!({"type":"custom_tool_call_output","call_id":"patch-1","output":"\nexact🙂\n "}),
            json!({"type":"function_call_output","name":"exec","namespace":"functions","output":[{"type":"input_text","text":"done"}]}),
            json!({"role":"user","content":"continue"}),
        ]);
        let request = compile(json!({"model":"alias","input":input}), dialect).unwrap();
        assert_eq!(request.wire()["messages"][1], native.replay_message());
        assert!(request.wire().get("tools").is_none());
        let content = &request.wire()["messages"][2]["content"];
        assert_eq!(content[0]["tool_use_id"], "patch-1");
        assert_eq!(content[0]["content"][0]["text"], "\nexact🙂\n ");
        assert_eq!(content[1]["tool_use_id"], "exec-1");
        assert_eq!(content[2]["text"], "continue");
    }
}
#[test]
fn results_reject_wrong_id_kind_identity_duplicates_and_missing_parallel_results() {
    let (_, output) = history();
    for results in [
        vec![json!({"type":"function_call_output","call_id":"unknown","output":"x"})],
        vec![json!({"type":"function_call_output","call_id":"patch-1","output":"x"})],
        vec![
            json!({"type":"function_call_output","call_id":"exec-1","namespace":"wrong","output":"x"}),
        ],
        vec![
            json!({"type":"function_call_output","call_id":"exec-1","output":"x"}),
            json!({"type":"function_call_output","call_id":"exec-1","output":"again"}),
        ],
        vec![
            json!({"type":"custom_tool_call_output","call_id":"patch-1","output":"x"}),
            json!({"role":"user","content":"missing exec result"}),
        ],
    ] {
        let mut input = output.clone();
        input.extend(results);
        assert!(
            compile(
                json!({"model":"alias","input":input}),
                ResponsesDialect::Classic
            )
            .is_err()
        );
    }
}
#[test]
fn legacy_name_only_parallel_results_are_rejected_when_ambiguous() {
    let call = |id| json!({"type":"function_call","namespace":"functions","name":"exec","call_id":id,"arguments":"{}"});
    let input = json!([{"role":"user","content":"start"},call("one"),call("two"),{"type":"function_call_output","name":"exec","namespace":"functions","output":"ambiguous"}]);
    assert!(
        compile(
            json!({"model":"alias","tools":tools(),"input":input}),
            ResponsesDialect::Classic
        )
        .is_err()
    );
}
#[test]
fn plain_calls_and_results_are_grouped_without_executing_tools() {
    let request = compile(json!({"model":"alias","tools":tools(),"input":[{"role":"user","content":"start"},{"type":"function_call","namespace":"functions","name":"exec","call_id":"one","arguments":"{\"n\":18446744073709551616}"},{"type":"function_call_output","call_id":"one","output":"result"}]}),ResponsesDialect::Classic).unwrap();
    assert_eq!(
        request.wire()["messages"][1]["content"][0]["input"],
        json!({"n":18446744073709551616_u128})
    );
    assert_eq!(
        request.wire()["messages"][2]["content"][0]["tool_use_id"],
        "one"
    );
}
#[test]
fn malformed_unsupported_parameters_late_instructions_and_foreign_history_fail_explicitly() {
    for wire in [
        json!({"model":"alias","input":"text","reasoning":{"effort":"high"}}),
        json!({"model":"alias","input":"text","store":true}),
        json!({"model":"alias","input":"text","tools":{},"parallel_tool_calls":false}),
        json!({"model":"alias","input":[{"role":"user","content":"start"},{"role":"developer","content":"late"}]}),
        json!({"model":"alias","input":[{"type":"reasoning","summary":[],"encrypted_content":"foreign"}]}),
        json!({"model":"alias","input":[{"type":"additional_tools","role":"developer","tools":[]}]}),
        json!({"model":"alias","input":[],"tool_choice":"required"}),
    ] {
        assert!(compile(wire, ResponsesDialect::Classic).is_err());
    }
    let request = CanonicalRequest::new(
        json!({"model":"alias","input":"text"}),
        ResponsesDialect::Classic,
    )
    .unwrap();
    for (model, tokens, limit) in [("", 1, LIMIT), ("native", 0, LIMIT), ("native", 1, 1)] {
        assert!(MessagesRequest::from_responses(&request, model, tokens, limit, 20).is_err());
    }
}
#[test]
fn image_content_keeps_order_and_bytes_in_messages_and_tool_results_without_fetching() {
    let image =
        json!({"type":"input_image","image_url":"data:image/png;base64,aW1hZ2U=","detail":"auto"});
    let url = json!({"type":"input_image","image_url":"https://example.invalid/image.png"});
    let result = compile(json!({"model":"alias","tools":tools(),"input":[{"role":"user","content":[image,url,{"type":"input_text","text":"after images"}]},{"type":"function_call","namespace":"functions","name":"exec","call_id":"one","arguments":"{}"},{"type":"function_call_output","call_id":"one","output":[image]}]}),ResponsesDialect::Classic).unwrap();
    assert_eq!(
        result.wire()["messages"][0]["content"][0]["source"],
        json!({"type":"base64","media_type":"image/png","data":"aW1hZ2U="})
    );
    assert_eq!(
        result.wire()["messages"][0]["content"][1]["source"]["type"],
        "url"
    );
    assert_eq!(
        result.wire()["messages"][0]["content"][2]["text"],
        "after images"
    );
    assert_eq!(
        result.wire()["messages"][2]["content"][0]["content"][0],
        result.wire()["messages"][0]["content"][0]
    );
    for image in [
        json!({"type":"input_image","image_url":"data:image/png;base64,%%%"}),
        json!({"type":"input_image","image_url":"file:///tmp/private"}),
        json!({"type":"input_image","image_url":"https://user:secret@example.invalid/image"}),
        json!({"type":"input_image","image_url":"https://example.invalid/image","detail":"low"}),
        json!({"type":"input_image","file_id":"foreign"}),
    ] {
        assert!(
            compile(
                json!({"model":"alias","input":[{"role":"user","content":[image]}]}),
                ResponsesDialect::Classic
            )
            .is_err()
        );
    }
}
#[test]
fn partially_delivered_parallel_results_cannot_be_interleaved_with_new_calls() {
    let call = |id| json!({"type":"function_call","name":"exec","namespace":"functions","call_id":id,"arguments":"{}"});
    let input = json!([{"role":"user","content":"start"},call("one"),call("two"),{"type":"function_call_output","call_id":"one","output":"done"},call("three"),{"type":"function_call_output","call_id":"two","output":"done"},{"type":"function_call_output","call_id":"three","output":"done"}]);
    assert!(
        compile(
            json!({"model":"alias","tools":tools(),"input":input}),
            ResponsesDialect::Classic
        )
        .is_err()
    );
    assert!(
        compile(
            json!({"model":"alias","input":"text","parallel_tool_calls":"false"}),
            ResponsesDialect::Classic
        )
        .is_err()
    );
}

fn compile_system(
    input: Vec<Value>,
    supported: bool,
) -> caidex_model_core::ProviderResult<MessagesRequest> {
    let request = CanonicalRequest::new(
        json!({"model":"arbitrary-alias","input":input}),
        ResponsesDialect::Classic,
    )
    .unwrap();
    MessagesRequest::from_responses_with_system_messages(
        &request,
        "arbitrary-native",
        4096,
        LIMIT,
        20,
        supported,
    )
}
#[test]
fn mid_conversation_system_capability_keeps_instruction_position_and_consecutive_order() {
    let input = vec![
        json!({"role":"developer","content":"initial"}),
        json!({"role":"user","content":"question"}),
        json!({"role":"developer","content":"new instruction"}),
        json!({"role":"system","content":"second instruction"}),
        json!({"role":"assistant","content":"answer"}),
        json!({"role":"user","content":"followup"}),
    ];
    assert!(compile_system(input.clone(), false).is_err());
    let compiled = compile_system(input.clone(), true).unwrap();
    assert_eq!(compiled.wire()["system"][0]["text"], "initial");
    let messages = compiled.wire()["messages"].as_array().unwrap();
    assert_eq!(
        messages
            .iter()
            .map(|v| v["role"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["user", "system", "assistant", "user"]
    );
    assert_eq!(
        messages[1]["content"],
        json!([{"type":"text","text":"new instruction"},{"type":"text","text":"second instruction"}])
    );
    assert_eq!(compiled.source()["input"], json!(input));
}
#[test]
fn system_messages_reject_wrong_positions_images_turn_scopes_and_partial_tool_results() {
    let user = json!({"role":"user","content":"question"});
    let assistant = json!({"role":"assistant","content":"answer"});
    let system = json!({"role":"developer","content":"new"});
    for input in [
        vec![user.clone(), system.clone(), user.clone()],
        vec![user.clone(), assistant.clone(), system.clone()],
        vec![
            user.clone(),
            json!({"role":"developer","content":[{"type":"input_image","image_url":"data:image/png;base64,aW1hZ2U="}]}),
        ],
        vec![
            user.clone(),
            json!({"role":"system","content":"temporary","clear_at":"next_user_message"}),
        ],
        vec![
            user.clone(),
            json!({"role":"system","content":"effort","output_config":{"effort":"low"}}),
        ],
    ] {
        assert!(compile_system(input, true).is_err());
    }
    assert!(compile_system(vec![user, system], true).is_ok());
    let (_, mut input) = history();
    input.push(json!({"type":"custom_tool_call_output","call_id":"patch-1","output":"done"}));
    input.push(json!({"role":"developer","content":"too early"}));
    let request = CanonicalRequest::new(
        json!({"model":"alias","input":input}),
        ResponsesDialect::Classic,
    )
    .unwrap();
    assert!(
        MessagesRequest::from_responses_with_system_messages(
            &request, "native", 4096, LIMIT, 20, true
        )
        .is_err()
    );
}
#[test]
fn system_messages_after_complete_tool_results_preserve_signed_history() {
    let (native, mut input) = history();
    input.extend([
        json!({"type":"custom_tool_call_output","call_id":"patch-1","output":"done"}),
        json!({"type":"function_call_output","call_id":"exec-1","output":"done"}),
        json!({"role":"developer","content":"continue under new instructions"}),
    ]);
    let request = CanonicalRequest::new(
        json!({"model":"alias","input":input}),
        ResponsesDialect::Classic,
    )
    .unwrap();
    let compiled = MessagesRequest::from_responses_with_system_messages(
        &request, "native", 4096, LIMIT, 20, true,
    )
    .unwrap();
    assert_eq!(compiled.wire()["messages"][0], native.replay_message());
    assert_eq!(compiled.wire()["messages"][2]["role"], "system");
    assert_eq!(
        compiled.wire()["messages"][2]["content"][0]["text"],
        "continue under new instructions"
    );
}
#[test]
fn system_after_native_server_result_preserves_paused_content_without_client_execution() {
    let native = NativeMessage::parse(json!({"type":"message","role":"assistant","model":"native","id":"paused","content":[{"type":"thinking","thinking":"private","signature":"signed+/=="},{"type":"server_tool_use","id":"srv-one","name":"web_fetch","input":{}},{"type":"web_fetch_tool_result","tool_use_id":"srv-one","content":{"type":"future_result","data":"opaque"}}],"stop_reason":"pause_turn","usage":{"input_tokens":1,"output_tokens":2}})).unwrap();
    let mut input = native.to_responses(LIMIT).unwrap().output().to_vec();
    assert_eq!(input.len(), 1);
    input.push(json!({"role":"developer","content":"new instruction"}));
    let request = CanonicalRequest::new(
        json!({"model":"alias","input":input}),
        ResponsesDialect::Classic,
    )
    .unwrap();
    let compiled = MessagesRequest::from_responses_with_system_messages(
        &request, "native", 4096, LIMIT, 20, true,
    )
    .unwrap();
    assert_eq!(compiled.wire()["messages"][0], native.replay_message());
    assert_eq!(compiled.wire()["messages"][1]["role"], "system");
    assert!(compiled.wire().get("tools").is_none());
}
