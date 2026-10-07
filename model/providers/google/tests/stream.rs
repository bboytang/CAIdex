use caidex_provider_google::{CandidateOutcome, ContentStream, NativeStreamState};
use serde_json::{Value, json};

fn frames(chunks: &[Value]) -> Vec<u8> {
    let mut bytes = b"\xef\xbb\xbf: heartbeat\r\n\r\n".to_vec();
    for chunk in chunks {
        bytes.extend_from_slice(format!("data: {chunk}\r\n\r\n").as_bytes());
    }
    bytes
}
fn candidate(index: u64, parts: Value, reason: Option<&str>) -> Value {
    let mut wire = json!({"candidates":[{"index":index,"content":{"role":"model","parts":parts}}]});
    if let Some(reason) = reason {
        wire["candidates"][0]["finishReason"] = reason.into();
    }
    wire
}

#[test]
fn every_byte_split_preserves_native_chunks_parts_and_late_usage_until_eof() {
    let mut first = candidate(
        0,
        json!([{"text":"思考","thought":true,"thoughtSignature":"opaque-a"}]),
        None,
    );
    first["responseId"] = "fixture-id".into();
    first["modelVersion"] = "fixture-001".into();
    first["future"] = serde_json::from_str("{\"big\":18446744073709551616}").unwrap();
    let chunks = vec![
        first,
        candidate(
            0,
            json!([{"text":"你好","thoughtSignature":"opaque-b"},{"futurePart":{"raw":true}}]),
            None,
        ),
        candidate(
            0,
            json!([{"functionCall":{"id":"call-1","name":"echo","args":{"text":"原文"}},"thoughtSignature":"opaque-call"}]),
            Some("STOP"),
        ),
        json!({"usageMetadata":{"promptTokenCount":8,"cachedContentTokenCount":2,"candidatesTokenCount":3,"thoughtsTokenCount":4,"totalTokenCount":15,"future":{"x":true}},"future":{"tail":true}}),
    ];
    let bytes = frames(&chunks);
    for split in 0..=bytes.len() {
        let mut stream = ContentStream::new(4096, 32768, 1).unwrap();
        let mut events = stream.push(&bytes[..split]).unwrap();
        assert!(stream.completed_response().is_none());
        events.extend(stream.push(&bytes[split..]).unwrap());
        assert_eq!(stream.state(), NativeStreamState::Open);
        assert!(stream.completed_response().is_none());
        assert_eq!(events.len(), chunks.len());
        for (event, chunk) in events.iter().zip(&chunks) {
            assert_eq!(event.wire(), chunk);
            assert_eq!(
                serde_json::from_str::<Value>(&event.frame().data).unwrap(),
                *chunk
            );
            assert!(!format!("{event:?}").contains("opaque"));
        }
        assert_eq!(stream.finish().unwrap(), NativeStreamState::Completed);
        let completed = stream.completed_response().unwrap();
        assert_eq!(completed.chunks(), chunks);
        assert_eq!(
            completed.chunks()[0]["future"]["big"].to_string(),
            "18446744073709551616"
        );
        let response = completed.response();
        assert_eq!(response.outcome(0), Some(CandidateOutcome::ToolCall));
        let expected = chunks[..3]
            .iter()
            .flat_map(|v| {
                v["candidates"][0]["content"]["parts"]
                    .as_array()
                    .unwrap()
                    .clone()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            response.wire()["candidates"][0]["content"]["parts"],
            json!(expected)
        );
        assert_eq!(response.wire()["usageMetadata"], chunks[3]["usageMetadata"]);
        assert!(!format!("{completed:?}").contains("opaque"));
        assert!(stream.push(b"data: {}\n\n").is_err());
    }
    let mut stream = ContentStream::new(4096, 32768, 1).unwrap();
    let mut count = 0;
    for byte in bytes {
        count += stream.push(&[byte]).unwrap().len();
    }
    assert_eq!(count, chunks.len());
    stream.finish().unwrap();
}

#[test]
fn all_requested_candidates_must_finish_and_prompt_blocking_stays_distinct() {
    let chunks = vec![
        candidate(1, json!([{"text":"second"}]), None),
        candidate(0, json!([{"text":"first"}]), Some("MAX_TOKENS")),
        candidate(1, json!([]), Some("FUTURE_REASON")),
    ];
    let mut stream = ContentStream::new(4096, 16384, 2).unwrap();
    stream.push(&frames(&chunks)).unwrap();
    stream.finish().unwrap();
    let response = stream.completed_response().unwrap().response();
    assert_eq!(response.outcome(0), Some(CandidateOutcome::MaxTokens));
    assert_eq!(response.outcome(1), Some(CandidateOutcome::Unknown));
    assert_eq!(
        response.candidates()[1]["content"]["parts"],
        json!([{"text":"second"}])
    );
    for chunks in [
        vec![chunks[0].clone(), chunks[1].clone()],
        vec![chunks[1].clone()],
    ] {
        let mut stream = ContentStream::new(4096, 16384, 2).unwrap();
        stream.push(&frames(&chunks)).unwrap();
        assert!(stream.finish().is_err());
        assert_eq!(stream.state(), NativeStreamState::Truncated);
        assert!(stream.completed_response().is_none());
    }
    let blocked = json!({"promptFeedback":{"blockReason":"SAFETY","future":true},"usageMetadata":{"promptTokenCount":8}});
    let mut stream = ContentStream::new(4096, 16384, 2).unwrap();
    stream
        .push(&frames(std::slice::from_ref(&blocked)))
        .unwrap();
    let metadata = json!({"promptFeedback":{},"usageMetadata":{"totalTokenCount":8}});
    stream
        .push(&frames(std::slice::from_ref(&metadata)))
        .unwrap();
    stream.finish().unwrap();
    assert_eq!(
        stream.completed_response().unwrap().chunks(),
        &[blocked, metadata]
    );
    assert_eq!(
        stream
            .completed_response()
            .unwrap()
            .response()
            .blocked_prompt(),
        Some("SAFETY")
    );
}

#[test]
fn malformed_identity_lifecycle_and_native_errors_fail_without_completed_history() {
    let start = candidate(0, json!([{"text":"x"}]), None);
    let stop = candidate(0, json!([]), Some("STOP"));
    let mut duplicate = start.clone();
    duplicate["candidates"]
        .as_array_mut()
        .unwrap()
        .push(start["candidates"][0].clone());
    let mut wrong_part = start.clone();
    wrong_part["candidates"][0]["content"]["parts"] = json!([{"text":4}]);
    let cases = vec![
        vec![json!([])],
        vec![duplicate],
        vec![wrong_part],
        vec![candidate(1, json!([]), None)],
        vec![json!({"responseId":"a"}), json!({"responseId":"b"})],
        vec![json!({"modelVersion":"a"}), json!({"modelVersion":"b"})],
        vec![stop.clone(), start.clone()],
        vec![stop.clone(), candidate(0, json!([]), Some("MAX_TOKENS"))],
        vec![start, json!({"promptFeedback":{"blockReason":"SAFETY"}})],
        vec![json!({"usageMetadata":{"thoughtsTokenCount":-1}})],
        vec![json!({"error":{"code":429,"message":"SECRET_ERROR_TEXT"}})],
        vec![
            json!({"promptFeedback":{"blockReason":"SAFETY"}}),
            json!({"promptFeedback":{}}),
            stop.clone(),
        ],
        vec![
            json!({"promptFeedback":{"blockReason":"SAFETY"}}),
            json!({"promptFeedback":{"blockReason":null}}),
            stop.clone(),
        ],
        vec![
            json!({"promptFeedback":{"blockReason":"SAFETY"}}),
            json!({"promptFeedback":{"blockReason":"BLOCK_REASON_UNSPECIFIED"}}),
            stop,
        ],
    ];
    for chunks in cases {
        let mut stream = ContentStream::new(4096, 16384, 1).unwrap();
        let error = stream.push(&frames(&chunks)).err().unwrap();
        assert!(matches!(error.http_status, 429 | 502));
        assert!(!format!("{error:?}").contains("SECRET_ERROR_TEXT"));
        assert!(stream.completed_response().is_none());
        assert!(stream.finish().is_err());
    }
    for bytes in [
        b"data: [DONE]\n\n".as_slice(),
        b"event: interaction.complete\ndata: {}\n\n",
        b"data: \xff\n\n",
    ] {
        let mut stream = ContentStream::new(4096, 16384, 1).unwrap();
        assert!(stream.push(bytes).is_err());
        assert!(stream.completed_response().is_none());
    }
}

#[test]
fn eof_partial_frames_cancellation_and_limits_never_synthesize_completion() {
    let stop = frames(&[candidate(0, json!([]), Some("STOP"))]);
    for tail in [b"data: {".as_slice(), b"data: {}\n", b"event: message\n"] {
        let mut stream = ContentStream::new(4096, 16384, 1).unwrap();
        stream.push(&stop).unwrap();
        stream.push(tail).unwrap();
        assert!(stream.finish().is_err());
        assert!(stream.completed_response().is_none());
    }
    for (frame, total) in [(8, 16384), (4096, 8)] {
        let mut stream = ContentStream::new(frame, total, 1).unwrap();
        assert!(stream.push(&stop).is_err());
        assert_eq!(stream.state(), NativeStreamState::Invalid);
    }
    for args in [(0, 1, 1), (1, 0, 1), (1, 1, 0)] {
        assert!(ContentStream::new(args.0, args.1, args.2).is_err());
    }
    let mut stream = ContentStream::new(4096, 16384, 1).unwrap();
    stream.push(&stop).unwrap();
    stream.cancel();
    assert_eq!(stream.state(), NativeStreamState::Cancelled);
    assert!(stream.finish().is_err());
    assert!(stream.push(&stop).is_err());
    assert!(stream.completed_response().is_none());
}
