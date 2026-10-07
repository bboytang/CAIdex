use caidex_model_core::{CanonicalRequest, ResponsesDialect};
use caidex_provider_google::{
    GenerateContentRequest, NativeHistory, NativeResponse, ReasoningMapping, RequestOptions,
};
use serde_json::{Value, json};
const MODEL: &str = "models/fixture-structured";
const LIMIT: usize = 256 * 1024;
fn request(dialect: ResponsesDialect, text: Value, mut input: Vec<Value>) -> CanonicalRequest {
    if dialect == ResponsesDialect::Lite {
        input.insert(
            0,
            json!({"type":"additional_tools","role":"developer","tools":[]}),
        );
    }
    CanonicalRequest::new(json!({"model":"alias","input":input,"text":text}), dialect).unwrap()
}
fn format(schema: Value) -> Value {
    json!({"format":{"type":"json_schema","name":"result","strict":true,"schema":schema}})
}
fn compile(
    source: &CanonicalRequest,
    options: &RequestOptions<'_>,
) -> caidex_model_core::ProviderResult<GenerateContentRequest> {
    GenerateContentRequest::from_responses_with_options(source, MODEL, 128, LIMIT, 8, options)
}
fn options() -> RequestOptions<'static> {
    RequestOptions {
        supports_structured_outputs: true,
        ..Default::default()
    }
}
fn schema() -> Value {
    json!({"type":"object","title":"原生结果","description":"Preserve this annotation.",
        "$defs":{"count":{"type":"integer","minimum":1,"maximum":100}},
        "properties":{
            "pattern":{"type":["string","null"],"format":"date-time"},
            "oneOf":{"$ref":"#/$defs/count"},
            "$ref":{"type":"string","enum":["原文","other"]},
            "values":{"type":"array","prefixItems":[{"type":"integer"}],"items":{"anyOf":[{"type":"number"},{"type":"null"}]},"minItems":1,"maxItems":3},
            "metadata":{"type":"object","additionalProperties":{"type":"string"}}},
        "required":["pattern","oneOf","$ref","values"],"additionalProperties":false})
}

// Catches using deprecated/typed Schema fields, weakening JSON-object mode,
// stripping annotations/references, or misreading property names as keywords.
#[test]
fn native_formats_preserve_schema_source_and_thinking_in_both_dialects() {
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        for (text, expected) in [
            (
                format(json!({"type":"boolean"})),
                json!({"text":{"mimeType":"APPLICATION_JSON","schema":{"type":"boolean"}}}),
            ),
            (
                format(json!({"$defs":{"a/b~c":{"type":"string"}},"$ref":"#/$defs/a~1b~0c"})),
                json!({"text":{"mimeType":"APPLICATION_JSON","schema":{"$defs":{"a/b~c":{"type":"string"}},"$ref":"#/$defs/a~1b~0c"}}}),
            ),
            (
                format(schema()),
                json!({"text":{"mimeType":"APPLICATION_JSON","schema":schema()}}),
            ),
            (
                json!({"format":{"type":"json_object"}}),
                json!({"text":{"mimeType":"APPLICATION_JSON","schema":{"type":"object"}}}),
            ),
        ] {
            let mappings =
                [ReasoningMapping::new("high".into(), json!({"thinkingLevel":"HIGH"})).unwrap()];
            let options = RequestOptions {
                reasoning_mappings: &mappings,
                ..options()
            };
            let mut source = request(
                dialect,
                text,
                vec![json!({"role":"user","content":"question"})],
            )
            .wire()
            .clone();
            source["reasoning"] = json!({"effort":"high"});
            let source = CanonicalRequest::new(source, dialect).unwrap();
            let compiled = compile(&source, &options).unwrap();
            assert_eq!(compiled.source(), source.wire());
            assert_eq!(
                compiled.wire()["generationConfig"],
                json!({"maxOutputTokens":128,"thinkingConfig":{"thinkingLevel":"HIGH"},"responseFormat":expected})
            );
            assert!(compiled.wire().get("text").is_none());
        }
    }
}

// Catches trusting arbitrary schema pass-through when Gemini ignores constraints
// or interprets oneOf as anyOf, including constraints hidden behind local refs.
#[test]
fn unsupported_schema_constraints_never_silently_reach_native_generation() {
    for bad in [
        json!({"type":"string","pattern":"^a"}),
        json!({"type":"string","minLength":2}),
        json!({"type":"string","maxLength":2}),
        json!({"type":"integer","multipleOf":2}),
        json!({"type":"integer","exclusiveMinimum":1}),
        json!({"type":"array","items":{"type":"string"},"uniqueItems":true}),
        json!({"oneOf":[{"type":"number"},{"type":"integer"}]}),
        json!({"allOf":[{"type":"object"}]}),
        json!({"not":{"type":"null"}}),
        json!({"type":"string","const":"fixed"}),
        json!({"type":"boolean","enum":[true,false]}),
        json!({"type":"string","format":"future-format"}),
        json!({"type":"object","future_keyword":null}),
        json!({"$schema":"https://json-schema.org/draft/2020-12/schema"}),
        json!({"$id":"https://example.invalid/root","type":"object"}),
        json!({"$anchor":"node","type":"object"}),
        json!({"$ref":"https://example.invalid/schema"}),
        json!({"$ref":"#node"}),
        json!({"$ref":"#/$defs/value","type":"string","$defs":{"value":{"type":"string"}}}),
        json!({"$ref":"#/$defs/a","$defs":{"a":{"$ref":"#/$defs/b"},"b":{"$ref":"#/$defs/a"}}}),
        json!({"type":"object","properties":{"optional":{"$ref":"#"}}}),
        json!({"type":"object","properties":{"nested":{"type":"array","items":{"type":"string","pattern":"^a"}}}}),
        json!({"type":"object","additionalProperties":{"type":"string","pattern":"^a"}}),
        json!({"$defs":{"unused":{"type":"string","pattern":"^a"}},"type":"object"}),
    ] {
        let source = request(
            ResponsesDialect::Classic,
            format(bad),
            vec![json!({"role":"user","content":"q"})],
        );
        assert_eq!(
            compile(&source, &options()).unwrap_err().code,
            "unsupported_google_output_schema"
        );
    }
}

// Catches implicit capabilities, discarding wrapper semantics, wrong shapes,
// and false schema constraints accidentally being treated as unspecified.
#[test]
fn malformed_formats_and_schemas_are_rejected_without_implicit_capabilities() {
    let source = request(
        ResponsesDialect::Classic,
        format(schema()),
        vec![json!({"role":"user","content":"q"})],
    );
    assert_eq!(
        compile(&source, &Default::default()).unwrap_err().code,
        "unsupported_google_output_format"
    );
    for text in [
        json!(1),
        json!({"format":[]}),
        json!({"format":{"type":"json_schema","name":"bad name","strict":true,"schema":{}}}),
        json!({"format":{"type":"json_schema","name":"a","strict":"true","schema":{}}}),
    ] {
        assert_eq!(
            compile(
                &request(
                    ResponsesDialect::Classic,
                    text,
                    vec![json!({"role":"user","content":"q"})]
                ),
                &options()
            )
            .unwrap_err()
            .code,
            "invalid_google_output_format"
        );
    }
    for text in [
        json!({"verbosity":"high"}),
        json!({"future":null}),
        json!({"format":{"type":"text","future":null}}),
        json!({"format":{"type":"json_object","schema":{}}}),
    ] {
        assert_eq!(
            compile(
                &request(
                    ResponsesDialect::Classic,
                    text,
                    vec![json!({"role":"user","content":"q"})]
                ),
                &options()
            )
            .unwrap_err()
            .code,
            "unsupported_google_output_format"
        );
    }
    for (field, value) in [
        ("strict", json!(false)),
        ("strict", Value::Null),
        ("description", json!("intent")),
        ("future", Value::Null),
    ] {
        let mut text = format(schema());
        text["format"][field] = value;
        assert_eq!(
            compile(
                &request(
                    ResponsesDialect::Classic,
                    text,
                    vec![json!({"role":"user","content":"q"})]
                ),
                &options()
            )
            .unwrap_err()
            .code,
            "unsupported_google_output_format"
        );
    }
    for bad in [
        json!([]),
        json!(false),
        json!({"type":"UNKNOWN"}),
        json!({"type":[]}),
        json!({"type":["string","string"]}),
        json!({"properties":[]}),
        json!({"items":false}),
        json!({"prefixItems":{}}),
        json!({"anyOf":[]}),
        json!({"anyOf":[1]}),
        json!({"required":[1]}),
        json!({"required":["a","a"]}),
        json!({"additionalProperties":null}),
        json!({"enum":[]}),
        json!({"enum":"a"}),
        json!({"minimum":"1"}),
        json!({"minItems":-1}),
        json!({"maxItems":1.5}),
        json!({"title":false}),
        json!({"description":1}),
        json!({"format":1}),
        json!({"$defs":[]}),
        json!({"$ref":1}),
        json!({"$ref":"#/$defs/missing"}),
    ] {
        assert_eq!(
            compile(
                &request(
                    ResponsesDialect::Classic,
                    format(bad),
                    vec![json!({"role":"user","content":"q"})]
                ),
                &options()
            )
            .unwrap_err()
            .code,
            "invalid_google_output_format"
        );
    }
    for text in [
        Value::Null,
        json!({}),
        json!({"format":null,"verbosity":null}),
        json!({"format":{"type":"text"}}),
    ] {
        let compiled = compile(
            &request(
                ResponsesDialect::Classic,
                text,
                vec![json!({"role":"user","content":"q"})],
            ),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(
            compiled.wire()["generationConfig"],
            json!({"maxOutputTokens":128})
        );
    }
}

// Catches configuring the schema after historical prefix comparison, or editing
// it without rejecting persisted signed v1/v2 carriers in either dialect.
#[test]
fn output_schema_is_bound_before_signed_native_history_replay() {
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        for with_tools in [false, true] {
            let mut input = vec![json!({"role":"user","content":"q"})];
            let first = compile(
                &request(dialect, format(schema()), input.clone()),
                &options(),
            )
            .unwrap();
            let signed = json!({"role":"model","parts":[{"text":r#"{"pattern":null,"oneOf":1,"$ref":"原文","values":[1],"metadata":{}}"#,"thoughtSignature":"structured-signature","future":{"keep":true}}]});
            let reply = NativeResponse::parse(
                json!({"candidates":[{"finishReason":"STOP","content":signed}]}),
            )
            .unwrap();
            let history = NativeHistory::from_response(
                &reply,
                MODEL,
                first.wire(),
                Some(0),
                "structured-first",
                LIMIT,
            )
            .unwrap();
            let history = if with_tools {
                history.with_tools(first.tools(), LIMIT).unwrap()
            } else {
                history
            };
            input.extend(history.to_responses(LIMIT).unwrap().output().to_vec());
            input.push(json!({"role":"user","content":"next"}));
            let source = request(dialect, format(schema()), input);
            let restored = CanonicalRequest::new(
                serde_json::from_slice(&serde_json::to_vec(&source).unwrap()).unwrap(),
                dialect,
            )
            .unwrap();
            let second = compile(&restored, &options()).unwrap();
            assert_eq!(second.wire()["contents"][1], signed);
            assert_eq!(
                second.wire()["generationConfig"]["responseFormat"],
                json!({"text":{"mimeType":"APPLICATION_JSON","schema":schema()}})
            );
            let mut changed = restored.wire().clone();
            changed["text"]["format"]["schema"]["description"] = "changed".into();
            assert_eq!(
                compile(
                    &CanonicalRequest::new(changed, dialect).unwrap(),
                    &options()
                )
                .unwrap_err()
                .code,
                "google_history_request_mismatch"
            );
            let mut changed = restored.wire().clone();
            changed.as_object_mut().unwrap().remove("text");
            assert_eq!(
                compile(
                    &CanonicalRequest::new(changed, dialect).unwrap(),
                    &options()
                )
                .unwrap_err()
                .code,
                "google_history_request_mismatch"
            );
        }
    }
}

// Catches using floating point to rewrite numeric constraints, dropping large
// schemas from byte accounting, and unbounded recursive schema traversal.
#[test]
fn schema_numbers_and_byte_limits_survive_without_recursive_expansion() {
    let numeric:Value=serde_json::from_str(r#"{"type":"number","enum":[18446744073709551616000001],"minimum":0.1234567890123456789012345}"#).unwrap();
    let source = request(
        ResponsesDialect::Classic,
        format(numeric.clone()),
        vec![json!({"role":"user","content":"q"})],
    );
    let compiled = compile(&source, &options()).unwrap();
    let native = &compiled.wire()["generationConfig"]["responseFormat"]["text"]["schema"];
    assert_eq!(native["enum"][0].to_string(), "18446744073709551616000001");
    assert_eq!(native["minimum"].to_string(), "0.1234567890123456789012345");
    let source = request(
        ResponsesDialect::Classic,
        format(json!({"type":"object"})),
        vec![json!({"role":"user","content":"q"})],
    );
    let compiled = compile(&source, &options()).unwrap();
    let limit = source.wire().to_string().len();
    assert!(compiled.wire().to_string().len() > limit);
    assert_eq!(
        GenerateContentRequest::from_responses_with_options(
            &source,
            MODEL,
            128,
            limit,
            8,
            &options()
        )
        .unwrap_err()
        .code,
        "invalid_google_request"
    );
    let mut deep = json!({"type":"string"});
    for _ in 0..70 {
        deep = json!({"type":"array","items":deep});
    }
    let source = request(
        ResponsesDialect::Classic,
        format(deep),
        vec![json!({"role":"user","content":"q"})],
    );
    assert_eq!(
        compile(&source, &options()).unwrap_err().code,
        "unsupported_google_output_schema"
    );
}

// Catches assuming that structured text and tool calling can be combined merely
// because each capability has been enabled individually for a native model.
#[test]
fn structured_output_with_tools_requires_its_own_execution_capability() {
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let mut source = request(
            dialect,
            format(schema()),
            vec![json!({"role":"user","content":"q"})],
        )
        .wire()
        .clone();
        let declarations =
            json!([{"type":"function","name":"echo","parameters":{"type":"object"}}]);
        if dialect == ResponsesDialect::Lite {
            source["input"][0]["tools"] = declarations;
        } else {
            source["tools"] = declarations;
        }
        let source = CanonicalRequest::new(source, dialect).unwrap();
        assert_eq!(
            compile(&source, &options()).unwrap_err().code,
            "unsupported_google_output_tools"
        );
        let options = RequestOptions {
            supports_structured_outputs_with_tools: true,
            ..options()
        };
        let compiled = compile(&source, &options).unwrap();
        assert_eq!(
            compiled.wire()["generationConfig"]["responseFormat"],
            json!({"text":{"mimeType":"APPLICATION_JSON","schema":schema()}})
        );
        assert_eq!(
            compiled.wire()["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"],
            json!({"type":"object"})
        );
    }
}

// Catches validating a different target from the URI fragment sent to Google,
// including encoded cycles, double decoding, and malformed literal-name aliases.
#[test]
fn local_reference_fragments_resolve_exactly_once_and_validate_escape_syntax() {
    let cyclic = json!({"$ref":"#/$defs/a","$defs":{
        "a":{"type":"object","properties":{"child":{"$ref":"#/$defs/%61"}}},
        "%61":{"type":"string"}}});
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let source = request(
            dialect,
            format(cyclic.clone()),
            vec![json!({"role":"user","content":"q"})],
        );
        assert_eq!(
            compile(&source, &options()).unwrap_err().code,
            "unsupported_google_output_schema"
        );
        for (reference, key) in [
            ("#/$defs/a%ZZ", "a%ZZ"),
            ("#/$defs/a%", "a%"),
            ("#/$defs/a%0", "a%0"),
            ("#/$defs/a%FF", "a%FF"),
            ("#/$defs/a~2", "a~2"),
            ("#/$defs/a~", "a~"),
            ("#/$defs/a%7E2", "a%7E2"),
        ] {
            let mut schema = json!({"$ref":reference,"$defs":{}});
            schema["$defs"][key] = json!({"type":"string"});
            let source = request(
                dialect,
                format(schema),
                vec![json!({"role":"user","content":"q"})],
            );
            assert_eq!(
                compile(&source, &options()).unwrap_err().code,
                "invalid_google_output_format"
            );
        }
        for (reference, key) in [
            ("#/$defs/a%20b", "a b"),
            ("#%2F$defs%2F%61", "a"),
            ("#/$defs/%2561", "%61"),
            ("#/$defs/a%7E1b%7E0c", "a/b~c"),
            ("#/$defs/%E5%8E%9F%E6%96%87", "原文"),
        ] {
            let mut schema = json!({"$ref":reference,"$defs":{}});
            schema["$defs"][key] = json!({"type":"string"});
            let source = request(
                dialect,
                format(schema.clone()),
                vec![json!({"role":"user","content":"q"})],
            );
            let compiled = compile(&source, &options()).unwrap();
            assert_eq!(compiled.source(), source.wire());
            assert_eq!(
                compiled.wire()["generationConfig"]["responseFormat"]["text"]["schema"],
                schema
            );
        }
    }
}
