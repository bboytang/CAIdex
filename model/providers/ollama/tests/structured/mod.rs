use super::*;

fn strict(schema: Value) -> Value {
    json!({"model":"fixture","input":"hello","text":{"format":{"type":"json_schema","name":"fixture","strict":true,"schema":schema}}})
}
fn stream_request(schema: Value) -> CanonicalRequest {
    let mut wire = strict(schema);
    wire["stream"] = true.into();
    request(wire)
}
fn message(text: &str) -> Value {
    json!({"type":"message","id":"msg_1","status":"completed","role":"assistant","content":[{"type":"output_text","text":text,"annotations":[]}]})
}
fn answer(output: Value) -> Value {
    json!({"id":"fixture","model":"native-fixture","status":"completed","output":output})
}
fn frames(chunks: &[Value]) -> String {
    chunks
        .iter()
        .map(|c| format!("event: {}\ndata: {c}\n\n", c["type"].as_str().unwrap()))
        .collect()
}
fn terminal(wire: &Value, sequence: u64) -> Value {
    json!({"type":format!("response.{}",wire["status"].as_str().unwrap()),"sequence_number":sequence,"response":wire})
}

#[tokio::test]
async fn strict_json_validates_constraints_refs_and_exact_numbers_before_delivery() {
    let precise: Value = serde_json::from_str("1.00000000000000000001").unwrap();
    let schema = json!({"type":"object","$defs":{"count":{"type":"integer","minimum":18446744073709551616_u128}},
        "properties":{"n":{"$ref":"#/$defs/count"},"fraction":{"const":precise},"email":{"type":"string","format":"email"}},
        "required":["n","fraction","email"],"additionalProperties":false});
    for (text, valid) in [
        (
            r#"{"n":18446744073709551616,"fraction":1.00000000000000000001,"email":"a@example.com"}"#,
            true,
        ),
        (
            r#"{"n":18446744073709551615,"fraction":1.00000000000000000001,"email":"a@example.com"}"#,
            false,
        ),
        (
            r#"{"n":18446744073709551616,"fraction":1.0,"email":"a@example.com"}"#,
            false,
        ),
        (
            r#"{"n":18446744073709551616,"fraction":1.00000000000000000001,"email":"bad"}"#,
            false,
        ),
        (
            r#"{"n":18446744073709551616,"fraction":1.00000000000000000001,"email":"a@example.com","extra":true}"#,
            false,
        ),
        (
            r#"{"n":18446744073709551616,"fraction":1.00000000000000000001}"#,
            false,
        ),
        ("{} trailing", false),
    ] {
        for history in [false, true] {
            let native = answer(json!([message(text)]));
            let mut fixture = Fixture::start(vec![Reply::json(native.clone())]).await;
            let (broker, reads) = broker();
            let mut provider = fixture.provider(broker, true).with_structured_output();
            if history {
                provider = provider.with_native_history();
            }
            let wire = strict(schema.clone());
            let result = provider
                .create_response(request(wire.clone()), RequestContext::default())
                .await;
            if valid {
                let response = result.unwrap().response;
                assert_eq!(response.output().last().unwrap(), &message(text));
                if history {
                    let mut input = vec![json!({"role":"user","content":"hello"})];
                    input.extend(response.output().iter().cloned());
                    input.push(json!({"role":"user","content":"next"}));
                    let mut next = wire.clone();
                    next["input"] = json!(input);
                    provider
                        .create_response(request(next), RequestContext::default())
                        .await
                        .unwrap();
                    fixture.request().await;
                } else {
                    assert_eq!(response.wire(), &native);
                }
            } else {
                let error = result.err().unwrap();
                assert_eq!(
                    (error.http_status, error.code),
                    (502, "ollama_invalid_structured_output")
                );
                assert!(!format!("{error:?}").contains(text));
            }
            assert_eq!(fixture.request().await.body.unwrap()["text"], wire["text"]);
            assert_eq!(
                reads.load(Ordering::SeqCst),
                if history && valid { 2 } else { 1 }
            );
        }
    }
}

#[tokio::test]
async fn invalid_and_external_schemas_fail_before_credentials_or_http() {
    let fixture = Fixture::start(vec![Reply::json(answer(json!([message("{}")])))]).await;
    let (broker, reads) = broker();
    let provider = fixture.provider(broker, true).with_structured_output();
    for schema in [
        json!({"type":"invalid"}),
        json!({"required":"wrong"}),
        json!({"$ref":format!("{}/probe",fixture.base)}),
        json!({"$ref":"file:///tmp/CAIDEX_SECRET_SCHEMA_MUST_NOT_READ"}),
        json!({"$ref":"#/$defs/missing"}),
        json!({"$schema":"https://fixture.invalid/custom-draft"}),
        json!({"type":"string","format":"unknown-fixture-format"}),
        json!({"type":"string","pattern":"["}),
    ] {
        for stream in [false, true] {
            let wire = strict(schema.clone());
            let error = if stream {
                provider
                    .stream_response(stream_request(schema.clone()), RequestContext::default())
                    .await
                    .err()
                    .unwrap()
            } else {
                provider
                    .create_response(request(wire), RequestContext::default())
                    .await
                    .err()
                    .unwrap()
            };
            assert_eq!(
                (error.http_status, error.code),
                (400, "ollama_invalid_output_schema")
            );
            assert!(!format!("{error:?}").contains("CAIDEX_SECRET"));
        }
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn strict_stream_keeps_provisional_text_and_holds_tools_until_valid_terminal() {
    for history in [false, true] {
        for valid in [false, true] {
            let text = if valid {
                r#"{"answer":"yes"}"#
            } else {
                r#"{"answer":false}"#
            };
            let msg = message(text);
            let call = response_wire()["output"][0].clone();
            let native = answer(json!([msg, call]));
            let chunks = [
                json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"type":"message","id":"msg_1","role":"assistant","status":"in_progress","content":[]}}),
                json!({"type":"response.output_text.delta","sequence_number":2,"output_index":0,"item_id":"msg_1","content_index":0,"delta":text}),
                json!({"type":"response.output_item.added","sequence_number":3,"output_index":1,"item":{"type":"function_call","id":"fc_1","name":"echo","call_id":"c1","arguments":""}}),
                json!({"type":"response.function_call_arguments.delta","sequence_number":4,"output_index":1,"item_id":"fc_1","delta":call["arguments"]}),
                json!({"type":"response.output_item.done","sequence_number":5,"output_index":1,"item":call}),
                json!({"type":"response.output_item.done","sequence_number":6,"output_index":0,"item":msg}),
                terminal(&native, 7),
            ];
            let mut fixture =
                Fixture::start(vec![Reply::stream(format!("{CREATED}{}", frames(&chunks)))]).await;
            let (broker, _) = broker();
            let mut provider = fixture.provider(broker, true).with_structured_output();
            if history {
                provider = provider.with_native_history();
            }
            let mut stream=provider.stream_response(stream_request(json!({"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"],"additionalProperties":false})),RequestContext::default()).await.unwrap().events;
            fixture.request().await;
            let (mut progress, mut done, mut error, mut completed, mut seq) =
                (false, 0, None, false, 0);
            while let Some(event) = stream.next().await {
                match event {
                    Err(e) => error = Some(e.code),
                    Ok(ProviderStreamEvent::Model(event)) => {
                        let wire = event.response.wire();
                        assert_eq!(wire["sequence_number"], seq);
                        seq += 1;
                        if event.response.kind() == "response.output_text.delta" {
                            progress = true;
                            assert_eq!(wire["output_index"], usize::from(history));
                        }
                        if event.response.kind() == "response.output_item.done" {
                            assert!(valid);
                            done += 1;
                        }
                        if event.response.kind() == "response.function_call_arguments.delta" {
                            assert!(valid && progress);
                        }
                        if event.response.terminal().is_some() {
                            completed = true;
                            assert!(valid && progress);
                            assert_eq!(
                                wire["response"]["output"].as_array().unwrap().len(),
                                2 + usize::from(history)
                            );
                            if !history {
                                assert_eq!(wire["response"], native);
                            }
                        }
                    }
                    _ => (),
                }
            }
            assert!(progress);
            assert_eq!(completed, valid);
            assert_eq!(done, if valid { 2 + usize::from(history) } else { 0 });
            assert_eq!(
                error,
                if valid {
                    None
                } else {
                    Some("ollama_invalid_structured_output")
                }
            );
        }
    }
}

#[tokio::test]
async fn strict_output_distinguishes_calls_refusals_and_partial_states_from_json_answers() {
    for case in [
        "call",
        "refusal",
        "failed",
        "incomplete",
        "empty",
        "thought",
        "malformed",
        "mixed",
        "unknown",
    ] {
        let refusal = json!({"type":"message","id":"msg_1","role":"assistant","status":"completed","content":[{"type":"refusal","refusal":"fixture refusal"}]});
        let mut native = answer(match case {
            "call" => response_wire()["output"].clone(),
            "refusal" => json!([refusal]),
            "empty" => json!([]),
            "thought" => {
                json!([{"type":"reasoning","id":"rs","summary":[],"encrypted_content":"thought"}])
            }
            "malformed" => json!([{"type":"message","id":"msg_1","role":"user","content":[]}]),
            "mixed" => {
                let mut item = refusal;
                item["content"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"type":"output_text","text":"{}"}));
                json!([item])
            }
            "unknown" => json!([{"type":"future_output"}]),
            _ => json!([message("{unfinished")]),
        });
        if matches!(case, "failed" | "incomplete") {
            native["status"] = case.into();
        }
        let valid = matches!(case, "call" | "refusal" | "failed" | "incomplete");
        for stream in [false, true] {
            let reply = if stream {
                Reply::stream(format!("{CREATED}{}", frames(&[terminal(&native, 1)])))
            } else {
                Reply::json(native.clone())
            };
            let mut fixture = Fixture::start(vec![reply]).await;
            let (broker, _) = broker();
            let provider = fixture.provider(broker, true).with_structured_output();
            if stream {
                let mut events = provider
                    .stream_response(
                        stream_request(json!({"type":"object"})),
                        RequestContext::default(),
                    )
                    .await
                    .unwrap()
                    .events;
                let (mut end, mut error, mut calls) = (None, None, 0);
                while let Some(event) = events.next().await {
                    match event {
                        Err(e) => error = Some(e.code),
                        Ok(ProviderStreamEvent::Model(event)) => {
                            if event.response.kind() == "response.output_item.done"
                                && event.response.wire()["item"]["type"] == "function_call"
                            {
                                calls += 1;
                            }
                            if event.response.terminal().is_some() {
                                end = Some(event.response.wire()["response"].clone());
                            }
                        }
                        _ => (),
                    }
                }
                assert_eq!(end, valid.then(|| native.clone()), "{case}");
                assert_eq!(calls, usize::from(case == "call"));
                assert_eq!(
                    error,
                    if valid {
                        None
                    } else {
                        Some("ollama_invalid_structured_output")
                    },
                    "{case}"
                );
            } else {
                let result = provider
                    .create_response(
                        request(strict(json!({"type":"object"}))),
                        RequestContext::default(),
                    )
                    .await;
                if valid {
                    assert_eq!(result.unwrap().response.wire(), &native);
                } else {
                    assert_eq!(
                        result.err().unwrap().code,
                        "ollama_invalid_structured_output",
                        "{case}"
                    );
                }
            }
            fixture.request().await;
        }
    }
}

#[tokio::test]
async fn strict_only_stream_rejects_mismatched_or_truncated_tools_and_preserves_native_reasoning() {
    for case in ["valid", "mismatch", "truncated"] {
        let reasoning = json!({"type":"reasoning","id":"rs_1","status":"completed","summary":[{"type":"summary_text","text":"thought"}],"encrypted_content":"original thought"});
        let call = response_wire()["output"][0].clone();
        let native = answer(json!([reasoning, call]));
        let mut done = call.clone();
        if case == "mismatch" {
            done["call_id"] = "different".into();
        }
        let mut chunks = vec![
            json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"type":"reasoning","id":"rs_1","summary":[]}}),
            json!({"type":"response.reasoning_summary_text.delta","sequence_number":2,"output_index":0,"item_id":"rs_1","summary_index":0,"delta":"thought"}),
            json!({"type":"response.output_item.added","sequence_number":3,"output_index":1,"item":{"type":"function_call","id":"fc_1","name":"echo","call_id":"c1","arguments":""}}),
            json!({"type":"response.output_item.done","sequence_number":4,"output_index":1,"item":done}),
        ];
        if case != "truncated" {
            chunks.push(terminal(&native, 5));
        }
        let mut fixture =
            Fixture::start(vec![Reply::stream(format!("{CREATED}{}", frames(&chunks)))]).await;
        let (broker, _) = broker();
        let provider = fixture.provider(broker, true).with_structured_output();
        let mut events = provider
            .stream_response(
                stream_request(json!({"type":"object"})),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .events;
        let (mut end, mut error, mut calls, mut progress) = (None, None, 0, false);
        while let Some(event) = events.next().await {
            match event {
                Err(e) => error = Some(e.code),
                Ok(ProviderStreamEvent::Model(event)) => {
                    if event.response.kind() == "response.reasoning_summary_text.delta" {
                        assert_eq!(event.response.wire()["item_id"], "rs_1");
                        assert_eq!(event.response.wire()["output_index"], 0);
                        progress = true;
                    }
                    if event.response.kind() == "response.output_item.done"
                        && event.response.wire()["item"]["type"] == "function_call"
                    {
                        calls += 1;
                    }
                    if event.response.terminal().is_some() {
                        end = Some(event.response.wire()["response"].clone());
                    }
                }
                _ => (),
            }
        }
        assert!(progress);
        assert_eq!(calls, usize::from(case == "valid"));
        assert_eq!(end, (case == "valid").then(|| native.clone()));
        assert_eq!(
            error,
            match case {
                "valid" => None,
                "mismatch" => Some("ollama_invalid_native_history"),
                _ => Some("provider_stream_truncated"),
            }
        );
        fixture.request().await;
    }
}

#[tokio::test]
async fn strict_pending_output_cancellation_deadline_drop_and_budget_release_transport() {
    for case in ["cancel", "deadline", "drop", "budget"] {
        let msg = message("{}");
        let native = answer(json!([msg]));
        let start = format!(
            "{CREATED}{}",
            frames(&[
                json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"type":"message","id":"msg_1","role":"assistant","status":"in_progress","content":[]}}),
                json!({"type":"response.output_text.delta","sequence_number":2,"output_index":0,"item_id":"msg_1","content_index":0,"delta":"{}"}),
            ])
        );
        let mut fixture = Fixture::start(vec![if case == "budget" {
            Reply::stream(format!("{start}{}", frames(&[terminal(&native, 3)])))
        } else {
            Reply {
                stall: 2,
                ..Reply::stream(start)
            }
        }])
        .await;
        let (broker, reads) = broker();
        let provider = OllamaProvider::new(
            OllamaConfig::new(&fixture.base, Some(reference())).unwrap(),
            vec![model("fixture", "native-fixture")],
            broker,
            Limits {
                request_bytes: if case == "budget" {
                    550
                } else {
                    limits().request_bytes
                },
                ..limits()
            },
        )
        .unwrap()
        .with_structured_output();
        let token = CancellationToken::new();
        let mut events = provider
            .stream_response(
                stream_request(json!({"type":"object"})),
                RequestContext {
                    cancellation: token.clone(),
                    deadline: (case == "deadline")
                        .then(|| std::time::Instant::now() + Duration::from_millis(400)),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .events;
        fixture.request().await;
        loop {
            let event = events.next().await.unwrap().unwrap();
            if let ProviderStreamEvent::Model(event) = event {
                assert_ne!(event.response.kind(), "response.output_item.done");
                if event.response.kind() == "response.output_text.delta" {
                    break;
                }
            }
        }
        if case == "deadline" {
            tokio::time::sleep(Duration::from_millis(450)).await;
        }
        if case == "cancel" {
            token.cancel();
        }
        if case != "drop" {
            assert_eq!(
                events.next().await.unwrap().err().unwrap().code,
                match case {
                    "cancel" => "provider_cancelled",
                    "deadline" => "provider_timeout",
                    _ => "ollama_history_too_large",
                }
            );
            assert!(events.next().await.is_none());
        }
        drop(events);
        fixture.disconnected().await;
        // A released slot can start the next round without introducing a worker.
        let stream = provider
            .stream_response(
                stream_request(json!({"type":"object"})),
                RequestContext::default(),
            )
            .await
            .unwrap();
        fixture.request().await;
        drop(stream);
        fixture.disconnected().await;
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn recursive_schemas_and_pure_ref_cycles_are_validated_offline() {
    for (schema, text, valid) in [
        (json!({"$ref":"#"}), "{}", true),
        (
            json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"array","items":{"type":"integer"},"minItems":2,"uniqueItems":true}),
            "[1,2]",
            true,
        ),
        (
            json!({"type":"string","pattern":"(?<=a)b"}),
            r#""ab""#,
            true,
        ),
        (
            json!({"type":"string","pattern":"(?<=a)b"}),
            r#""cb""#,
            false,
        ),
        (
            json!({"$defs":{"a":{"$ref":"#/$defs/b"},"b":{"$ref":"#/$defs/a","type":"integer"}},"$ref":"#/$defs/a"}),
            "1",
            true,
        ),
        (
            json!({"$defs":{"a":{"$ref":"#/$defs/b"},"b":{"$ref":"#/$defs/a","type":"integer"}},"$ref":"#/$defs/a"}),
            "false",
            false,
        ),
        (
            json!({"type":"object","properties":{"value":{"type":"integer"},"child":{"$ref":"#"}},"required":["value"],"additionalProperties":false}),
            r#"{"value":1,"child":{"value":2}}"#,
            true,
        ),
        (
            json!({"type":"object","properties":{"value":{"type":"integer"},"child":{"$ref":"#"}},"required":["value"],"additionalProperties":false}),
            r#"{"value":1,"child":{"value":false}}"#,
            false,
        ),
    ] {
        let mut fixture = Fixture::start(vec![Reply::json(answer(json!([message(text)])))]).await;
        let (broker, _) = broker();
        let provider = fixture.provider(broker, true).with_structured_output();
        let result = provider
            .create_response(request(strict(schema)), RequestContext::default())
            .await;
        if valid {
            result.unwrap();
        } else {
            assert_eq!(
                result.err().unwrap().code,
                "ollama_invalid_structured_output"
            );
        }
        fixture.request().await;
    }
}
