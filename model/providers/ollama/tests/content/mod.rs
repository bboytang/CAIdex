use super::*;

fn image(mime: &str) -> Value {
    // Native decoding validates Base64; model-side image decoding is separate.
    json!({"type":"input_image","image_url":format!("data:{mime};base64,iVBORw0KGgo="),"detail":"auto"})
}
fn format() -> Value {
    json!({"format":{"type":"json_schema","name":"fixture_schema","strict":false,
        "schema":{"type":"object","properties":{"answer":{"type":"string"}},
        "required":["answer"],"additionalProperties":false,
        "x-future":{"precise":18446744073709551616_u128}}}})
}

#[tokio::test]
async fn inline_images_and_non_strict_schema_preserve_native_wire_in_json_and_sse() {
    for stream in [false, true] {
        let reply = if stream {
            Reply::stream(format!(
                "{CREATED}event: response.completed\ndata: {}\n\n",
                json!({"type":"response.completed","sequence_number":1,"response":response_wire()})
            ))
        } else {
            Reply::json(response_wire())
        };
        let mut fixture = Fixture::start(vec![reply]).await;
        let (broker, reads) = broker();
        let provider = fixture
            .provider(broker, true)
            .with_images()
            .with_structured_output();
        for mime in ["image/png", "image/jpeg", "image/jpg", "image/webp", ""] {
            let input = json!([{"role":"user","content":[{"type":"input_text","text":"before🙂"},image(mime),{"type":"input_text","text":"after"}]}]);
            let wire = json!({"model":"fixture","input":input,"text":format(),"stream":stream});
            if stream {
                let mut events = provider
                    .stream_response(request(wire), RequestContext::default())
                    .await
                    .unwrap()
                    .events;
                while let Some(event) = events.next().await {
                    event.unwrap();
                }
            } else {
                provider
                    .create_response(request(wire), RequestContext::default())
                    .await
                    .unwrap();
            }
            let captured = fixture.request().await.body.unwrap();
            assert_eq!(captured["model"], "native-fixture");
            assert_eq!(captured["input"], input);
            assert_eq!(captured["text"], format());
        }
        assert_eq!(reads.load(Ordering::SeqCst), 5);
    }
}

#[tokio::test]
async fn image_tool_results_and_schema_changes_replay_native_bound_history() {
    let mut fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker();
    let provider = fixture
        .provider(broker, true)
        .with_images()
        .with_structured_output()
        .with_native_history();
    let first =
        json!({"role":"user","content":[image("image/png"),{"type":"input_text","text":"look"}]});
    let response = provider
        .create_response(
            request(json!({"model":"fixture","input":[first],"text":format()})),
            RequestContext::default(),
        )
        .await
        .unwrap()
        .response;
    assert_eq!(fixture.request().await.body.unwrap()["text"], format());
    let result = json!({"type":"function_call_output","call_id":"c1","output":[{"type":"input_text","text":"result"},image("image/webp")]});
    let mut display = vec![first.clone()];
    display.extend(response.output().iter().cloned());
    display.push(result.clone());
    let changed = json!({"format":{"type":"json_object"}});
    provider
        .create_response(
            request(json!({"model":"fixture","input":display,"text":changed})),
            RequestContext::default(),
        )
        .await
        .unwrap();
    let captured = fixture.request().await.body.unwrap();
    let mut native = vec![first];
    native.extend(
        response_wire()["output"]
            .as_array()
            .unwrap()
            .iter()
            .cloned(),
    );
    native.push(result);
    assert_eq!(captured["input"], json!(native));
    assert_eq!(
        captured["text"],
        json!({"format":{"type":"json_schema","name":"caidex_json_object","schema":{"type":"object"}}})
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn media_sources_detail_invalid_bytes_and_unmapped_formats_fail_before_keys() {
    let fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker();
    let provider = fixture
        .provider(broker, true)
        .with_images()
        .with_structured_output();
    let mut cases = Vec::new();
    for url in [
        "https://fixture.invalid/picture.png",
        "file:///picture.png",
        "data:image/gif;base64,YQ==",
        "data:image/png;base64,",
        "data:image/png;base64,!!!!",
        "data:image/png;base64,YQ",
        "data:image/png;base64,YR==",
    ] {
        let mut part = image("image/png");
        part["image_url"] = url.into();
        cases.push(json!({"model":"fixture","input":[{"role":"user","content":[part]}]}));
    }
    for (key, value) in [
        ("detail", json!("high")),
        ("detail", json!("low")),
        ("file_id", json!("stored_file")),
        ("future", json!(true)),
        ("image_url", json!(null)),
    ] {
        let mut part = image("image/png");
        part[key] = value;
        cases.push(json!({"model":"fixture","input":[{"role":"user","content":[part]}]}));
    }
    for text in [
        json!({"verbosity":"high"}),
        json!({"format":{"type":"future_format"}}),
        json!({"format":{"type":"json_schema","name":"bad name","schema":{}}}),
        json!({"format":{"type":"json_schema","name":"fixture","strict":true,"schema":{}}}),
        json!({"format":{"type":"json_schema","name":"fixture","schema":[],"strict":false}}),
        json!({"format":{"type":"json_schema","name":"fixture","schema":{},"description":"model guidance"}}),
    ] {
        cases.push(json!({"model":"fixture","input":"hello","text":text}));
    }
    for wire in cases {
        for stream in [false, true] {
            let error = if stream {
                provider
                    .stream_response(request(wire.clone()), RequestContext::default())
                    .await
                    .err()
                    .unwrap()
            } else {
                provider
                    .create_response(request(wire.clone()), RequestContext::default())
                    .await
                    .err()
                    .unwrap()
            };
            assert_eq!(error.http_status, 400);
            assert!(!format!("{error:?}").contains(KEY));
        }
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn content_opt_in_and_explicit_unsupported_capabilities_are_independent() {
    for (enabled, vision, structured, use_format) in [
        (false, false, false, false),
        (false, false, false, true),
        (true, true, false, false),
        (true, false, true, true),
    ] {
        let fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
        let (broker, reads) = broker();
        let mut metadata = model("fixture", "native-fixture");
        if vision {
            metadata.capabilities.vision = caidex_model_core::CapabilitySupport::Unsupported;
        }
        if structured {
            metadata.capabilities.structured_output =
                caidex_model_core::CapabilitySupport::Unsupported;
        }
        let mut provider = OllamaProvider::new(
            OllamaConfig::new(&fixture.base, Some(reference())).unwrap(),
            vec![metadata],
            broker,
            limits(),
        )
        .unwrap();
        if enabled {
            provider = provider.with_images().with_structured_output();
        }
        let wire = if use_format {
            json!({"model":"fixture","input":"hello","text":format()})
        } else {
            json!({"model":"fixture","input":[{"role":"user","content":[image("image/png")]}]})
        };
        assert_eq!(
            provider
                .create_response(request(wire), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            400
        );
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn body_budget_bounds_inline_bytes_and_schema_before_keys_in_both_paths() {
    let fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker();
    let provider = OllamaProvider::new(
        OllamaConfig::new(&fixture.base, Some(reference())).unwrap(),
        vec![model("fixture", "native-fixture")],
        broker,
        Limits {
            request_bytes: 512,
            ..limits()
        },
    )
    .unwrap()
    .with_images()
    .with_structured_output();
    let mut part = image("image/png");
    part["image_url"] = format!("data:image/png;base64,{}", "YWFh".repeat(256)).into();
    let mut schema = format();
    schema["format"]["schema"]["description"] = "large".repeat(256).into();
    for wire in [
        json!({"model":"fixture","input":[{"role":"user","content":[part]}]}),
        json!({"model":"fixture","input":"hello","text":schema}),
    ] {
        for stream in [false, true] {
            let error = if stream {
                provider
                    .stream_response(request(wire.clone()), RequestContext::default())
                    .await
                    .err()
                    .unwrap()
            } else {
                provider
                    .create_response(request(wire.clone()), RequestContext::default())
                    .await
                    .err()
                    .unwrap()
            };
            assert_eq!(error.http_status, 413);
            assert_eq!(error.code, "invalid_or_oversized_body");
        }
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn plain_text_defaults_and_optional_null_formats_do_not_require_opt_in() {
    let mut fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker();
    let provider = fixture.provider(broker, true);
    for text in [
        json!(null),
        json!({}),
        json!({"format":null}),
        json!({"format":{"type":"text"}}),
    ] {
        provider
            .create_response(
                request(json!({"model":"fixture","input":"hello","text":text})),
                RequestContext::default(),
            )
            .await
            .unwrap();
        let captured = fixture.request().await.body.unwrap();
        if text["format"]["type"] == "text" {
            assert_eq!(captured["text"], text);
        } else {
            assert!(captured.get("text").is_none());
        }
    }
    assert_eq!(reads.load(Ordering::SeqCst), 4);
}
