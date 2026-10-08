use super::*;
fn frames(chunks: &[Value]) -> String {
    chunks
        .iter()
        .map(|chunk| {
            format!(
                "event: {}\ndata: {chunk}\n\n",
                chunk["type"].as_str().unwrap()
            )
        })
        .collect()
}

#[tokio::test]
async fn text_without_native_reasoning_shifts_indices_and_streams_before_terminal() {
    let message = json!({"type":"message","id":"msg_1","status":"completed","role":"assistant","content":[{"type":"output_text","text":"answer🙂","annotations":[]}]});
    let terminal =
        json!({"id":"fixture","model":"native-fixture","status":"completed","output":[message]});
    let start = [
        serde_json::from_str::<Value>(
            CREATED
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .unwrap(),
        )
        .unwrap(),
        json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"type":"message","id":"msg_1","status":"in_progress","role":"assistant","content":[]}}),
        json!({"type":"response.content_part.added","sequence_number":2,"output_index":0,"item_id":"msg_1","content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
        json!({"type":"response.output_text.delta","sequence_number":3,"output_index":0,"item_id":"msg_1","content_index":0,"delta":"answer🙂"}),
    ];
    for complete in [false, true] {
        let mut chunks = start.to_vec();
        if complete {
            chunks.push(json!({"type":"response.output_item.done","sequence_number":4,"output_index":0,"item":message}));
            chunks
                .push(json!({"type":"response.completed","sequence_number":5,"response":terminal}));
        }
        let mut fixture = Fixture::start(vec![Reply {
            stall: if complete { 0 } else { 2 },
            ..Reply::stream(frames(&chunks))
        }])
        .await;
        let (broker, _) = broker();
        let provider = fixture.provider(broker, true).with_native_history();
        let mut stream = provider
            .stream_response(
                request(json!({"model":"fixture","input":"hello","stream":true})),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .events;
        fixture.request().await;
        let mut progress = false;
        let mut has_terminal = false;
        loop {
            let event = stream.next().await;
            let Some(event) = event else { break };
            if let ProviderStreamEvent::Model(event) = event.unwrap() {
                if event.response.kind() == "response.output_text.delta" {
                    assert_eq!(event.response.wire()["output_index"], 1);
                    assert_eq!(event.response.wire()["delta"], "answer🙂");
                    progress = true;
                    if !complete {
                        break;
                    }
                }
                if event.response.terminal().is_some() {
                    assert!(progress);
                    assert_eq!(event.response.wire()["response"]["output"][1], message);
                    has_terminal = true;
                }
            }
        }
        assert!(progress);
        assert_eq!(has_terminal, complete);
        drop(stream);
        fixture.disconnected().await;
    }
}

#[tokio::test]
async fn terminal_validation_failed_incomplete_and_frame_limits_do_not_release_bad_or_partial_calls()
 {
    for case in [
        "truncated",
        "mismatch",
        "failed",
        "incomplete",
        "frame_limit",
    ] {
        let mut terminal = native().wire().clone();
        terminal["output"].as_array_mut().unwrap().pop();
        if case == "failed" {
            terminal["status"] = "failed".into();
            terminal["error"] = json!({"message":KEY});
        }
        if case == "incomplete" {
            terminal["status"] = "incomplete".into();
            terminal["incomplete_details"] = json!({"reason":"max_output_tokens"});
        }
        let kind = match case {
            "failed" => "response.failed",
            "incomplete" => "response.incomplete",
            _ => "response.completed",
        };
        let mut call = terminal["output"][1].clone();
        if case == "mismatch" {
            call["call_id"] = "different".into();
        }
        let end = json!({"type":kind,"sequence_number":4,"response":terminal});
        let mut chunks = vec![
            serde_json::from_str::<Value>(
                CREATED
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .unwrap(),
            )
            .unwrap(),
            json!({"type":"response.output_item.added","sequence_number":1,"output_index":1,"item":{"type":"function_call","id":"fc_1","name":"echo","call_id":"c1","arguments":""}}),
            json!({"type":"response.function_call_arguments.delta","sequence_number":2,"output_index":1,"item_id":"fc_1","delta":call["arguments"]}),
            json!({"type":"response.output_item.done","sequence_number":3,"output_index":1,"item":call}),
        ];
        if case != "truncated" {
            chunks.push(end.clone());
        }
        let mut fixture = Fixture::start(vec![Reply::stream(frames(&chunks))]).await;
        let (broker, _) = broker();
        let provider = OllamaProvider::new(
            OllamaConfig::new(&fixture.base, Some(reference())).unwrap(),
            vec![model("fixture", "native-fixture")],
            broker,
            Limits {
                frame_bytes: if case == "frame_limit" {
                    end.to_string().len() + 100
                } else {
                    BUDGET
                },
                ..limits()
            },
        )
        .unwrap()
        .with_native_history();
        let mut stream = provider
            .stream_response(
                request(json!({"model":"fixture","input":"hello","stream":true})),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .events;
        fixture.request().await;
        let mut state = None;
        let mut error = None;
        let mut carrier = false;
        while let Some(event) = stream.next().await {
            match event {
                Err(e) => {
                    assert!(!format!("{e:?}").contains(KEY));
                    error = Some(e.code);
                }
                Ok(ProviderStreamEvent::Model(event)) => {
                    if event.response.kind() == "response.output_item.done" {
                        assert_ne!(event.response.wire()["item"]["type"], "function_call");
                        carrier |= event.response.wire()["item"]["type"] == "reasoning";
                    }
                    if let Some(terminal) = event.response.terminal() {
                        state = Some(terminal);
                    }
                }
                _ => (),
            }
        }
        match case {
            "failed" => {
                assert_eq!(state, Some(StreamState::Failed));
                assert!(carrier);
                assert!(error.is_none());
            }
            "incomplete" => {
                assert_eq!(state, Some(StreamState::Incomplete));
                assert!(carrier);
                assert!(error.is_none());
            }
            "mismatch" => {
                assert_eq!(error, Some("ollama_invalid_native_history"));
                assert!(!carrier);
            }
            "frame_limit" => {
                assert_eq!(error, Some("ollama_history_too_large"));
                assert!(!carrier);
            }
            _ => {
                assert_eq!(error, Some("provider_stream_truncated"));
                assert!(!carrier);
            }
        }
    }
}

#[tokio::test]
async fn cancellation_drop_and_busy_close_the_shared_native_socket_without_executable_output() {
    let mut fixture=Fixture::start(vec![Reply{stall:2,..Reply::stream(format!("{CREATED}{}",frames(&[
        json!({"type":"response.reasoning_summary_text.delta","sequence_number":1,"output_index":0,"item_id":"native-rs","summary_index":0,"delta":"progress before terminal"})]))) }]).await;
    let (broker, reads) = broker();
    let provider = fixture.provider(broker, true).with_native_history();
    for cancel in [false, true] {
        let token = CancellationToken::new();
        let mut stream = provider
            .stream_response(
                request(json!({"model":"fixture","input":"hello","stream":true})),
                RequestContext {
                    cancellation: token.clone(),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .events;
        fixture.request().await;
        loop {
            if let ProviderStreamEvent::Model(event) = stream.next().await.unwrap().unwrap() {
                assert_ne!(event.response.kind(), "response.output_item.done");
                if event.response.kind() == "response.reasoning_summary_text.delta" {
                    break;
                }
            }
        }
        assert_eq!(
            provider
                .stream_response(
                    request(json!({"model":"fixture","input":"hello","stream":true})),
                    RequestContext::default()
                )
                .await
                .err()
                .unwrap()
                .code,
            "provider_busy"
        );
        if cancel {
            token.cancel();
            assert_eq!(
                stream.next().await.unwrap().err().unwrap().code,
                "provider_cancelled"
            );
            assert!(stream.next().await.is_none());
        }
        drop(stream);
        fixture.disconnected().await;
    }
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn stream_provider_keeps_progress_and_delivers_bound_history_before_executable_calls() {
    let mut terminal = native().wire().clone();
    terminal["output"].as_array_mut().unwrap().pop();
    let reasoning = terminal["output"][0].clone();
    let call = terminal["output"][1].clone();
    let chunks = [
        serde_json::from_str::<Value>(
            CREATED
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .unwrap(),
        )
        .unwrap(),
        json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"type":"reasoning","id":"native-rs","summary":[]}}),
        json!({"type":"response.reasoning_summary_text.delta","sequence_number":2,"output_index":0,"item_id":"native-rs","summary_index":0,"delta":"private thought🙂"}),
        json!({"type":"response.output_item.done","sequence_number":3,"output_index":0,"item":reasoning}),
        json!({"type":"response.output_item.added","sequence_number":4,"output_index":1,"item":{"type":"function_call","id":"fc_1","status":"in_progress","name":"echo","call_id":"c1","arguments":""}}),
        json!({"type":"response.function_call_arguments.delta","sequence_number":5,"output_index":1,"item_id":"fc_1","delta":call["arguments"]}),
        json!({"type":"response.output_item.done","sequence_number":6,"output_index":1,"item":call}),
        json!({"type":"response.completed","sequence_number":7,"response":terminal}),
    ];
    let body = frames(&chunks);
    let mut fixture = Fixture::start(vec![Reply::stream(body)]).await;
    let (broker, reads) = broker();
    let provider = fixture.provider(broker, true).with_native_history();
    let original = json!({"role":"user","content":"hello"});
    let mut input = vec![original.clone()];
    let mut native_input = input.clone();
    for turn in 0..2 {
        let mut stream = provider
            .stream_response(
                request(json!({"model":"fixture","input":input,"stream":true})),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .events;
        assert_eq!(
            fixture.request().await.body.unwrap()["input"],
            json!(native_input)
        );
        let mut has_carrier = false;
        let mut has_progress = false;
        let mut projected = None;
        let mut parser = caidex_model_core::ResponsesStream::new(BUDGET).unwrap();
        while let Some(event) = stream.next().await {
            if let ProviderStreamEvent::Model(event) = event.unwrap() {
                parser
                    .push(
                        format!(
                            "event: {}\ndata: {}\n\n",
                            event.frame.event, event.frame.data
                        )
                        .as_bytes(),
                    )
                    .unwrap();
                if event.response.kind() == "response.reasoning_summary_text.delta" {
                    has_progress = true;
                }
                if event.response.kind() == "response.output_item.done" {
                    if event.response.wire()["item"]["type"] == "reasoning" {
                        assert!(
                            event.response.wire()["item"]["encrypted_content"]
                                .as_str()
                                .unwrap()
                                .starts_with(PREFIX)
                        );
                        has_carrier = true;
                    } else if event.response.wire()["item"]["type"] == "function_call" {
                        assert!(has_carrier);
                    }
                }
                if event.response.terminal().is_some() {
                    projected = Some(
                        CanonicalResponse::new(event.response.wire()["response"].clone()).unwrap(),
                    );
                }
            }
        }
        assert_eq!(parser.finish().unwrap(), StreamState::Completed);
        assert!(has_progress && has_carrier);
        let projected = projected.unwrap();
        let result =
            json!({"type":"function_call_output","call_id":"c1","output":format!("result {turn}")});
        input.extend(projected.output().iter().cloned());
        input.push(result.clone());
        native_input.extend(terminal["output"].as_array().unwrap().iter().cloned());
        native_input.push(result);
    }
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 2);
}
