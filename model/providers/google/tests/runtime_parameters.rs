use caidex_model_core::{CanonicalRequest, ResponsesDialect};
use caidex_provider_google::{
    GenerateContentRequest, NativeHistory, NativeResponse, RequestOptions, ServiceTierMapping,
    VerbosityMapping,
};
use serde_json::{Value, json};

const MODEL: &str = "models/fixture-parameters";
const LIMIT: usize = 128 * 1024;
fn source(extra: Value, dialect: ResponsesDialect) -> CanonicalRequest {
    let mut source = json!({"model":"alias","input":[{"role":"user","content":"question"}]});
    source
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    if dialect == ResponsesDialect::Lite
        && let Some(instructions) = source.as_object_mut().unwrap().remove("instructions")
    {
        // Fixed Runtime encodes Lite base instructions as a developer item.
        source["input"]
            .as_array_mut()
            .unwrap()
            .insert(0, json!({"role":"developer","content":instructions}));
    }
    CanonicalRequest::new(source, dialect).unwrap()
}
fn compile(
    extra: Value,
    options: &RequestOptions<'_>,
    dialect: ResponsesDialect,
) -> caidex_model_core::ProviderResult<GenerateContentRequest> {
    GenerateContentRequest::from_responses_with_options(
        &source(extra, dialect),
        MODEL,
        128,
        LIMIT,
        8,
        options,
    )
}

// Catches losing attribution, enabling native caching, or leaking local hints.
#[test]
fn routing_hints_are_retained_only_locally_in_both_dialects() {
    let options = RequestOptions {
        retain_runtime_metadata: true,
        ..Default::default()
    };
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let extra = json!({"include":["reasoning.encrypted_content"],"client_metadata":{"x-codex-turn-metadata":"{\"turn_id\":\"synthetic\"}","future":"原文🙂"},"prompt_cache_key":"routing hint"});
        let compiled = compile(extra.clone(), &options, dialect).unwrap();
        assert_eq!(compiled.source(), source(extra, dialect).wire());
        assert_eq!(
            compiled.wire(),
            &json!({"generationConfig":{"maxOutputTokens":128},"contents":[{"role":"user","parts":[{"text":"question"}]}]})
        );
        assert!(!format!("{compiled:?}").contains("routing hint"));
        for extra in [
            json!({"client_metadata":{}}),
            json!({"prompt_cache_key":"hint"}),
        ] {
            assert_eq!(
                compile(extra, &RequestOptions::default(), dialect)
                    .unwrap_err()
                    .code,
                "unsupported_google_runtime_parameter"
            );
        }
    }
}

// Catches dropping unsupported delivery/access controls or accepting bad hints.
#[test]
fn runtime_parameter_shapes_and_unsupported_semantics_are_distinct() {
    let options = RequestOptions {
        retain_runtime_metadata: true,
        ..Default::default()
    };
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        for extra in [
            json!({"include":["message.output_text.logprobs"]}),
            json!({"stream_options":{"reasoning_summary_delivery":"sequential_cutoff"}}),
            json!({"access_programs":{}}),
            json!({"service_tier":"default"}),
        ] {
            assert_eq!(
                compile(extra.clone(), &options, dialect).unwrap_err().code,
                "unsupported_google_runtime_parameter",
                "{extra}"
            );
        }
        for extra in [
            json!({"include":1}),
            json!({"include":[1]}),
            json!({"include":["reasoning.encrypted_content","reasoning.encrypted_content"]}),
            json!({"prompt_cache_key":false}),
            json!({"prompt_cache_key":""}),
            json!({"client_metadata":[]}),
            json!({"client_metadata":{"key":1}}),
            json!({"stream_options":[]}),
            json!({"service_tier":1}),
        ] {
            assert_eq!(
                compile(extra.clone(), &options, dialect).unwrap_err().code,
                "invalid_google_runtime_parameter",
                "{extra}"
            );
        }
        assert_eq!(
            compile(json!({"prompt_cache_retention":"24h"}), &options, dialect)
                .unwrap_err()
                .code,
            "unsupported_google_request"
        );
    }
}

// Catches inventing defaults or requiring optional controls despite null/empty.
#[test]
fn optional_null_parameters_and_empty_delivery_controls_do_not_change_native_body() {
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let expected = json!({"generationConfig":{"maxOutputTokens":128},"contents":[{"role":"user","parts":[{"text":"question"}]}]});
        for extra in [
            json!({"include":null,"stream_options":null,"service_tier":null,"client_metadata":null,"prompt_cache_key":null,"access_programs":null,"text":null}),
            json!({"include":[],"stream_options":{},"text":{"verbosity":null,"format":null}}),
        ] {
            assert_eq!(
                compile(extra, &RequestOptions::default(), dialect)
                    .unwrap()
                    .wire(),
                &expected
            );
        }
    }
}

// Catches hint-only treatment of real native tiers or losing style/system roles.
#[test]
fn explicit_tier_and_verbosity_preserve_format_and_initial_system_order() {
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        for (level, instruction, tier, native) in [
            ("low", "Be brief.", "default", "standard"),
            ("medium", "Explain the main points.", "auto", "standard"),
            (
                "high",
                "Include relevant detail. 原文🙂",
                "priority",
                "priority",
            ),
            ("medium", "Explain the main points.", "flex", "flex"),
        ] {
            let verbosity = [VerbosityMapping::new(level.into(), instruction.into()).unwrap()];
            let tiers = [ServiceTierMapping::new(tier.into(), native.into()).unwrap()];
            let options = RequestOptions {
                verbosity_mappings: &verbosity,
                service_tier_mappings: &tiers,
                supports_structured_outputs: true,
                ..Default::default()
            };
            let extra = json!({"instructions":"fixed instructions","input":[
                {"role":"system","content":"initial system"},
                {"role":"developer","content":[{"type":"input_text","text":"initial developer"}]},
                {"role":"user","content":"question"}],
                "text":{"verbosity":level,"format":{"type":"json_schema","name":"answer","strict":true,"schema":{"type":"string"}}},
                "service_tier":tier});
            let compiled = compile(extra.clone(), &options, dialect).unwrap();
            assert_eq!(compiled.source(), source(extra, dialect).wire());
            assert_eq!(
                compiled.wire(),
                &json!({
                "serviceTier":native,
                "generationConfig":{"maxOutputTokens":128,"responseFormat":{"text":{"mimeType":"APPLICATION_JSON","schema":{"type":"string"}}}},
                "systemInstruction":{"parts":[{"text":"fixed instructions"},{"text":"initial system"},{"text":"initial developer"},{"text":instruction}]},
                "contents":[{"role":"user","parts":[{"text":"question"}]}]})
            );
            assert!(!format!("{compiled:?}").contains(instruction));
        }
        // Style-only requests need no structured-output capability.
        let verbosity = [VerbosityMapping::new("low".into(), "Be brief.".into()).unwrap()];
        let options = RequestOptions {
            verbosity_mappings: &verbosity,
            ..Default::default()
        };
        let compiled = compile(json!({"text":{"verbosity":"low"}}), &options, dialect).unwrap();
        assert_eq!(
            compiled.wire()["systemInstruction"],
            json!({"parts":[{"text":"Be brief."}]})
        );
        assert_eq!(
            compiled.wire()["generationConfig"],
            json!({"maxOutputTokens":128})
        );
    }
}

// Catches widening native tier classes, ambiguous configs, or implicit styles.
#[test]
fn malformed_and_duplicate_tier_or_verbosity_configuration_is_rejected() {
    for (source, native) in [
        ("default", "priority"),
        ("auto", "flex"),
        ("priority", "standard"),
        ("flex", "priority"),
        ("scale", "standard"),
        ("default", "STANDARD"),
        ("default", "unspecified"),
    ] {
        assert!(
            ServiceTierMapping::new(source.into(), native.into()).is_err(),
            "{source}/{native}"
        );
    }
    for (level, instruction) in [("future", "style"), ("low", ""), ("high", " \t\n")] {
        assert!(VerbosityMapping::new(level.into(), instruction.into()).is_err());
    }
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        for extra in [
            json!({"text":{"verbosity":"low"}}),
            json!({"service_tier":"priority"}),
            json!({"service_tier":"flex"}),
        ] {
            assert!(compile(extra, &RequestOptions::default(), dialect).is_err());
        }
        for extra in [
            json!({"text":{"verbosity":1}}),
            json!({"text":{"verbosity":"future"}}),
        ] {
            assert_eq!(
                compile(extra, &RequestOptions::default(), dialect)
                    .unwrap_err()
                    .code,
                "invalid_google_output_format"
            );
        }
        let tiers = [
            ServiceTierMapping::new("auto".into(), "standard".into()).unwrap(),
            ServiceTierMapping::new("auto".into(), "standard".into()).unwrap(),
        ];
        assert_eq!(
            compile(
                json!({}),
                &RequestOptions {
                    service_tier_mappings: &tiers,
                    ..Default::default()
                },
                dialect
            )
            .unwrap_err()
            .code,
            "invalid_google_runtime_parameter"
        );
        let verbosity = [
            VerbosityMapping::new("low".into(), "one".into()).unwrap(),
            VerbosityMapping::new("low".into(), "two".into()).unwrap(),
        ];
        assert_eq!(
            compile(
                json!({}),
                &RequestOptions {
                    verbosity_mappings: &verbosity,
                    ..Default::default()
                },
                dialect
            )
            .unwrap_err()
            .code,
            "invalid_google_output_format"
        );
    }
}

// Catches applying style/tier after checking history, editing signed Parts, or
// binding local hints as though they were native generation parameters.
#[test]
fn native_tier_and_style_bind_persisted_history_but_local_hints_can_change() {
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        for v2 in [false, true] {
            let verbosity = [VerbosityMapping::new("low".into(), "Be brief.".into()).unwrap()];
            let tiers = [
                ServiceTierMapping::new("default".into(), "standard".into()).unwrap(),
                ServiceTierMapping::new("priority".into(), "priority".into()).unwrap(),
            ];
            let options = RequestOptions {
                retain_runtime_metadata: true,
                verbosity_mappings: &verbosity,
                service_tier_mappings: &tiers,
                ..Default::default()
            };
            let input = vec![
                json!({"role":"developer","content":"fixed"}),
                json!({"role":"user","content":"question"}),
            ];
            let extra = json!({"input":input,"text":{"verbosity":"low"},"service_tier":"default","client_metadata":{"turn":"first"},"prompt_cache_key":"first hint"});
            let first = compile(extra.clone(), &options, dialect).unwrap();
            let signed = json!({"role":"model","parts":[{"text":"answer","thoughtSignature":"signed-style","future":{"keep":true}}]});
            let reply = NativeResponse::parse(
                json!({"candidates":[{"finishReason":"STOP","content":signed}]}),
            )
            .unwrap();
            let history = NativeHistory::from_response(
                &reply,
                MODEL,
                first.wire(),
                Some(0),
                "parameters-first",
                LIMIT,
            )
            .unwrap();
            let history = if v2 {
                history.with_tools(first.tools(), LIMIT).unwrap()
            } else {
                history
            };
            let mut extra = extra;
            extra["input"]
                .as_array_mut()
                .unwrap()
                .extend(history.to_responses(LIMIT).unwrap().output().to_vec());
            extra["input"]
                .as_array_mut()
                .unwrap()
                .push(json!({"role":"user","content":"next"}));
            extra["client_metadata"] = json!({"turn":"second","future":"原文"});
            extra["prompt_cache_key"] = "second hint".into();
            let persisted = serde_json::to_vec(&source(extra, dialect)).unwrap();
            let restored =
                CanonicalRequest::new(serde_json::from_slice(&persisted).unwrap(), dialect)
                    .unwrap();
            let compile_restored =
                |source: &CanonicalRequest, options: &RequestOptions<'_>, bytes| {
                    GenerateContentRequest::from_responses_with_options(
                        source, MODEL, 128, bytes, 8, options,
                    )
                };
            let second = compile_restored(&restored, &options, LIMIT).unwrap();
            assert_eq!(second.source(), restored.wire());
            assert_eq!(second.wire()["contents"][1], signed);
            assert_eq!(
                second.wire()["systemInstruction"],
                json!({"parts":[{"text":"fixed"},{"text":"Be brief."}]})
            );
            assert_eq!(second.wire()["serviceTier"], "standard");
            for key in ["text", "service_tier"] {
                let mut changed = restored.wire().clone();
                changed.as_object_mut().unwrap().remove(key);
                assert_eq!(
                    compile_restored(
                        &CanonicalRequest::new(changed, dialect).unwrap(),
                        &options,
                        LIMIT
                    )
                    .unwrap_err()
                    .code,
                    "google_history_request_mismatch"
                );
            }
            let mut changed = restored.wire().clone();
            changed["service_tier"] = "priority".into();
            assert_eq!(
                compile_restored(
                    &CanonicalRequest::new(changed, dialect).unwrap(),
                    &options,
                    LIMIT
                )
                .unwrap_err()
                .code,
                "google_history_request_mismatch"
            );
            let changed = [VerbosityMapping::new("low".into(), "Changed style.".into()).unwrap()];
            assert_eq!(
                compile_restored(
                    &restored,
                    &RequestOptions {
                        verbosity_mappings: &changed,
                        ..options
                    },
                    LIMIT
                )
                .unwrap_err()
                .code,
                "google_history_request_mismatch"
            );
            let huge = [VerbosityMapping::new("low".into(), "x".repeat(LIMIT)).unwrap()];
            assert!(
                compile_restored(
                    &source(json!({"text":{"verbosity":"low"}}), dialect),
                    &RequestOptions {
                        verbosity_mappings: &huge,
                        ..options
                    },
                    LIMIT
                )
                .is_err()
            );
        }
    }
}
