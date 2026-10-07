use super::*;
use caidex_model_core::{
    CanonicalRequest, CapabilitySupport, CredentialRequirement, EvidenceSource, ModelMetadata,
    ModelProvider, ResponsesDialect,
};
use caidex_provider_anthropic::{
    AnthropicModel, AnthropicProvider, NativeMessage, ReasoningMapping, ServiceTierMapping,
    SummaryMapping, ThinkingContext, ToolMap,
};

pub(super) fn profile() -> AnthropicModel {
    AnthropicModel::new(
        ModelMetadata::configured(
            "alias".into(),
            "native".into(),
            vec![ResponsesDialect::Classic, ResponsesDialect::Lite],
        ),
        100,
        10,
    )
}
fn canonical(dialect: ResponsesDialect, tools: &[Value], stream: bool) -> CanonicalRequest {
    let prompt = json!({"role":"user","content":"hello"});
    let wire = match dialect {
        ResponsesDialect::Classic => {
            json!({"model":"alias","input":[prompt],"tools":tools,"stream":stream})
        }
        ResponsesDialect::Lite => {
            json!({"model":"alias","input":[{"type":"additional_tools","role":"developer","tools":tools},prompt],"stream":stream})
        }
    };
    CanonicalRequest::new(wire, dialect).unwrap()
}

#[tokio::test]
async fn provider_json_binding_reports_reject_lossy_history_without_retry_for_both_dialects() {
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        for (kind, reason, code) in [
            (
                "thinking_dropped",
                "prefix_binding_mismatch",
                Some("anthropic_input_thinking_dropped"),
            ),
            (
                "thinking_dropped",
                "model_binding_mismatch",
                Some("anthropic_input_thinking_dropped"),
            ),
            (
                "thinking_dropped",
                "organization_binding_mismatch",
                Some("anthropic_input_thinking_dropped"),
            ),
            (
                "thinking_dropped",
                "end_user_binding_mismatch",
                Some("anthropic_input_thinking_dropped"),
            ),
            (
                "thinking_mismatch_allowed",
                "prefix_binding_mismatch",
                Some("anthropic_input_binding_mismatch"),
            ),
            ("future_transform", "future_reason", None),
        ] {
            let mut native = reply();
            native["input_transformations"] = json!([{"type":kind,"reason":reason,"path":"messages.1.content.0","private":"PRIVATE_REPORT"}]);
            let (base, mut requests, _, task) =
                fixture(vec![(200, native.to_string())], false).await;
            let (client, reads) = client(&base, Some(KEY), Limits::default());
            let provider = AnthropicProvider::new(client, vec![profile()], 10).unwrap();
            let result = provider
                .create_response(canonical(dialect, &[], false), RequestContext::default())
                .await;
            if let Some(code) = code {
                let error = result.err().unwrap();
                assert_eq!(error.code, code);
                assert_eq!(error.http_status, 502);
                assert!(!format!("{error:?}").contains("PRIVATE_REPORT"));
            } else {
                let response = result.unwrap().response;
                let restored =
                    NativeMessage::from_responses_output(response.output(), "native", 128 * 1024)
                        .unwrap();
                assert_eq!(restored.wire(), &native);
            }
            assert_eq!(reads.load(Ordering::SeqCst), 1);
            assert!(received(&mut requests).await.0.starts_with("POST "));
            task.await.unwrap();
            assert!(requests.try_recv().is_err());
        }
    }
}

#[tokio::test]
async fn native_json_keeps_actual_fallback_identity_and_fixed_provider_never_relabels_it() {
    let mut wire = reply();
    wire["model"] = "serving".into();
    wire["content"].as_array_mut().unwrap().insert(0, json!({"type":"fallback","from":{"model":"native"},"to":{"model":"serving"},"future":"retain"}));
    wire["usage"]["iterations"] = json!([{"type":"message","model":"native","input_tokens":90},{"type":"fallback_message","model":"serving","input_tokens":2}]);
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let (base, mut requests, _, task) = fixture(
            vec![(200, wire.to_string()), (200, wire.to_string())],
            false,
        )
        .await;
        let (client, reads) = client(&base, Some(KEY), Limits::default());
        let native = client
            .create_message("native", request(), RequestContext::default())
            .await
            .unwrap();
        assert_eq!(native.wire(), &wire);
        assert_eq!(native.model(), "serving");
        assert_eq!(native.replay_message()["content"], wire["content"]);
        let provider = AnthropicProvider::new(client, vec![profile()], 10).unwrap();
        let error = provider
            .create_response(canonical(dialect, &[], false), RequestContext::default())
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, "anthropic_response_model_mismatch");
        assert_eq!(error.http_status, 502);
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        received(&mut requests).await;
        received(&mut requests).await;
        task.await.unwrap();
        assert!(requests.try_recv().is_err());
    }
}

#[tokio::test]
async fn provider_catalog_filters_profiles_and_api_key_requirement_is_safe_metadata() {
    let (base, mut requests, _, task) =
        fixture(vec![(200, page("native", false).to_string())], false).await;
    let (client, reads) = client(&base, Some(KEY), Limits::default());
    let mut missing = profile();
    missing.metadata.id = "missing".into();
    missing.metadata.native_model = "not-available".into();
    let provider = AnthropicProvider::new(client, vec![profile(), missing], 10).unwrap();
    let requirement = provider.credential_requirements("alias").unwrap();
    assert!(
        matches!(&requirement, CredentialRequirement::ApiKey { reference: r } if r == &reference())
    );
    let serialized = serde_json::to_string(&requirement).unwrap();
    assert!(serialized.contains("apiKey"));
    assert!(!serialized.contains(KEY));
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(
        provider.metadata("alias").unwrap().source,
        EvidenceSource::Configured
    );
    assert_eq!(
        provider.capabilities("alias").unwrap().streaming,
        CapabilitySupport::Unknown
    );
    let models = provider.list_models().await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "alias");
    assert_eq!(models[0].native_model, "native");
    assert_eq!(models[0].source, EvidenceSource::ProviderCatalog);
    assert!(models[0].codex_compatibility.is_none());
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert!(received(&mut requests).await.0.starts_with("GET "));
    task.await.unwrap();
}

#[tokio::test]
async fn provider_classic_and_lite_create_replay_signed_custom_history_without_second_transport() {
    let tools = [json!({"type":"custom","name":"patch"})];
    let aliases = ToolMap::new(&tools, 10).unwrap();
    let mut first_reply = reply();
    first_reply["content"].as_array_mut().unwrap().push(json!({"type":"tool_use","id":"tool-one","name":aliases.native_tools()[0]["name"],"input":{"input":"\npatch🙂\n"}}));
    first_reply["stop_reason"] = "tool_use".into();
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let (base, mut requests, _, task) = fixture(
            vec![(200, first_reply.to_string()), (200, reply().to_string())],
            false,
        )
        .await;
        let (client, reads) = client(&base, Some(KEY), Limits::default());
        let mut model = profile();
        model.retain_runtime_metadata = true;
        model.supports_structured_outputs = true;
        model.thinking_context = Some(ThinkingContext::AllTurns);
        model.summary_mappings =
            vec![SummaryMapping::new("auto".into(), "summarized".into()).unwrap()];
        model.service_tier_mappings =
            vec![ServiceTierMapping::new("default".into(), "standard_only".into()).unwrap()];
        model.reasoning_mappings = vec![
            ReasoningMapping::new(
                "high".into(),
                Some("medium".into()),
                Some(json!({"type":"adaptive"})),
            )
            .unwrap(),
        ];
        let provider = AnthropicProvider::new(client, vec![model], 10).unwrap();
        let mut wire = canonical(dialect, &tools, false).wire().clone();
        wire["reasoning"] = json!({"effort":"high","summary":"auto","context":"all_turns"});
        wire["service_tier"] = "default".into();
        wire["client_metadata"] = json!({"session_id":"fixture"});
        wire["prompt_cache_key"] = "fixture-cache".into();
        wire["include"] = json!(["reasoning.encrypted_content"]);
        wire["text"] = json!({"format":{"type":"json_schema","name":"result","strict":true,"schema":{"type":"object"}}});
        let first_request = CanonicalRequest::new(wire, dialect).unwrap();
        let response = provider
            .create_response(first_request.clone(), RequestContext::default())
            .await
            .unwrap()
            .response;
        assert_eq!(response.output()[2]["type"], "custom_tool_call");
        assert_eq!(response.output()[2]["input"], "\npatch🙂\n");
        let native =
            NativeMessage::from_responses_output(response.output(), "native", 128 * 1024).unwrap();
        assert_eq!(native.wire(), &first_reply);
        let mut second_wire = first_request.wire().clone();
        let input = second_wire["input"].as_array_mut().unwrap();
        input.extend_from_slice(response.output());
        input.push(json!({"type":"custom_tool_call_output","call_id":"tool-one","output":"done"}));
        let second = provider
            .create_response(
                CanonicalRequest::new(second_wire, dialect).unwrap(),
                RequestContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(second.response.wire()["status"], "completed");
        let (head, first_body) = received(&mut requests).await;
        assert!(
            head.to_ascii_lowercase()
                .contains("x-api-key: synthetic_anthropic_key")
        );
        let first_wire: Value = serde_json::from_slice(&first_body).unwrap();
        assert_eq!(first_wire["model"], "native");
        assert_eq!(first_wire["max_tokens"], 100);
        assert_eq!(first_wire["service_tier"], "standard_only");
        assert_eq!(first_wire["output_config"]["effort"], "medium");
        assert_eq!(
            first_wire["output_config"]["format"]["schema"],
            json!({"type":"object"})
        );
        assert_eq!(
            first_wire["thinking"],
            json!({"type":"adaptive","display":"summarized"})
        );
        for field in ["include", "client_metadata", "prompt_cache_key"] {
            assert!(first_wire.get(field).is_none());
        }
        let (_, second_body) = received(&mut requests).await;
        let second_wire: Value = serde_json::from_slice(&second_body).unwrap();
        assert_eq!(second_wire["messages"][1], native.replay_message());
        assert_eq!(
            second_wire["messages"][2]["content"][0]["tool_use_id"],
            "tool-one"
        );
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        task.await.unwrap();
    }
}

#[tokio::test]
async fn provider_wrong_model_and_oversized_projection_fail_without_returning_history() {
    for oversized in [false, true] {
        let mut native = reply();
        let mut limits = Limits::default();
        if oversized {
            // Native body and carrier fit; the full Responses wrapper does not.
            let response = NativeMessage::parse(native.clone())
                .unwrap()
                .to_responses_with_tools(&ToolMap::new(&[], 10).unwrap(), 128 * 1024)
                .unwrap();
            limits.response_bytes = response.output()[0]["encrypted_content"]
                .as_str()
                .unwrap()
                .len();
            assert!(native.to_string().len() < limits.response_bytes);
        } else {
            native["model"] = "other-version".into();
        }
        let (base, mut requests, _, task) = fixture(vec![(200, native.to_string())], false).await;
        let (client, _) = client(&base, Some(KEY), limits);
        let provider = AnthropicProvider::new(client, vec![profile()], 10).unwrap();
        let result = provider
            .create_response(
                canonical(ResponsesDialect::Classic, &[], false),
                RequestContext::default(),
            )
            .await;
        assert_eq!(
            result.err().unwrap().code,
            if oversized {
                "anthropic_projection_too_large"
            } else {
                "anthropic_response_model_mismatch"
            }
        );
        received(&mut requests).await;
        task.await.unwrap();
    }
}

#[tokio::test]
async fn provider_preflight_rejects_unknown_routes_dialects_modes_and_native_context_before_key_reads()
 {
    let (client, reads) = client("http://127.0.0.1:1/v1", Some(KEY), Limits::default());
    let mut model = profile();
    model.metadata.dialects = vec![ResponsesDialect::Classic];
    model.metadata.capabilities.streaming = CapabilitySupport::Unsupported;
    let provider = AnthropicProvider::new(client, vec![model], 10).unwrap();
    assert_eq!(
        provider.metadata("unknown").unwrap_err().code,
        "unknown_model"
    );
    assert_eq!(
        provider
            .credential_requirements("unknown")
            .unwrap_err()
            .code,
        "unknown_model"
    );
    for (request, expected) in [
        (
            canonical(ResponsesDialect::Lite, &[], false),
            "unsupported_dialect",
        ),
        (
            CanonicalRequest::new(
                json!({"model":"unknown","input":"hello"}),
                ResponsesDialect::Classic,
            )
            .unwrap(),
            "unknown_model",
        ),
    ] {
        assert_eq!(
            provider
                .create_response(request, RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            expected
        );
    }
    assert_eq!(
        provider
            .stream_response(
                canonical(ResponsesDialect::Classic, &[], false),
                RequestContext::default()
            )
            .await
            .err()
            .unwrap()
            .code,
        "invalid_model_request"
    );
    assert_eq!(
        provider
            .create_response(
                canonical(ResponsesDialect::Classic, &[], true),
                RequestContext::default()
            )
            .await
            .err()
            .unwrap()
            .code,
        "invalid_model_request"
    );
    assert_eq!(
        provider
            .stream_response(
                canonical(ResponsesDialect::Classic, &[], true),
                RequestContext::default()
            )
            .await
            .err()
            .unwrap()
            .code,
        "unsupported_streaming"
    );
    let mut context = RequestContext::default();
    context
        .headers
        .insert(
            "session_id",
            "local-session".into(),
            caidex_model_core::REQUEST_HEADERS,
        )
        .unwrap();
    assert_eq!(
        provider
            .create_response(canonical(ResponsesDialect::Classic, &[], false), context)
            .await
            .err()
            .unwrap()
            .code,
        "unsupported_native_context_header"
    );
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert_eq!(
        provider
            .create_response(
                canonical(ResponsesDialect::Classic, &[], false),
                RequestContext {
                    cancellation,
                    ..Default::default()
                }
            )
            .await
            .err()
            .unwrap()
            .code,
        "provider_cancelled"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[test]
fn provider_profiles_reject_duplicate_ids_invalid_budgets_and_false_full_reports() {
    for scenario in 0..5 {
        let (client, _) = client("http://127.0.0.1:1/v1", Some(KEY), Limits::default());
        let mut model = profile();
        let mut models = Vec::new();
        match scenario {
            0 => model.max_tokens = 0,
            1 => model.max_tools = 0,
            2 => {
                model.metadata.capabilities.output_limit = Some(99);
            }
            3 => models.push(profile()),
            _ => {
                model.metadata.codex_compatibility = Some(caidex_model_core::CompatibilityReport {
                    schema_version: 1,
                    level: caidex_model_core::CompatibilityLevel::Full,
                    source: EvidenceSource::ProtocolFixture,
                    reference: "fixture".into(),
                    tested_model_version: "native".into(),
                    limitations: vec![],
                })
            }
        }
        models.push(model);
        assert!(AnthropicProvider::new(client, models, 10).is_err());
    }
    for dialects in [
        vec![ResponsesDialect::Lite],
        vec![ResponsesDialect::Lite, ResponsesDialect::Classic],
    ] {
        let (client, reads) = client("http://127.0.0.1:1/v1", Some(KEY), Limits::default());
        let mut model = profile();
        model.metadata.dialects = dialects.clone();
        let provider = AnthropicProvider::new(client, vec![model], 10).unwrap();
        assert_eq!(provider.metadata("alias").unwrap().dialects, dialects);
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn provider_binding_mapping_and_beta_are_paired_for_classic_and_lite() {
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        for report in [
            None,
            Some(Value::Null),
            Some(json!([])),
            Some(
                json!([{"type":"thinking_dropped","reason":"organization_binding_mismatch","path":"messages.1.content.0","private":"PRIVATE_REPORT"}]),
            ),
        ] {
            let mut native = reply();
            if let Some(report) = &report {
                native["input_transformations"] = report.clone();
            }
            let (base, mut requests, _, task) =
                fixture(vec![(200, native.to_string())], false).await;
            let (client, reads) = binding_client(&base, Limits::default());
            let mut profile = profile();
            profile.reasoning_mappings = vec![ReasoningMapping::new("high".into(),None,Some(json!({"type":"adaptive","block_binding":{"prefix_mismatch_behavior":"error"}}))).unwrap()];
            let provider = AnthropicProvider::new(client, vec![profile], 10).unwrap();
            let mut wire = canonical(dialect, &[], false).wire().clone();
            wire["reasoning"] = json!({"effort":"high"});
            let result = provider
                .create_response(
                    CanonicalRequest::new(wire, dialect).unwrap(),
                    RequestContext::default(),
                )
                .await;
            match report.as_ref().and_then(Value::as_array) {
                Some(entries) if entries.is_empty() => {
                    assert_eq!(result.unwrap().response.wire()["status"], "completed")
                }
                Some(_) => assert_eq!(
                    result.err().unwrap().code,
                    "anthropic_input_thinking_dropped"
                ),
                None => assert_eq!(
                    result.err().unwrap().code,
                    "anthropic_binding_report_missing"
                ),
            }
            let (head, body) = received(&mut requests).await;
            assert!(
                head.to_ascii_lowercase()
                    .contains("anthropic-beta: thinking-binding-controls-2026-08-01")
            );
            let sent: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                sent["thinking"],
                json!({"type":"adaptive","block_binding":{"prefix_mismatch_behavior":"error"}})
            );
            assert!(!head.contains("drop_block"));
            assert_eq!(reads.load(Ordering::SeqCst), 1);
            task.await.unwrap();
            assert!(requests.try_recv().is_err());
        }
    }
}
