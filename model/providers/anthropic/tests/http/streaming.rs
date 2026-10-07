use super::*;
use caidex_provider_anthropic::{NativeStreamEvent, NativeStreamingResponse};
use futures_util::StreamExt;

struct Fixture {
    base: String,
    requests: mpsc::UnboundedReceiver<Value>,
    closed: mpsc::UnboundedReceiver<()>,
    task: tokio::task::JoinHandle<()>,
}
impl Fixture {
    async fn start(body: String, end_http: bool, content_type: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let (tx, requests) = mpsc::unbounded_channel();
        let (closed_tx, closed) = mpsc::unbounded_channel();
        let content_type = content_type.to_owned();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let (offset, length) = loop {
                let mut buffer = [0; 1024];
                let n = socket.read(&mut buffer).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
                if let Some(offset) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                    let head = String::from_utf8(bytes[..offset].to_vec()).unwrap();
                    assert!(
                        head.to_ascii_lowercase()
                            .contains("accept: text/event-stream")
                    );
                    assert!(
                        head.to_ascii_lowercase()
                            .contains("x-api-key: synthetic_anthropic_key")
                    );
                    assert!(!head.to_ascii_lowercase().contains("authorization:"));
                    let length = head
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    break (offset + 4, length);
                }
            };
            while bytes.len() < offset + length {
                let mut buffer = [0; 1024];
                let n = socket.read(&mut buffer).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
            }
            tx.send(serde_json::from_slice(&bytes[offset..offset + length]).unwrap())
                .unwrap();
            socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ntransfer-encoding: chunked\r\n\r\n").as_bytes()).await.unwrap();
            for piece in body.as_bytes().chunks(73) {
                let chunk = format!("{:x}\r\n", piece.len());
                if socket.write_all(chunk.as_bytes()).await.is_err()
                    || socket.write_all(piece).await.is_err()
                    || socket.write_all(b"\r\n").await.is_err()
                {
                    break;
                }
            }
            if end_http {
                let _ = socket.write_all(b"0\r\n\r\n").await;
            }
            let mut byte = [0];
            let result = socket.read(&mut byte).await;
            assert!(matches!(result, Ok(0) | Err(_)));
            closed_tx.send(()).unwrap();
        });
        Self {
            base,
            requests,
            closed,
            task,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn event(wire: Value) -> String {
    format!(
        "event: {}\ndata: {wire}\n\n",
        wire["type"].as_str().unwrap()
    )
}
fn start() -> String {
    event(
        json!({"type":"message_start","message":{"type":"message","id":"native-stream",
    "role":"assistant","model":"native","content":[],"stop_reason":null,"usage":{"input_tokens":7,"output_tokens":1}}}),
    )
}
fn complete() -> String {
    [start(),event(json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}})),
    event(json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"中文🙂"}})),
    event(json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"signed+/==\n"}})),
    event(json!({"type":"content_block_stop","index":0})),
    event(json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tool-fixture","name":"data_only","input":{}}})),
    event(json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"n\":18446744073709551616}"}})),
    event(json!({"type":"content_block_stop","index":1})),
    event(json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":4}})),
    event(json!({"type":"message_stop"}))].concat()
}
async fn next(
    stream: &mut NativeStreamingResponse,
) -> Option<caidex_model_core::ProviderResult<NativeStreamEvent>> {
    tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .unwrap()
}
async fn released(client: &AnthropicClient<Store>) {
    let error = client
        .discover_models(2, RequestContext::default())
        .await
        .unwrap_err();
    assert_ne!(error.code, "provider_busy");
}

#[tokio::test]
async fn model_provider_streams_signed_function_history_for_classic_and_lite() {
    use caidex_model_core::{
        CanonicalRequest, ModelProvider, ProviderStreamEvent, ResponsesDialect,
    };
    use caidex_provider_anthropic::{AnthropicProvider, NativeMessage, ToolMap};
    let tools = [json!({"type":"function","name":"data_only","parameters":{"type":"object"}})];
    let map = ToolMap::new(&tools, 10).unwrap();
    let body = complete().replace("\"data_only\"", &map.native_tools()[0]["name"].to_string());
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let mut fixture = Fixture::start(body.clone(), false, "text/event-stream").await;
        let (client, reads) = client(&fixture.base, Some(KEY), Limits::default());
        let provider =
            AnthropicProvider::new(client, vec![super::provider::profile()], 10).unwrap();
        let prompt = json!({"role":"user","content":"hello"});
        let wire = if dialect == ResponsesDialect::Classic {
            json!({"model":"alias","tools":tools,"input":[prompt],"stream":true})
        } else {
            json!({"model":"alias","input":[{"type":"additional_tools","role":"developer","tools":tools},prompt],"stream":true})
        };
        let mut stream = provider
            .stream_response(
                CanonicalRequest::new(wire, dialect).unwrap(),
                RequestContext::default(),
            )
            .await
            .unwrap()
            .events;
        let mut completed = None;
        let mut arguments = String::new();
        while let Some(item) = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
        {
            if let ProviderStreamEvent::Model(event) = item.unwrap() {
                if event.response.kind() == "response.function_call_arguments.delta" {
                    arguments.push_str(event.response.wire()["delta"].as_str().unwrap());
                }
                if event.response.kind() == "response.completed" {
                    completed = Some(event.response.wire()["response"].clone());
                }
            }
        }
        let completed = completed.unwrap();
        assert_eq!(arguments, "{\"n\":18446744073709551616}");
        assert_eq!(completed["output"][1]["name"], "data_only");
        let restored = NativeMessage::from_responses_output(
            completed["output"].as_array().unwrap(),
            "native",
            128 * 1024,
        )
        .unwrap();
        assert_eq!(restored.content()[0]["signature"], "signed+/==\n");
        assert_eq!(restored.content()[1]["name"], map.native_tools()[0]["name"]);
        let sent = received(&mut fixture.requests).await;
        assert_eq!(sent["model"], "native");
        assert_eq!(sent["max_tokens"], 100);
        assert_eq!(sent["stream"], true);
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        received(&mut fixture.closed).await;
    }
}

#[tokio::test]
async fn native_http_stream_preserves_signed_blocks_tools_usage_and_closes_without_http_eof() {
    let mut fixture = Fixture::start(complete(), false, "text/event-stream; charset=utf-8").await;
    let (client, _) = client(&fixture.base, Some(KEY), Limits::default());
    let mut stream = client
        .stream_message("native", request(), RequestContext::default())
        .await
        .unwrap();
    let wire = received(&mut fixture.requests).await;
    assert_eq!(wire["model"], "native");
    assert_eq!(wire["stream"], true);
    let mut count = 0;
    let mut final_message = None;
    while let Some(item) = next(&mut stream).await {
        match item.unwrap() {
            NativeStreamEvent::Event(_) => count += 1,
            NativeStreamEvent::Completed(message) => final_message = Some(message),
        }
    }
    assert_eq!(count, 10);
    let message = final_message.unwrap();
    assert_eq!(message.content()[0]["thinking"], "中文🙂");
    assert_eq!(message.content()[0]["signature"], "signed+/==\n");
    assert_eq!(
        message.content()[1]["input"]["n"].to_string(),
        "18446744073709551616"
    );
    assert_eq!(message.wire()["usage"]["output_tokens"], 4);
    assert_eq!(
        message.outcome(),
        caidex_provider_anthropic::MessageOutcome::ToolUse
    );
    received(&mut fixture.closed).await;
}
#[tokio::test]
async fn native_error_events_never_deliver_raw_diagnostics_or_completion() {
    for kind in ["overloaded_error", "rate_limit_error", "future_error"] {
        let body = event(
            json!({"type":"error","error":{"type":kind,"message":KEY,"url":"https://private.fixture"}}),
        );
        let mut fixture = Fixture::start(body, false, "text/event-stream").await;
        let (client, _) = client(&fixture.base, Some(KEY), Limits::default());
        let mut stream = client
            .stream_message("native", request(), RequestContext::default())
            .await
            .unwrap();
        let error = next(&mut stream).await.unwrap().unwrap_err();
        assert_eq!(
            error.code,
            match kind {
                "overloaded_error" => "provider_unavailable",
                "rate_limit_error" => "provider_rate_limited",
                _ => "provider_stream_error",
            }
        );
        assert!(!format!("{error:?}").contains(KEY));
        assert!(!format!("{error:?}").contains("private.fixture"));
        assert!(next(&mut stream).await.is_none());
        received(&mut fixture.closed).await;
    }
}
#[tokio::test]
async fn truncated_eof_and_stream_bounds_never_emit_completed() {
    for mode in ["eof", "frame", "budget"] {
        let body = if mode == "eof" { start() } else { complete() };
        let mut fixture = Fixture::start(body, true, "text/event-stream").await;
        let limits = Limits {
            frame_bytes: if mode == "frame" { 20 } else { 4096 },
            response_bytes: if mode == "budget" { 40 } else { 16000 },
            ..Default::default()
        };
        let (client, _) = client(&fixture.base, Some(KEY), limits);
        let mut stream = client
            .stream_message("native", request(), RequestContext::default())
            .await
            .unwrap();
        let mut failure = None;
        while let Some(item) = next(&mut stream).await {
            match item {
                Ok(NativeStreamEvent::Completed(_)) => panic!("unexpected completion"),
                Ok(_) => (),
                Err(error) => failure = Some(error),
            }
        }
        assert_eq!(
            failure.unwrap().code,
            match mode {
                "eof" => "anthropic_stream_truncated",
                "frame" => "anthropic_invalid_stream",
                _ => "anthropic_stream_too_large",
            }
        );
        received(&mut fixture.closed).await;
    }
}
#[tokio::test]
async fn dropping_delivery_and_cancelling_idle_stream_close_the_socket_and_release_slot() {
    for drop_delivery in [false, true] {
        let mut fixture = Fixture::start(start(), false, "text/event-stream").await;
        let (client, _) = client(
            &fixture.base,
            Some(KEY),
            Limits {
                in_flight: 1,
                ..Default::default()
            },
        );
        let cancellation = CancellationToken::new();
        let mut stream = client
            .stream_message(
                "native",
                request(),
                RequestContext {
                    cancellation: cancellation.clone(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            next(&mut stream).await.unwrap().unwrap(),
            NativeStreamEvent::Event(_)
        ));
        assert_eq!(
            client
                .discover_models(2, RequestContext::default())
                .await
                .unwrap_err()
                .code,
            "provider_busy"
        );
        if drop_delivery {
            drop(stream);
        } else {
            tokio::task::yield_now().await;
            cancellation.cancel();
            assert_eq!(
                next(&mut stream).await.unwrap().unwrap_err().code,
                "provider_cancelled"
            );
            assert!(next(&mut stream).await.is_none());
        }
        received(&mut fixture.closed).await;
        tokio::task::yield_now().await;
        released(&client).await;
    }
}
#[tokio::test]
async fn unconsumed_full_slot_deadline_and_cancellation_preserve_error_and_release_permit() {
    for cancel in [false, true] {
        let body = [
            start(),
            event(json!({"type":"ping"})),
            event(json!({"type":"ping"})),
        ]
        .concat();
        let mut fixture = Fixture::start(body, false, "text/event-stream").await;
        let (client, _) = client(
            &fixture.base,
            Some(KEY),
            Limits {
                in_flight: 1,
                total_timeout: Duration::from_millis(500),
                ..Default::default()
            },
        );
        let cancellation = CancellationToken::new();
        let mut stream = client
            .stream_message(
                "native",
                request(),
                RequestContext {
                    cancellation: cancellation.clone(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        received(&mut fixture.requests).await;
        if cancel {
            // First recv guarantees a delivered native event; leave the next
            // slot unconsumed and cancel while the worker still owns I/O.
            assert!(next(&mut stream).await.unwrap().is_ok());
            tokio::task::yield_now().await;
            cancellation.cancel();
        }
        received(&mut fixture.closed).await;
        tokio::task::yield_now().await;
        released(&client).await;
        let mut found = None;
        while let Some(item) = next(&mut stream).await {
            if let Err(error) = item {
                found = Some(error);
            } else {
                assert!(!matches!(item.unwrap(), NativeStreamEvent::Completed(_)));
            }
        }
        assert_eq!(
            found.unwrap().code,
            if cancel {
                "provider_cancelled"
            } else {
                "provider_timeout"
            }
        );
    }
}
#[tokio::test]
async fn idle_timeout_and_bad_content_type_fail_without_completion() {
    for bad_media in [false, true] {
        let mut fixture = Fixture::start(
            String::new(),
            false,
            if bad_media {
                "application/json"
            } else {
                "text/event-stream"
            },
        )
        .await;
        let (client, _) = client(
            &fixture.base,
            Some(KEY),
            Limits {
                idle_timeout: Duration::from_millis(200),
                ..Default::default()
            },
        );
        let result = client
            .stream_message("native", request(), RequestContext::default())
            .await;
        if bad_media {
            assert_eq!(result.err().unwrap().code, "provider_invalid_content_type");
        } else {
            let mut stream = result.unwrap();
            assert_eq!(
                next(&mut stream).await.unwrap().unwrap_err().code,
                "provider_timeout"
            );
        }
        received(&mut fixture.closed).await;
    }
}

#[tokio::test]
async fn projected_http_text_arrives_before_native_stop_and_cancellation_closes_transport() {
    use caidex_model_core::ProviderStreamEvent;
    use caidex_provider_anthropic::{ProjectedStreamingResponse, ResponsesProjection, ToolMap};
    let body = [start(),event(json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})),event(json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"live🙂"}}))].concat();
    let mut fixture = Fixture::start(body, false, "text/event-stream").await;
    let (client, _) = client(&fixture.base, Some(KEY), Limits::default());
    let context = RequestContext::default();
    let cancel = context.cancellation.clone();
    let native = client
        .stream_message("native", request(), context)
        .await
        .unwrap();
    let projection =
        ResponsesProjection::new("native".into(), ToolMap::new(&[], 10).unwrap(), 128 * 1024)
            .unwrap();
    let mut stream = ProjectedStreamingResponse::new(native, projection);
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let ProviderStreamEvent::Model(event) = event {
            assert!(event.response.terminal().is_none());
            assert_ne!(event.response.kind(), "response.output_item.done");
            if event.response.text_delta() == Some("live🙂") {
                break;
            }
        }
    }
    cancel.cancel();
    let error = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, "provider_cancelled");
    assert!(stream.next().await.is_none());
    received(&mut fixture.closed).await;
    released(&client).await;
}

#[tokio::test]
async fn projected_http_complete_signed_function_history_preserves_raw_arguments_and_drop_closes() {
    use caidex_model_core::ProviderStreamEvent;
    use caidex_provider_anthropic::{
        NativeMessage, ProjectedStreamingResponse, ResponsesProjection, ToolMap,
    };
    let map = ToolMap::new(
        &[json!({"type":"function","name":"data_only","parameters":{"type":"object"}})],
        10,
    )
    .unwrap();
    let body = complete().replace(
        "\"name\":\"data_only\"",
        &format!("\"name\":{}", map.native_tools()[0]["name"]),
    );
    let mut fixture = Fixture::start(body, false, "text/event-stream").await;
    let (client, _) = super::client(&fixture.base, Some(KEY), Limits::default());
    let native = client
        .stream_message("native", request(), RequestContext::default())
        .await
        .unwrap();
    let mut stream = ProjectedStreamingResponse::new(
        native,
        ResponsesProjection::new("native".into(), map, 128 * 1024).unwrap(),
    );
    let mut response = None;
    while let Some(event) = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .unwrap()
    {
        if let ProviderStreamEvent::Model(event) = event.unwrap()
            && event.response.terminal().is_some()
        {
            response = Some(event.response.wire()["response"].clone());
        }
    }
    let response = response.unwrap();
    let native = NativeMessage::from_responses_output(
        response["output"].as_array().unwrap(),
        "native",
        128 * 1024,
    )
    .unwrap();
    assert_eq!(native.content()[0]["signature"], "signed+/==\n");
    assert_eq!(
        response["output"][1]["arguments"],
        "{\"n\":18446744073709551616}"
    );
    received(&mut fixture.closed).await;
    released(&client).await;

    let mut fixture = Fixture::start(start(), false, "text/event-stream").await;
    let (client, _) = super::client(&fixture.base, Some(KEY), Limits::default());
    let native = client
        .stream_message("native", request(), RequestContext::default())
        .await
        .unwrap();
    let stream = ProjectedStreamingResponse::new(
        native,
        ResponsesProjection::new("native".into(), ToolMap::new(&[], 10).unwrap(), 128 * 1024)
            .unwrap(),
    );
    drop(stream);
    received(&mut fixture.closed).await;
    released(&client).await;
}
