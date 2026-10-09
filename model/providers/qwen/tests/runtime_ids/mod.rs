use super::*;

fn source(streaming: bool) -> Value {
    json!({"model":"fixture","stream":streaming,"input":[
        {"type":"message","role":"developer","id":"msg_developer","content":[{"type":"input_text","text":"Keep the rules"}]},
        {"type":"message","role":"user","id":"msg_user","content":[{"type":"input_text","text":"Question"}]}]})
}

#[tokio::test]
async fn explicit_runtime_message_ids_preserve_roles_and_native_prefix_json_sse() {
    for history in [false, true] {
        for streaming in [false, true] {
            let original = source(streaming);
            let reply = if streaming {
                Reply::stream(created() + &terminal(native()))
            } else {
                Reply::json(native())
            };
            let mut fixture = Fixture::start(vec![reply, Reply::json(native())]).await;
            let (broker, reads) = fixture_broker(Some(KEY));
            let mut enabled = fixture
                .provider(
                    broker,
                    vec![metadata("fixture", "native-fixture")],
                    limits(),
                )
                .with_runtime_context();
            if history {
                enabled = enabled.with_native_history();
            }
            let response = if streaming {
                let mut events = enabled
                    .stream_response(request(original.clone()), runtime_context())
                    .await
                    .unwrap()
                    .events;
                let mut terminal = None;
                while let Some(event) = events.next().await {
                    if let ProviderStreamEvent::Model(event) = event.unwrap()
                        && event.response.kind() == "response.completed"
                    {
                        terminal = Some(event.response.wire()["response"].clone());
                    }
                }
                caidex_model_core::CanonicalResponse::new(terminal.unwrap()).unwrap()
            } else {
                enabled
                    .create_response(request(original.clone()), runtime_context())
                    .await
                    .unwrap()
                    .response
            };
            let sent = fixture.captured().await.body.unwrap();
            assert_eq!(sent["input"], original["input"]);
            if history {
                assert_eq!(capsule(response.output())["request"], sent);
                let mut next = followup(&original, response.output());
                next["stream"] = false.into();
                for change in ["id", "role", "text"] {
                    let mut bad = next.clone();
                    match change {
                        "id" => bad["input"][1]["id"] = "msg_other".into(),
                        "role" => bad["input"][1]["role"] = "developer".into(),
                        _ => bad["input"][1]["content"][0]["text"] = "other".into(),
                    }
                    assert_eq!(
                        enabled
                            .create_response(request(bad), runtime_context())
                            .await
                            .err()
                            .unwrap()
                            .http_status,
                        400
                    );
                }
                assert_eq!(reads.load(Ordering::SeqCst), 1);
                enabled
                    .create_response(request(next), runtime_context())
                    .await
                    .unwrap();
                let replay = fixture.captured().await.body.unwrap();
                assert_eq!(replay["input"][0], original["input"][0]);
                assert_eq!(replay["input"][1], original["input"][1]);
                assert_eq!(reads.load(Ordering::SeqCst), 2);
            }
        }
    }
}

#[tokio::test]
async fn runtime_message_ids_require_explicit_policy_and_valid_typed_input_before_keys() {
    let fixture = Fixture::start(vec![Reply::json(native())]).await;
    let (broker, reads) = fixture_broker(Some(KEY));
    for history in [false, true] {
        let mut default = fixture.provider(
            broker.clone(),
            vec![metadata("fixture", "native-fixture")],
            limits(),
        );
        if history {
            default = default.with_native_history();
        }
        assert_eq!(
            default
                .create_response(request(source(false)), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            400
        );
        let enabled = default.with_runtime_context();
        for corrupt in 0..7 {
            let mut wire = source(false);
            match corrupt {
                0 => wire["input"][0]["id"] = Value::Null,
                1 => wire["input"][1]["id"] = 5.into(),
                2 => wire["input"][1]["id"] = "bad\n".into(),
                3 => wire["input"][0]["id"] = " ".into(),
                4 => wire["input"][1]["status"] = "completed".into(),
                5 => wire["input"][0]["role"] = "system".into(),
                _ => wire["input"][1]["content"] = "text".into(),
            }
            for streaming in [false, true] {
                wire["stream"] = streaming.into();
                let error = if streaming {
                    enabled
                        .stream_response(request(wire.clone()), runtime_context())
                        .await
                        .err()
                        .unwrap()
                } else {
                    enabled
                        .create_response(request(wire.clone()), runtime_context())
                        .await
                        .err()
                        .unwrap()
                };
                assert_eq!(error.http_status, 400, "case {corrupt}");
            }
        }
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}
