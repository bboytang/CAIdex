use caidex_model_core::{CanonicalRequest, ResponsesDialect};
use caidex_provider_google::{
    GenerateContentRequest, NativeHistory, NativeResponse, ReasoningMapping, RequestOptions,
    SummaryMapping, ThinkingContext,
};
use serde_json::{Value, json};
const MODEL: &str = "models/fixture-thinking";
const LIMIT: usize = 256 * 1024;
fn request(dialect: ResponsesDialect, reasoning: Value, mut input: Vec<Value>) -> CanonicalRequest {
    if dialect == ResponsesDialect::Lite {
        input.insert(
            0,
            json!({"type":"additional_tools","role":"developer","tools":[]}),
        );
    }
    CanonicalRequest::new(
        json!({"model":"alias","input":input,"reasoning":reasoning}),
        dialect,
    )
    .unwrap()
}
fn question(dialect: ResponsesDialect, reasoning: Value) -> CanonicalRequest {
    request(
        dialect,
        reasoning,
        vec![json!({"role":"user","content":"question"})],
    )
}
fn compile(
    source: &CanonicalRequest,
    options: &RequestOptions<'_>,
) -> caidex_model_core::ProviderResult<GenerateContentRequest> {
    GenerateContentRequest::from_responses_with_options(source, MODEL, 128, LIMIT, 8, options)
}
fn summaries() -> Vec<SummaryMapping> {
    [
        ("auto", true),
        ("concise", true),
        ("detailed", true),
        ("none", false),
    ]
    .into_iter()
    .map(|(source, include)| SummaryMapping::new(source.into(), include).unwrap())
    .collect()
}

// Catches model-name guessing, merging budget/level, changing configured values,
// or mistaking maxOutputTokens (which includes thought tokens) for an effort cap.
#[test]
fn explicit_effort_mappings_preserve_native_budget_level_and_source_in_both_dialects() {
    for (effort, native) in [
        ("none", json!({"thinkingBudget":0})),
        ("dynamic", json!({"thinkingBudget":-1})),
        ("minimal", json!({"thinkingLevel":"MINIMAL"})),
        ("low", json!({"thinkingLevel":"LOW"})),
        ("medium", json!({"thinkingLevel":"MEDIUM"})),
        (
            "high",
            json!({"thinkingLevel":"HIGH","includeThoughts":false}),
        ),
        ("xhigh", json!({"thinkingBudget":32768})),
        ("future-effort", json!({"thinkingBudget":1024})),
    ] {
        let mappings = [ReasoningMapping::new(effort.into(), native.clone()).unwrap()];
        let options = RequestOptions {
            reasoning_mappings: &mappings,
            ..Default::default()
        };
        for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
            let source = question(dialect, json!({"effort":effort}));
            let compiled = compile(&source, &options).unwrap();
            assert_eq!(
                compiled.wire()["generationConfig"],
                json!({"maxOutputTokens":128,"thinkingConfig":native})
            );
            assert_eq!(compiled.source(), source.wire());
            assert!(compiled.wire().get("reasoning").is_none());
            assert_eq!(
                compile(&source, &Default::default()).unwrap_err().code,
                "unsupported_google_reasoning"
            );
        }
    }
}

// Catches summary requests silently being omitted, changing the reasoning mode,
// or includeThoughts being used to pretend that disabled thinking is enabled.
#[test]
fn summaries_change_only_native_display_without_enabling_disabled_thinking() {
    let summaries = summaries();
    let mappings = [
        ReasoningMapping::new(
            "high".into(),
            json!({"thinkingLevel":"HIGH","includeThoughts":false}),
        )
        .unwrap(),
        ReasoningMapping::new("none".into(), json!({"thinkingBudget":0})).unwrap(),
    ];
    let options = RequestOptions {
        reasoning_mappings: &mappings,
        summary_mappings: &summaries,
        ..Default::default()
    };
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        for (summary, include) in [
            ("auto", true),
            ("concise", true),
            ("detailed", true),
            ("none", false),
        ] {
            let source = question(dialect, json!({"summary":summary}));
            assert_eq!(
                compile(&source, &options).unwrap().wire()["generationConfig"],
                json!({"maxOutputTokens":128,"thinkingConfig":{"includeThoughts":include}})
            );
            let source = question(dialect, json!({"effort":"high","summary":summary}));
            assert_eq!(
                compile(&source, &options).unwrap().wire()["generationConfig"]["thinkingConfig"],
                json!({"thinkingLevel":"HIGH","includeThoughts":include})
            );
        }
        let source = question(dialect, json!({"effort":"none","summary":"none"}));
        assert_eq!(
            compile(&source, &options).unwrap().wire()["generationConfig"]["thinkingConfig"],
            json!({"thinkingBudget":0,"includeThoughts":false})
        );
        let source = question(dialect, json!({"effort":"none","summary":"auto"}));
        assert_eq!(
            compile(&source, &options).unwrap_err().code,
            "unsupported_google_reasoning_summary"
        );
        let source = question(dialect, json!({"summary":"auto"}));
        assert_eq!(
            compile(&source, &Default::default()).unwrap_err().code,
            "unsupported_google_reasoning_summary"
        );
    }
}

// Catches forwarding an invented native context field, accepting an undeclared
// retention policy, or trimming historical text merely to satisfy a context.
#[test]
fn context_requires_exact_execution_policy_and_never_rewrites_history() {
    for (context, policy, other) in [
        (
            "all_turns",
            ThinkingContext::AllTurns,
            ThinkingContext::CurrentTurn,
        ),
        (
            "current_turn",
            ThinkingContext::CurrentTurn,
            ThinkingContext::AllTurns,
        ),
    ] {
        for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
            let source = request(
                dialect,
                json!({"context":context}),
                vec![
                    json!({"role":"user","content":"first"}),
                    json!({"role":"assistant","content":"reply"}),
                    json!({"role":"user","content":"next"}),
                ],
            );
            let options = RequestOptions {
                thinking_context: Some(policy),
                ..Default::default()
            };
            let compiled = compile(&source, &options).unwrap();
            assert_eq!(
                compiled.wire()["contents"],
                json!([{"role":"user","parts":[{"text":"first"}]},{"role":"model","parts":[{"text":"reply"}]},{"role":"user","parts":[{"text":"next"}]}])
            );
            assert_eq!(
                compiled.wire()["generationConfig"],
                json!({"maxOutputTokens":128})
            );
            for policy in [None, Some(other)] {
                assert_eq!(
                    compile(
                        &source,
                        &RequestOptions {
                            thinking_context: policy,
                            ..Default::default()
                        }
                    )
                    .unwrap_err()
                    .code,
                    "unsupported_google_reasoning_context"
                );
            }
            assert_eq!(compiled.source(), source.wire());
        }
    }
}

// Catches applying thinking settings after prefix verification or regenerating
// signed Parts from the displayed reasoning summary. Config changes stay bound.
#[test]
fn signed_history_binds_thinking_before_replay_and_preserves_native_summaries_and_signatures() {
    let mappings = [ReasoningMapping::new("high".into(), json!({"thinkingLevel":"HIGH"})).unwrap()];
    let summaries = summaries();
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let options = RequestOptions {
            reasoning_mappings: &mappings,
            summary_mappings: &summaries,
            thinking_context: Some(ThinkingContext::AllTurns),
            ..Default::default()
        };
        let reasoning = json!({"effort":"high","summary":"auto","context":"all_turns"});
        let mut input = vec![json!({"role":"user","content":"question"})];
        let first = compile(
            &request(dialect, reasoning.clone(), input.clone()),
            &options,
        )
        .unwrap();
        let signed = json!({"role":"model","parts":[{"thought":true,"text":"summary","thoughtSignature":"thought-signature","future":{"keep":true}},
            {"text":"answer","thoughtSignature":"text-signature"}]});
        let response =
            NativeResponse::parse(json!({"candidates":[{"finishReason":"STOP","content":signed}]}))
                .unwrap();
        let projected = NativeHistory::from_response(
            &response,
            MODEL,
            first.wire(),
            Some(0),
            "thinking-first",
            LIMIT,
        )
        .unwrap()
        .to_responses(LIMIT)
        .unwrap();
        assert_eq!(
            projected.output()[0]["summary"],
            json!([{"type":"summary_text","text":"summary"}])
        );
        input.extend(projected.output().to_vec());
        input.push(json!({"role":"user","content":"continue"}));
        let saved = serde_json::to_vec(&request(dialect, reasoning, input)).unwrap();
        let source =
            CanonicalRequest::new(serde_json::from_slice(&saved).unwrap(), dialect).unwrap();
        let second = compile(&source, &options).unwrap();
        assert_eq!(second.wire()["contents"][1], signed);
        assert_eq!(
            second.wire()["generationConfig"],
            json!({"maxOutputTokens":128,"thinkingConfig":{"thinkingLevel":"HIGH","includeThoughts":true}})
        );
        let changed =
            [ReasoningMapping::new("high".into(), json!({"thinkingLevel":"LOW"})).unwrap()];
        assert_eq!(
            compile(
                &source,
                &RequestOptions {
                    reasoning_mappings: &changed,
                    ..options
                }
            )
            .unwrap_err()
            .code,
            "google_history_request_mismatch"
        );
        let mut changed = source.wire().clone();
        changed["reasoning"]["summary"] = "none".into();
        assert_eq!(
            compile(&CanonicalRequest::new(changed, dialect).unwrap(), &options)
                .unwrap_err()
                .code,
            "google_history_request_mismatch"
        );
    }
}

// Catches implicit defaults, ignoring unknown constraints, malformed native
// settings, and duplicate execution profiles with order-dependent selection.
#[test]
fn malformed_reasoning_and_conflicting_profiles_are_rejected_without_defaults() {
    for (effort, native) in [
        ("", json!({"thinkingBudget":1})),
        ("  ", json!({"thinkingBudget":1})),
        ("bad\n", json!({"thinkingBudget":1})),
        ("high", json!({})),
        ("high", json!(null)),
        ("high", json!({"thinkingBudget":-2})),
        ("high", json!({"thinkingBudget":2147483648u64})),
        ("high", json!({"thinkingBudget":1.5})),
        ("high", json!({"thinkingBudget":"1024"})),
        ("high", json!({"thinkingLevel":"UNKNOWN"})),
        ("none", json!({"thinkingLevel":"MINIMAL"})),
        ("none", json!({"thinkingBudget":-1})),
        (
            "high",
            json!({"thinkingBudget":1024,"thinkingLevel":"HIGH"}),
        ),
        ("high", json!({"includeThoughts":true})),
        (
            "high",
            json!({"thinkingLevel":"HIGH","includeThoughts":"true"}),
        ),
        ("high", json!({"thinkingLevel":"HIGH","future":1})),
    ] {
        assert_eq!(
            ReasoningMapping::new(effort.into(), native)
                .unwrap_err()
                .code,
            "invalid_google_reasoning"
        );
    }
    for (source, include) in [("unknown", true), ("auto", false), ("none", true)] {
        assert_eq!(
            SummaryMapping::new(source.into(), include)
                .unwrap_err()
                .code,
            "invalid_google_reasoning"
        );
    }
    for reasoning in [
        json!(null),
        json!({}),
        json!({"effort":null,"summary":null,"context":null}),
    ] {
        let source = question(ResponsesDialect::Classic, reasoning);
        assert_eq!(
            compile(&source, &Default::default()).unwrap().wire()["generationConfig"],
            json!({"maxOutputTokens":128})
        );
    }
    for (reasoning, code) in [
        (json!("high"), "invalid_google_reasoning"),
        (json!({"effort":1}), "invalid_google_reasoning"),
        (json!({"summary":true}), "invalid_google_reasoning"),
        (json!({"context":1}), "invalid_google_reasoning"),
        (json!({"future":null}), "unsupported_google_reasoning"),
        (json!({"effort":""}), "unsupported_google_reasoning"),
        (
            json!({"summary":"unknown"}),
            "unsupported_google_reasoning_summary",
        ),
        (
            json!({"context":"unknown"}),
            "unsupported_google_reasoning_context",
        ),
    ] {
        assert_eq!(
            compile(
                &question(ResponsesDialect::Classic, reasoning),
                &Default::default()
            )
            .unwrap_err()
            .code,
            code
        );
    }
    let mappings = [
        ReasoningMapping::new("high".into(), json!({"thinkingBudget":1024})).unwrap(),
        ReasoningMapping::new("high".into(), json!({"thinkingLevel":"HIGH"})).unwrap(),
    ];
    let source = question(ResponsesDialect::Classic, json!(null));
    assert_eq!(
        compile(
            &source,
            &RequestOptions {
                reasoning_mappings: &mappings,
                ..Default::default()
            }
        )
        .unwrap_err()
        .code,
        "invalid_google_reasoning"
    );
    let duplicate = [
        SummaryMapping::new("auto".into(), true).unwrap(),
        SummaryMapping::new("auto".into(), true).unwrap(),
    ];
    assert_eq!(
        compile(
            &source,
            &RequestOptions {
                summary_mappings: &duplicate,
                ..Default::default()
            }
        )
        .unwrap_err()
        .code,
        "invalid_google_reasoning"
    );
}
