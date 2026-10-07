use caidex_model_core::{CanonicalRequest, ResponsesDialect};
use caidex_provider_google::{
    GenerateContentRequest, ImageDetailMapping, NativeHistory, NativeResponse, RequestOptions,
    ToolMap,
};
use serde_json::{Value, json};
const LIMIT: usize = 256 * 1024;
const MODEL: &str = "models/fixture-media";
const USER_MIMES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/heic",
    "image/heif",
];
const RESULT_MIMES: &[&str] = &["image/png", "image/jpeg", "image/webp"];
fn options() -> RequestOptions<'static> {
    RequestOptions {
        image_mime_types: USER_MIMES,
        tool_result_image_mime_types: RESULT_MIMES,
        ..Default::default()
    }
}
fn image(mime: &str) -> Value {
    json!({"type":"input_image","image_url":format!("data:{mime};base64,aW1hZ2U="),"detail":"auto"})
}
fn request(dialect: ResponsesDialect, mut input: Vec<Value>, tools: &[Value]) -> CanonicalRequest {
    let mut source =
        json!({"model":"alias","stream":true,"store":false,"parallel_tool_calls":true});
    if dialect == ResponsesDialect::Lite {
        input.insert(
            0,
            json!({"type":"additional_tools","role":"developer","tools":tools}),
        );
    } else {
        source["tools"] = json!(tools);
    }
    source["input"] = json!(input);
    CanonicalRequest::new(source, dialect).unwrap()
}
fn compile(source: &CanonicalRequest, options: &RequestOptions<'_>) -> GenerateContentRequest {
    GenerateContentRequest::from_responses_with_options(source, MODEL, 128, LIMIT, 8, options)
        .unwrap()
}
fn declarations() -> Vec<Value> {
    vec![
        json!({"type":"function","name":"inspect","parameters":{"type":"object"}}),
        json!({"type":"custom","name":"raw","format":{"type":"text"}}),
    ]
}

// Catches dropping images, changing bytes/order/MIME, or treating provider
// names as evidence; default profiles stay disabled.
#[test]
fn explicitly_enabled_user_images_keep_inline_bytes_and_text_order_in_both_dialects() {
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let mut content = vec![json!({"type":"input_text","text":"before"})];
        for mime in USER_MIMES {
            content.push(image(mime));
        }
        content.push(json!({"type":"input_text","text":"after"}));
        let source = request(dialect, vec![json!({"role":"user","content":content})], &[]);
        let compiled = compile(&source, &options());
        let parts = compiled.wire()["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts[0], json!({"text":"before"}));
        for (i, mime) in USER_MIMES.iter().enumerate() {
            assert_eq!(
                parts[i + 1],
                json!({"inlineData":{"mimeType":mime,"data":"aW1hZ2U="}})
            );
        }
        assert_eq!(parts[6], json!({"text":"after"}));
        assert_eq!(compiled.source(), source.wire());
        assert!(!format!("{compiled:?}").contains("aW1hZ2U"));
        assert_eq!(
            GenerateContentRequest::from_responses(&source, MODEL, 128, LIMIT, 8)
                .unwrap_err()
                .code,
            "unsupported_google_images"
        );
        let mut null_detail = image("image/png");
        null_detail["detail"] = Value::Null;
        let compiled = compile(
            &request(
                dialect,
                vec![json!({"role":"user","content":[null_detail]})],
                &[],
            ),
            &options(),
        );
        assert!(
            compiled.wire()["contents"][0]["parts"][0]
                .get("mediaResolution")
                .is_none()
        );
    }
}

// Catches image JSON being mistaken for native media, result order/association
// loss, duplicate reference names, or a locally generated ID in signed Parts.
#[test]
fn signed_function_and_custom_results_reference_unique_nested_native_media_in_original_order() {
    let declarations = declarations();
    let map = ToolMap::new(&declarations, 8).unwrap();
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let start = vec![
            json!({"role":"user","content":[image("image/png"),{"type":"input_text","text":"start"}]}),
        ];
        let first = compile(&request(dialect, start.clone(), &declarations), &options());
        let raw = json!({"candidates":[{"finishReason":"STOP","content":{"parts":[
            {"functionCall":{"name":map.native_tools()[0]["name"],"args":{}},"thoughtSignature":"signed-first"},
            {"functionCall":{"name":map.native_tools()[1]["name"],"id":"native-raw","args":{"input":"  original🙂 "}},"thoughtSignature":"signed-second"}
        ]}}]});
        let group = NativeHistory::from_response(
            &NativeResponse::parse(raw.clone()).unwrap(),
            MODEL,
            first.wire(),
            Some(0),
            "media-first",
            LIMIT,
        )
        .unwrap()
        .with_tools(first.tools(), LIMIT)
        .unwrap()
        .to_responses(LIMIT)
        .unwrap();
        let mut input = start;
        input.extend(group.output().to_vec());
        let mut annotated = image("image/png");
        annotated["future"] = json!({"keep":true});
        let function_output = json!([{"type":"input_text","text":"before"},annotated,
            {"type":"input_text","text":"between"},image("image/webp"),{"type":"input_text","text":"after"}]);
        input.push(json!({"type":"custom_tool_call_output","call_id":"native-raw","output":[image("image/jpeg")]}));
        input.push(json!({"type":"function_call_output","call_id":"call_media-first_0_0","output":function_output}));
        let source = request(dialect, input.clone(), &declarations);
        let second = compile(&source, &options());
        let parts = &second.wire()["contents"][2]["parts"];
        let function = &parts[0]["functionResponse"];
        let custom = &parts[1]["functionResponse"];
        assert!(function.get("id").is_none());
        assert_eq!(custom["id"], "native-raw");
        assert_eq!(
            function["parts"],
            json!([
                {"inlineData":{"mimeType":"image/png","data":"aW1hZ2U=","displayName":"caidex_image_1_1"}},
                {"inlineData":{"mimeType":"image/webp","data":"aW1hZ2U=","displayName":"caidex_image_1_3"}}
            ])
        );
        assert_eq!(
            function["response"]["output"],
            json!([
                {"type":"input_text","text":"before"},
                {"type":"input_image","image_url":{"$ref":"caidex_image_1_1"},"detail":"auto","future":{"keep":true}},
                {"type":"input_text","text":"between"},
                {"type":"input_image","image_url":{"$ref":"caidex_image_1_3"},"detail":"auto"},
                {"type":"input_text","text":"after"}
            ])
        );
        assert_eq!(
            custom["parts"],
            json!([{"inlineData":{"mimeType":"image/jpeg","data":"aW1hZ2U=","displayName":"caidex_image_2_0"}}])
        );
        assert_eq!(
            custom["response"]["output"],
            json!([{"type":"input_image","image_url":{"$ref":"caidex_image_2_0"},"detail":"auto"}])
        );
        let mut replay = raw["candidates"][0]["content"].clone();
        replay["role"] = "model".into();
        assert_eq!(second.wire()["contents"][1], replay);
        assert_eq!(second.source(), source.wire());
        // Native media and reference names must reconstruct identically at the
        // next bound prefix after serialization and profile reuse.
        let done=NativeResponse::parse(json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":"done","thoughtSignature":"third-prefix"}]}}]})).unwrap();
        input.extend(
            NativeHistory::from_response(
                &done,
                MODEL,
                second.wire(),
                Some(0),
                "media-second",
                LIMIT,
            )
            .unwrap()
            .with_tools(second.tools(), LIMIT)
            .unwrap()
            .to_responses(LIMIT)
            .unwrap()
            .output()
            .to_vec(),
        );
        input.push(json!({"role":"user","content":"next"}));
        let stored = serde_json::to_vec(&request(dialect, input.clone(), &declarations)).unwrap();
        let restored: Value = serde_json::from_slice(&stored).unwrap();
        let restored = CanonicalRequest::new(restored, dialect).unwrap();
        let third = compile(&restored, &options());
        assert_eq!(third.wire()["contents"][2], second.wire()["contents"][2]);
        input[0]["content"][0]["image_url"] = "data:image/png;base64,ZWRpdGVk".into();
        assert_eq!(
            GenerateContentRequest::from_responses_with_options(
                &request(dialect, input, &declarations),
                MODEL,
                128,
                LIMIT,
                8,
                &options()
            )
            .unwrap_err()
            .code,
            "google_history_request_mismatch"
        );
    }
}

// Catches silently guessing that native resolution enums have identical
// semantics, losing per-image choices, or putting Part-only settings on results.
#[test]
fn detail_intents_require_explicit_executor_mappings_and_remain_per_user_part() {
    let mappings = [
        ImageDetailMapping::new("low".into(), "MEDIA_RESOLUTION_LOW".into()).unwrap(),
        ImageDetailMapping::new("high".into(), "MEDIA_RESOLUTION_HIGH".into()).unwrap(),
        ImageDetailMapping::new("original".into(), "MEDIA_RESOLUTION_ULTRA_HIGH".into()).unwrap(),
    ];
    let opts = RequestOptions {
        image_detail_mappings: &mappings,
        ..options()
    };
    let mut content = Vec::new();
    for detail in ["low", "high", "original"] {
        let mut block = image("image/png");
        block["detail"] = detail.into();
        content.push(block);
    }
    let source = request(
        ResponsesDialect::Classic,
        vec![json!({"role":"user","content":content})],
        &[],
    );
    assert_eq!(
        GenerateContentRequest::from_responses_with_options(
            &source,
            MODEL,
            128,
            LIMIT,
            8,
            &options()
        )
        .unwrap_err()
        .code,
        "unsupported_google_image_detail"
    );
    let compiled = compile(&source, &opts);
    for (i, level) in [
        "MEDIA_RESOLUTION_LOW",
        "MEDIA_RESOLUTION_HIGH",
        "MEDIA_RESOLUTION_ULTRA_HIGH",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            compiled.wire()["contents"][0]["parts"][i]["mediaResolution"],
            json!({"level":level})
        );
    }
    assert!(
        compiled.wire()["generationConfig"]
            .get("mediaResolution")
            .is_none()
    );
    for (source, native) in [
        ("auto", "MEDIA_RESOLUTION_HIGH"),
        ("unknown", "MEDIA_RESOLUTION_HIGH"),
        ("high", "UNKNOWN"),
    ] {
        assert_eq!(
            ImageDetailMapping::new(source.into(), native.into())
                .unwrap_err()
                .code,
            "invalid_google_image_options"
        );
    }
    let duplicates = [
        ImageDetailMapping::new("high".into(), "MEDIA_RESOLUTION_HIGH".into()).unwrap(),
        ImageDetailMapping::new("high".into(), "MEDIA_RESOLUTION_LOW".into()).unwrap(),
    ];
    assert_eq!(
        GenerateContentRequest::from_responses_with_options(
            &source,
            MODEL,
            128,
            LIMIT,
            8,
            &RequestOptions {
                image_detail_mappings: &duplicates,
                ..options()
            }
        )
        .unwrap_err()
        .code,
        "invalid_google_image_options"
    );
    let mut block = image("image/png");
    block["detail"] = "high".into();
    let source = request(
        ResponsesDialect::Classic,
        vec![
            json!({"role":"user","content":"start"}),
            json!({"type":"function_call","name":"inspect","call_id":"one","arguments":"{}"}),
            json!({"type":"function_call_output","call_id":"one","output":[block]}),
        ],
        &declarations(),
    );
    assert_eq!(
        GenerateContentRequest::from_responses_with_options(&source, MODEL, 128, LIMIT, 8, &opts)
            .unwrap_err()
            .code,
        "unsupported_google_image_detail"
    );
}

#[test]
fn media_boundary_rejects_unsafe_sources_bad_bytes_roles_and_unsupported_capabilities() {
    for (url, code) in [
        ("data:image/png;base64,%%%", "invalid_google_image"),
        ("data:image/png;base64,", "invalid_google_image"),
        ("data:image/png;base64,a", "invalid_google_image"),
        ("data:image/png;aW1hZ2U=", "invalid_google_image"),
        (
            "data:image/png;other;base64,aW1hZ2U=",
            "unsupported_google_images",
        ),
        (
            "data:image/gif;base64,aW1hZ2U=",
            "unsupported_google_images",
        ),
        (
            "https://example.invalid/image.png",
            "unsupported_google_image_source",
        ),
        (
            "https://user:secret@example.invalid/image.png",
            "unsupported_google_image_source",
        ),
        ("file:///private.png", "unsupported_google_image_source"),
    ] {
        let source = request(
            ResponsesDialect::Classic,
            vec![json!({"role":"user","content":[{"type":"input_image","image_url":url}]})],
            &[],
        );
        let error = GenerateContentRequest::from_responses_with_options(
            &source,
            MODEL,
            128,
            LIMIT,
            8,
            &options(),
        )
        .unwrap_err();
        assert_eq!(error.code, code);
        assert!(!format!("{error:?}").contains("secret"));
    }
    for role in ["system", "developer", "assistant"] {
        let source = request(
            ResponsesDialect::Classic,
            vec![
                json!({"role":role,"content":[image("image/png")]}),
                json!({"role":"user","content":"start"}),
            ],
            &[],
        );
        assert_eq!(
            GenerateContentRequest::from_responses_with_options(
                &source,
                MODEL,
                128,
                LIMIT,
                8,
                &options()
            )
            .unwrap_err()
            .code,
            "unsupported_google_images"
        );
    }
    let mut foreign = image("image/png");
    foreign["file_id"] = "foreign".into();
    let source = request(
        ResponsesDialect::Classic,
        vec![json!({"role":"user","content":[foreign]})],
        &[],
    );
    assert_eq!(
        GenerateContentRequest::from_responses_with_options(
            &source,
            MODEL,
            128,
            LIMIT,
            8,
            &options()
        )
        .unwrap_err()
        .code,
        "unsupported_google_image_source"
    );
    for mimes in [&["image/gif"][..], &["image/png", "image/png"][..]] {
        let source = request(
            ResponsesDialect::Classic,
            vec![json!({"role":"user","content":"text"})],
            &[],
        );
        assert_eq!(
            GenerateContentRequest::from_responses_with_options(
                &source,
                MODEL,
                128,
                LIMIT,
                8,
                &RequestOptions {
                    image_mime_types: mimes,
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            "invalid_google_image_options"
        );
    }
    for mime in ["image/png", "image/heic"] {
        let source = request(
            ResponsesDialect::Classic,
            vec![
                json!({"role":"user","content":"start"}),
                json!({"type":"function_call","name":"inspect","call_id":"one","arguments":"{}"}),
                json!({"type":"function_call_output","call_id":"one","output":[image(mime)]}),
            ],
            &declarations(),
        );
        let opts = RequestOptions {
            image_mime_types: USER_MIMES,
            ..Default::default()
        };
        assert_eq!(
            GenerateContentRequest::from_responses_with_options(
                &source, MODEL, 128, LIMIT, 8, &opts
            )
            .unwrap_err()
            .code,
            "unsupported_google_images"
        );
    }
}

#[test]
fn encoded_source_and_expanded_native_media_are_bounded_before_transport() {
    let source = request(
        ResponsesDialect::Classic,
        vec![
            json!({"role":"user","content":[image("image/png")]}),
            json!({"type":"function_call","name":"inspect","call_id":"one","arguments":"{}"}),
            json!({"type":"function_call_output","call_id":"one","output":[image("image/png")]}),
        ],
        &declarations(),
    );
    let source_len = source.wire().to_string().len();
    assert!(
        GenerateContentRequest::from_responses_with_options(
            &source,
            MODEL,
            128,
            source_len - 1,
            8,
            &options()
        )
        .is_err()
    );
    let compiled = compile(&source, &options());
    assert!(compiled.wire().to_string().len() > source_len);
    assert!(
        GenerateContentRequest::from_responses_with_options(
            &source,
            MODEL,
            128,
            source_len,
            8,
            &options()
        )
        .is_err()
    );
}

#[test]
fn native_media_references_cannot_be_duplicated_or_redirected_by_output_metadata() {
    for reference in ["caidex_image_1_1", "foreign"] {
        let output = json!([{"type":"input_text","text":"metadata","future":{"$ref":reference}},image("image/png")]);
        let source = request(
            ResponsesDialect::Classic,
            vec![
                json!({"role":"user","content":"start"}),
                json!({"type":"function_call","name":"inspect","call_id":"one","arguments":"{}"}),
                json!({"type":"function_call_output","call_id":"one","output":output}),
            ],
            &declarations(),
        );
        assert_eq!(
            GenerateContentRequest::from_responses_with_options(
                &source,
                MODEL,
                128,
                LIMIT,
                8,
                &options()
            )
            .unwrap_err()
            .code,
            "invalid_google_image_reference"
        );
    }
    let source = request(
        ResponsesDialect::Classic,
        vec![
            json!({"role":"user","content":"start"}),
            json!({"type":"function_call","name":"inspect","call_id":"one","arguments":"{}"}),
            json!({"type":"function_call_output","call_id":"one","output":[{"type":"input_text","text":"literal {\"$ref\":\"foreign\"}"},image("image/png")]}),
        ],
        &declarations(),
    );
    assert_eq!(
        compile(&source, &options()).wire()["contents"][2]["parts"][0]["functionResponse"]["response"]
            ["output"][0]["text"],
        "literal {\"$ref\":\"foreign\"}"
    );
}
