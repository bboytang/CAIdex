use super::*;
use caidex_model_core::{CanonicalRequest, ResponseItem, ResponsesDialect};
use caidex_provider_anthropic::{
    MessagesRequest, ReasoningMapping, RequestOptions, ServiceTierMapping, SummaryMapping,
    ThinkingContext, ToolMap,
};

#[tokio::test]
async fn compiled_classic_and_lite_requests_replay_signed_custom_history_over_real_http() {
    let declarations = vec![
        json!({"type":"namespace","name":"functions","tools":[{"type":"custom","name":"patch"}]}),
    ];
    let map = ToolMap::new(&declarations, 10).unwrap();
    let call = map.native_call(&ResponseItem::new(json!({"type":"custom_tool_call","namespace":"functions","name":"patch","call_id":"tool-one","input":"\npatch🙂\n"})).unwrap()).unwrap();
    let mut first_reply = reply();
    first_reply["content"].as_array_mut().unwrap().push(call);
    first_reply["stop_reason"] = "tool_use".into();
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let (base, mut requests, _, fixture_task) = fixture(
            vec![(200, first_reply.to_string()), (200, reply().to_string())],
            false,
        )
        .await;
        let (client, reads) = client(&base, Some(KEY), Limits::default());
        let prompt = json!({"type":"message","role":"user","content":"start"});
        let mut wire = if dialect == ResponsesDialect::Lite {
            json!({"model":"alias","input":[{"type":"additional_tools","role":"developer","tools":declarations},prompt],"parallel_tool_calls":false})
        } else {
            json!({"model":"alias","input":[prompt],"tools":declarations,"parallel_tool_calls":false})
        };
        wire["include"] = json!(["reasoning.encrypted_content"]);
        wire["prompt_cache_key"] = json!("fixture-cache-key");
        wire["client_metadata"] =
            json!({"session_id":"fixture-session","future":"fixture-preserved"});
        wire["service_tier"] = json!("default");
        let tiers = [ServiceTierMapping::new("default".into(), "standard_only".into()).unwrap()];
        wire["text"] = json!({"format":{"type":"json_schema","name":"result","strict":true,"schema":{"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"],"additionalProperties":false}}});
        wire["reasoning"] = json!({"effort":"high","summary":"auto","context":"all_turns"});
        let summaries = [SummaryMapping::new("auto".into(), "summarized".into()).unwrap()];
        let mappings = [ReasoningMapping::new(
            "high".into(),
            Some("medium".into()),
            Some(json!({"type":"adaptive","display":"summarized"})),
        )
        .unwrap()];
        let first = MessagesRequest::from_responses_with_options(
            &CanonicalRequest::new(wire.clone(), dialect).unwrap(),
            "native",
            100,
            128 * 1024,
            10,
            &RequestOptions {
                supports_structured_outputs: true,
                retain_runtime_metadata: true,
                service_tier_mappings: &tiers,
                reasoning_mappings: &mappings,
                summary_mappings: &summaries,
                thinking_context: Some(ThinkingContext::AllTurns),
                ..Default::default()
            },
        )
        .unwrap();
        let native = client
            .create_message("native", first.wire().clone(), RequestContext::default())
            .await
            .unwrap();
        let projected = native
            .to_responses_with_tools(first.tools(), 128 * 1024)
            .unwrap();
        assert_eq!(
            projected.output().last().unwrap()["type"],
            "custom_tool_call"
        );
        let mut input = wire["input"].as_array().unwrap().clone();
        input.extend(projected.output().to_vec());
        input.push(
            json!({"type":"custom_tool_call_output","call_id":"tool-one","output":"\nresult🙂\n "}),
        );
        input.push(
            json!({"role":"developer","content":"new instructions after complete tool results"}),
        );
        let mut second_wire = wire.clone();
        second_wire["input"] = input.into();
        let second = MessagesRequest::from_responses_with_options(
            &CanonicalRequest::new(second_wire, dialect).unwrap(),
            "native",
            100,
            128 * 1024,
            10,
            &RequestOptions {
                supports_system_messages: true,
                supports_structured_outputs: true,
                retain_runtime_metadata: true,
                service_tier_mappings: &tiers,
                reasoning_mappings: &mappings,
                summary_mappings: &summaries,
                thinking_context: Some(ThinkingContext::AllTurns),
                expected_organization: None,
                supports_tool_discovery: false,
                verbosity_mappings: &[],
            },
        )
        .unwrap();
        let result = client
            .create_message("native", second.wire().clone(), RequestContext::default())
            .await
            .unwrap();
        assert_eq!(result.wire(), &reply());
        let (_, first_body) = requests.recv().await.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&first_body).unwrap(),
            *first.wire()
        );
        let (head, second_body) = requests.recv().await.unwrap();
        assert!(
            head.to_ascii_lowercase()
                .contains("x-api-key: synthetic_anthropic_key")
        );
        let sent: Value = serde_json::from_slice(&second_body).unwrap();
        assert_eq!(sent["service_tier"], "standard_only");
        for key in ["include", "prompt_cache_key", "client_metadata"] {
            assert!(sent.get(key).is_none());
            assert_eq!(second.source()[key], wire[key]);
        }
        assert_eq!(sent["output_config"]["effort"], "medium");
        assert_eq!(
            sent["output_config"]["format"]["schema"],
            wire["text"]["format"]["schema"]
        );
        assert_eq!(
            sent["thinking"],
            json!({"type":"adaptive","display":"summarized"})
        );
        assert_eq!(sent["messages"][1], native.replay_message());
        assert_eq!(sent["messages"][2]["content"][0]["tool_use_id"], "tool-one");
        assert_eq!(
            sent["messages"][2]["content"][0]["content"][0]["text"],
            "\nresult🙂\n "
        );
        assert_eq!(sent["messages"][3]["role"], "system");
        assert_eq!(
            sent["messages"][3]["content"][0]["text"],
            "new instructions after complete tool results"
        );
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        fixture_task.await.unwrap();
    }
}
