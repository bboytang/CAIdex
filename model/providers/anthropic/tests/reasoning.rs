use caidex_model_core::{CanonicalRequest, ResponsesDialect};
use caidex_provider_anthropic::{MessagesRequest, ReasoningMapping};
use serde_json::{Value, json};
fn compile(
    reasoning: Value,
    mappings: &[ReasoningMapping],
    tokens: u64,
) -> caidex_model_core::ProviderResult<MessagesRequest> {
    let request = CanonicalRequest::new(
        json!({"model":"arbitrary","input":"question","reasoning":reasoning}),
        ResponsesDialect::Classic,
    )
    .unwrap();
    MessagesRequest::from_responses_with_reasoning(
        &request,
        "native",
        tokens,
        128 * 1024,
        20,
        false,
        mappings,
    )
}
#[test]
fn explicit_effort_mapping_separates_native_effort_and_thinking_and_retains_source() {
    let adaptive = ReasoningMapping::new(
        "high".into(),
        Some("medium".into()),
        Some(json!({"type":"adaptive","display":"summarized"})),
    )
    .unwrap();
    let request = compile(json!({"effort":"high"}), &[adaptive], 4096).unwrap();
    assert_eq!(request.wire()["output_config"]["effort"], "medium");
    assert_eq!(
        request.wire()["thinking"],
        json!({"type":"adaptive","display":"summarized"})
    );
    assert_eq!(request.source()["reasoning"], json!({"effort":"high"}));
    let disabled =
        ReasoningMapping::new("none".into(), None, Some(json!({"type":"disabled"}))).unwrap();
    let request = compile(json!({"effort":"none"}), &[disabled], 4096).unwrap();
    assert!(request.wire().get("output_config").is_none());
    assert_eq!(request.wire()["thinking"]["type"], "disabled");
    let effort_only = ReasoningMapping::new("low".into(), Some("low".into()), None).unwrap();
    let request = compile(json!({"effort":"low"}), &[effort_only], 4096).unwrap();
    assert!(request.wire().get("thinking").is_none());
}
#[test]
fn unknown_effort_summary_context_and_malformed_input_never_get_defaulted_or_dropped() {
    let mapping = ReasoningMapping::new("high".into(), Some("high".into()), None).unwrap();
    for reasoning in [
        json!({"effort":"unknown"}),
        json!({"effort":1}),
        json!({"effort":"high","summary":"auto"}),
        json!({"effort":"high","context":"all_turns"}),
        json!({"effort":"high","future":null}),
        json!([]),
    ] {
        assert!(compile(reasoning, std::slice::from_ref(&mapping), 4096).is_err());
    }
    assert!(compile(json!({"effort":"high"}), &[], 4096).is_err());
    for reasoning in [
        Value::Null,
        json!({}),
        json!({"effort":null,"summary":null,"context":null}),
    ] {
        let request = compile(reasoning, &[], 4096).unwrap();
        assert!(request.wire().get("thinking").is_none());
        assert!(request.wire().get("output_config").is_none());
    }
}
#[test]
fn manual_budget_is_explicit_bounded_and_cannot_be_taken_from_client_json() {
    let mapping = ReasoningMapping::new(
        "high".into(),
        None,
        Some(json!({"type":"enabled","budget_tokens":1024})),
    )
    .unwrap();
    assert!(
        compile(
            json!({"effort":"high"}),
            std::slice::from_ref(&mapping),
            1024
        )
        .is_err()
    );
    let request = compile(
        json!({"effort":"high"}),
        std::slice::from_ref(&mapping),
        4096,
    )
    .unwrap();
    assert_eq!(request.wire()["thinking"]["budget_tokens"], 1024);
    assert!(
        compile(
            json!({"effort":"high","budget_tokens":2048}),
            &[mapping],
            4096
        )
        .is_err()
    );
    for thinking in [
        json!({"type":"enabled","budget_tokens":1023}),
        json!({"type":"enabled","budget_tokens":-1}),
        json!({"type":"adaptive","budget_tokens":1024}),
        json!({"type":"disabled","display":"summarized"}),
        json!({"type":"between_tools","display":"summarized"}),
        json!({"type":"adaptive","future":true}),
        json!({"type":"other"}),
    ] {
        assert!(ReasoningMapping::new("high".into(), None, Some(thinking)).is_err());
    }
}
#[test]
fn duplicate_mapping_empty_mapping_and_between_tools_conflicts_are_rejected() {
    assert!(ReasoningMapping::new("high".into(), None, None).is_err());
    assert!(ReasoningMapping::new("bad\nname".into(), Some("high".into()), None).is_err());
    assert!(ReasoningMapping::new("high".into(), Some("ultra".into()), None).is_err());
    assert!(
        ReasoningMapping::new(
            "high".into(),
            Some("max".into()),
            Some(json!({"type":"between_tools"}))
        )
        .is_err()
    );
    let make = || ReasoningMapping::new("high".into(), Some("high".into()), None).unwrap();
    assert!(compile(json!({"effort":"high"}), &[make(), make()], 4096).is_err());
    let between = ReasoningMapping::new(
        "low".into(),
        Some("low".into()),
        Some(json!({"type":"between_tools"})),
    )
    .unwrap();
    assert_eq!(
        compile(json!({"effort":"low"}), &[between], 4096)
            .unwrap()
            .wire()["thinking"]["type"],
        "between_tools"
    );
}
#[test]
fn active_thinking_rejects_forced_tools_and_unsigned_manual_history() {
    let request = CanonicalRequest::new(json!({"model":"alias","input":"question","reasoning":{"effort":"high"},"tools":[{"type":"function","name":"exec","parameters":{"type":"object"}}],"tool_choice":"required"}),ResponsesDialect::Classic).unwrap();
    let mapping = ReasoningMapping::new(
        "high".into(),
        Some("high".into()),
        Some(json!({"type":"adaptive"})),
    )
    .unwrap();
    assert!(
        MessagesRequest::from_responses_with_reasoning(
            &request,
            "native",
            4096,
            128 * 1024,
            20,
            false,
            &[mapping]
        )
        .is_err()
    );
    let request = CanonicalRequest::new(json!({"model":"alias","input":[{"role":"user","content":"question"},{"role":"assistant","content":"unsigned"},{"role":"user","content":"continue"}],"reasoning":{"effort":"high"}}),ResponsesDialect::Classic).unwrap();
    let mapping = ReasoningMapping::new(
        "high".into(),
        None,
        Some(json!({"type":"enabled","budget_tokens":1024})),
    )
    .unwrap();
    assert!(
        MessagesRequest::from_responses_with_reasoning(
            &request,
            "native",
            4096,
            128 * 1024,
            20,
            false,
            &[mapping]
        )
        .is_err()
    );
}

#[test]
fn summary_mapping_overrides_only_display_and_context_requires_exact_profile_policy() {
    use caidex_provider_anthropic::{RequestOptions, SummaryMapping, ThinkingContext};
    let mappings = [ReasoningMapping::new(
        "high".into(),
        Some("medium".into()),
        Some(json!({"type":"adaptive","display":"omitted"})),
    )
    .unwrap()];
    let summaries = [SummaryMapping::new("auto".into(), "summarized".into()).unwrap()];
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let source = json!({"model":"alias","input":[{"role":"user","content":"question"}],"reasoning":{"effort":"high","summary":"auto","context":"all_turns"}});
        let request = CanonicalRequest::new(source.clone(), dialect).unwrap();
        let options = RequestOptions {
            reasoning_mappings: &mappings,
            summary_mappings: &summaries,
            thinking_context: Some(ThinkingContext::AllTurns),
            ..Default::default()
        };
        let compiled = MessagesRequest::from_responses_with_options(
            &request,
            "native",
            4096,
            128 * 1024,
            10,
            &options,
        )
        .unwrap();
        assert_eq!(compiled.source(), &source);
        assert_eq!(
            compiled.wire()["thinking"],
            json!({"type":"adaptive","display":"summarized"})
        );
        assert_eq!(compiled.wire()["output_config"]["effort"], "medium");
        assert!(compiled.wire().get("reasoning").is_none());
        assert!(compiled.wire().get("context_management").is_none());
        for policy in [None, Some(ThinkingContext::CurrentTurn)] {
            let options = RequestOptions {
                thinking_context: policy,
                ..options
            };
            assert!(
                MessagesRequest::from_responses_with_options(
                    &request,
                    "native",
                    4096,
                    128 * 1024,
                    10,
                    &options
                )
                .is_err()
            );
        }
    }
}
#[test]
fn summary_settings_cannot_enable_thinking_and_bad_profiles_are_rejected() {
    use caidex_provider_anthropic::{RequestOptions, SummaryMapping};
    for (source, display) in [
        ("auto", "full"),
        ("auto", "omitted"),
        ("none", "omitted"),
        ("detailed", "updates"),
    ] {
        assert!(SummaryMapping::new(source.into(), display.into()).is_err());
    }
    let make = || SummaryMapping::new("auto".into(), "summarized".into()).unwrap();
    let summaries = [make()];
    for thinking in [
        None,
        Some(json!({"type":"disabled"})),
        Some(json!({"type":"between_tools"})),
    ] {
        let mappings =
            [ReasoningMapping::new("high".into(), Some("medium".into()), thinking).unwrap()];
        let options = RequestOptions {
            reasoning_mappings: &mappings,
            summary_mappings: &summaries,
            ..Default::default()
        };
        for reasoning in [
            json!({"summary":"auto"}),
            json!({"effort":"high","summary":"auto"}),
            json!({"effort":"high","summary":1}),
            json!({"effort":"high","context":1}),
        ] {
            let request = CanonicalRequest::new(
                json!({"model":"alias","input":"q","reasoning":reasoning}),
                ResponsesDialect::Classic,
            )
            .unwrap();
            assert!(
                MessagesRequest::from_responses_with_options(
                    &request,
                    "native",
                    4096,
                    128 * 1024,
                    10,
                    &options
                )
                .is_err()
            );
        }
    }
    let duplicates = [make(), make()];
    let options = RequestOptions {
        summary_mappings: &duplicates,
        ..Default::default()
    };
    let request = CanonicalRequest::new(
        json!({"model":"alias","input":"q"}),
        ResponsesDialect::Classic,
    )
    .unwrap();
    assert!(
        MessagesRequest::from_responses_with_options(
            &request,
            "native",
            4096,
            128 * 1024,
            10,
            &options
        )
        .is_err()
    );
}
#[test]
fn current_turn_policy_accepts_only_its_declared_context_without_stripping_history() {
    use caidex_provider_anthropic::{RequestOptions, ThinkingContext};
    let options = RequestOptions {
        thinking_context: Some(ThinkingContext::CurrentTurn),
        ..Default::default()
    };
    let wire = json!({"model":"alias","input":[{"role":"user","content":"first"},{"role":"assistant","content":"reply"},{"role":"user","content":"next"}],"reasoning":{"context":"current_turn"}});
    let request = CanonicalRequest::new(wire.clone(), ResponsesDialect::Classic).unwrap();
    let result = MessagesRequest::from_responses_with_options(
        &request,
        "native",
        4096,
        128 * 1024,
        10,
        &options,
    )
    .unwrap();
    assert_eq!(result.source(), &wire);
    assert_eq!(result.wire()["messages"].as_array().unwrap().len(), 3);
    let options = RequestOptions::default();
    assert!(
        MessagesRequest::from_responses_with_options(
            &request,
            "native",
            4096,
            128 * 1024,
            10,
            &options
        )
        .is_err()
    );
}

#[test]
fn binding_mappings_enforce_errors_only_in_explicit_adaptive_or_enabled_profiles() {
    for thinking in [
        json!({"type":"adaptive","block_binding":{"prefix_mismatch_behavior":"error"}}),
        json!({"type":"enabled","budget_tokens":1024,"block_binding":{"prefix_mismatch_behavior":"error"}}),
    ] {
        let mappings =
            [ReasoningMapping::new("high".into(), None, Some(thinking.clone())).unwrap()];
        for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
            let request = CanonicalRequest::new(json!({"model":"alias","input":[{"role":"user","content":"q"}],"reasoning":{"effort":"high"}}), dialect).unwrap();
            let compiled = MessagesRequest::from_responses_with_reasoning(
                &request,
                "native",
                4096,
                128 * 1024,
                10,
                false,
                &mappings,
            )
            .unwrap();
            assert_eq!(compiled.wire()["thinking"], thinking);
            assert!(
                compiled.source()["reasoning"]
                    .get("block_binding")
                    .is_none()
            );
            let mut bad = request.wire().clone();
            bad["reasoning"]["block_binding"] = json!({"prefix_mismatch_behavior":"drop_block"});
            assert!(
                MessagesRequest::from_responses_with_reasoning(
                    &CanonicalRequest::new(bad, dialect).unwrap(),
                    "native",
                    4096,
                    128 * 1024,
                    10,
                    false,
                    &mappings
                )
                .is_err()
            );
        }
    }
    for thinking in [
        json!({"type":"adaptive","block_binding":null}),
        json!({"type":"adaptive","block_binding":{}}),
        json!({"type":"adaptive","block_binding":{"prefix_mismatch_behavior":"drop_block"}}),
        json!({"type":"adaptive","block_binding":{"prefix_mismatch_behavior":"error","future":true}}),
        json!({"type":"between_tools","block_binding":{"prefix_mismatch_behavior":"error"}}),
        json!({"type":"disabled","block_binding":{"prefix_mismatch_behavior":"error"}}),
    ] {
        assert!(ReasoningMapping::new("high".into(), None, Some(thinking)).is_err());
    }
}
