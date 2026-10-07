use caidex_model_core::{CanonicalRequest, ResponsesDialect};
use caidex_provider_anthropic::{MessagesRequest, ReasoningMapping, RequestOptions};
use serde_json::{Value, json};
fn compile(
    text: Value,
    supported: bool,
    mappings: &[ReasoningMapping],
    dialect: ResponsesDialect,
) -> caidex_model_core::ProviderResult<MessagesRequest> {
    let mut wire =
        json!({"model":"alias","input":[{"role":"user","content":"question"}],"text":text});
    if !mappings.is_empty() {
        wire["reasoning"] = json!({"effort":"high"});
    }
    MessagesRequest::from_responses_with_options(
        &CanonicalRequest::new(wire, dialect).unwrap(),
        "native",
        4096,
        128 * 1024,
        10,
        &RequestOptions {
            supports_structured_outputs: supported,
            reasoning_mappings: mappings,
            ..Default::default()
        },
    )
}
fn format() -> Value {
    json!({"format":{"type":"json_schema","name":"result","strict":true,"schema":{"type":"object","properties":{"n":{"type":"integer","minimum":1,"maximum":100},"s":{"type":"string","minLength":2,"pattern":"^a"}},"required":["n","s"],"additionalProperties":false,"future_keyword":{"nested":[true,null]}}}})
}
#[test]
fn exact_schema_and_source_survive_both_dialects_with_effort_and_thinking() {
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        for thinking in [None, Some(json!({"type":"adaptive"}))] {
            let mapping =
                ReasoningMapping::new("high".into(), Some("medium".into()), thinking.clone())
                    .unwrap();
            let text = format();
            let result = compile(text.clone(), true, &[mapping], dialect).unwrap();
            assert_eq!(result.source()["text"], text);
            assert_eq!(
                result.wire()["output_config"]["format"],
                json!({"type":"json_schema","schema":text["format"]["schema"]})
            );
            assert_eq!(result.wire()["output_config"]["effort"], "medium");
            assert_eq!(result.wire().get("thinking"), thinking.as_ref());
        }
        let result = compile(format(), true, &[], dialect).unwrap();
        assert!(result.wire()["output_config"].get("effort").is_none());
    }
}
#[test]
fn unsupported_semantics_and_missing_capability_are_explicit_errors() {
    assert!(compile(format(), false, &[], ResponsesDialect::Classic).is_err());
    for text in [
        json!({"format":{"type":"json_object"}}),
        json!({"verbosity":"high"}),
        json!({"future":null}),
        json!({"format":{"type":"text","future":true}}),
    ] {
        assert!(compile(text, true, &[], ResponsesDialect::Classic).is_err());
    }
    for (field, value) in [
        ("strict", json!(false)),
        ("strict", Value::Null),
        ("description", json!("must guide model")),
        ("future", Value::Null),
    ] {
        let mut text = format();
        text["format"][field] = value;
        assert!(compile(text, true, &[], ResponsesDialect::Classic).is_err());
    }
    let mut text = format();
    text["format"].as_object_mut().unwrap().remove("strict");
    assert!(compile(text, true, &[], ResponsesDialect::Classic).is_err());
}
#[test]
fn malformed_format_is_rejected_and_absent_format_needs_no_capability() {
    for text in [json!([]), json!({"format":[]})] {
        assert!(compile(text, true, &[], ResponsesDialect::Classic).is_err());
    }
    for (field, value) in [
        ("name", json!("bad name")),
        ("name", json!("a".repeat(65))),
        ("name", json!(1)),
        ("schema", json!([])),
        ("strict", json!("true")),
    ] {
        let mut text = format();
        text["format"][field] = value;
        assert!(compile(text, true, &[], ResponsesDialect::Classic).is_err());
    }
    for text in [
        Value::Null,
        json!({}),
        json!({"format":null,"verbosity":null}),
        json!({"format":{"type":"text"}}),
    ] {
        let result = compile(text, false, &[], ResponsesDialect::Classic).unwrap();
        assert!(result.wire().get("output_config").is_none());
    }
}

#[test]
fn schema_precision_and_final_native_byte_limit_are_preserved() {
    let mut text = format();
    text["format"]["schema"]["future_integer"] =
        serde_json::from_str("18446744073709551616000001").unwrap();
    let request = CanonicalRequest::new(
        json!({"model":"a","input":"q","text":text}),
        ResponsesDialect::Classic,
    )
    .unwrap();
    let options = RequestOptions {
        supports_structured_outputs: true,
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
    assert_eq!(
        compiled.wire()["output_config"]["format"]["schema"]["future_integer"].to_string(),
        "18446744073709551616000001"
    );
    let limit = request.wire().to_string().len();
    assert!(compiled.wire().to_string().len() > limit);
    assert!(
        MessagesRequest::from_responses_with_options(&request, "native", 4096, limit, 10, &options)
            .is_err()
    );
}

#[test]
fn verbosity_uses_explicit_style_mapping_and_preserves_native_controls() {
    use caidex_provider_anthropic::VerbosityMapping;
    let mappings: Vec<_> = ["low", "medium", "high"]
        .into_iter()
        .map(|level| {
            VerbosityMapping::new(level.into(), format!("Style instruction for {level}.")).unwrap()
        })
        .collect();
    let reasoning = [ReasoningMapping::new(
        "high".into(),
        Some("medium".into()),
        Some(json!({"type":"adaptive"})),
    )
    .unwrap()];
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        for level in ["low", "medium", "high"] {
            let mut text = format();
            text["verbosity"] = level.into();
            let mut source = json!({"model":"alias","instructions":"Existing system instruction.","input":[{"role":"user","content":"question"}],"reasoning":{"effort":"high"},"text":text});
            if dialect == ResponsesDialect::Lite {
                source.as_object_mut().unwrap().remove("instructions");
                source["input"].as_array_mut().unwrap().insert(
                    0,
                    json!({"role":"developer","content":"Existing system instruction."}),
                );
            }
            let request = CanonicalRequest::new(source.clone(), dialect).unwrap();
            let compiled = MessagesRequest::from_responses_with_options(
                &request,
                "native",
                4096,
                128 * 1024,
                10,
                &RequestOptions {
                    verbosity_mappings: &mappings,
                    supports_structured_outputs: true,
                    reasoning_mappings: &reasoning,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(compiled.source(), &source);
            assert_eq!(
                compiled.wire()["system"],
                json!([
                    {"type":"text","text":"Existing system instruction."},
                    {"type":"text","text":format!("Style instruction for {level}.")}
                ])
            );
            assert_eq!(compiled.wire()["output_config"]["effort"], "medium");
            assert_eq!(
                compiled.wire()["output_config"]["format"]["schema"],
                source["text"]["format"]["schema"]
            );
            assert!(compiled.wire().get("text").is_none());
            assert_eq!(compiled.wire()["thinking"], json!({"type":"adaptive"}));
        }
    }
}

#[test]
fn verbosity_policy_is_validated_even_without_a_requested_level() {
    use caidex_provider_anthropic::VerbosityMapping;
    for (level, instruction) in [("unknown", "style"), ("low", ""), ("medium", "  ")] {
        assert!(VerbosityMapping::new(level.into(), instruction.into()).is_err());
    }
    let mappings = [VerbosityMapping::new("low".into(), "Brief answers.".into()).unwrap()];
    let duplicate = [
        VerbosityMapping::new("low".into(), "A".into()).unwrap(),
        VerbosityMapping::new("low".into(), "B".into()).unwrap(),
    ];
    let compile = |text: Value, mappings: &[VerbosityMapping], limit| {
        let request = CanonicalRequest::new(
            json!({"model":"alias","input":"question","text":text}),
            ResponsesDialect::Classic,
        )
        .unwrap();
        MessagesRequest::from_responses_with_options(
            &request,
            "native",
            4096,
            limit,
            10,
            &RequestOptions {
                verbosity_mappings: mappings,
                ..Default::default()
            },
        )
    };
    assert!(compile(Value::Null, &duplicate, 128 * 1024).is_err());
    for level in [json!("high"), json!("unknown"), json!(1), json!({})] {
        assert!(compile(json!({"verbosity":level}), &mappings, 128 * 1024).is_err());
    }
    for text in [Value::Null, json!({}), json!({"verbosity":null})] {
        assert!(
            compile(text, &mappings, 128 * 1024)
                .unwrap()
                .wire()
                .get("system")
                .is_none()
        );
    }
    assert!(compile(json!({"verbosity":"low"}), &[], 128 * 1024).is_err());
    let huge = [VerbosityMapping::new("low".into(), "x".repeat(1024)).unwrap()];
    assert!(compile(json!({"verbosity":"low"}), &huge, 512).is_err());
}
