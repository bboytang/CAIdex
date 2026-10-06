use caidex_model_core::{Error, ResponsesStream, SseDecoder, StreamState};
use serde_json::{Value, json};

fn frame(value: &Value) -> String {
    format!("data: {value}\n\n")
}
fn completed(id: &str) -> Value {
    json!({"type":"response.completed", "response":{"id":id,"status":"completed","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}})
}

#[test]
fn sse_framing_is_independent_of_every_byte_split_utf8_and_line_ending() {
    for ending in ["\n", "\r\n", "\r"] {
        let wire = format!(
            "\u{feff}: comment{ending}id: frame-1{ending}event: response.output_text.delta{ending}retry: 27{ending}data: {{\"type\":\"response.output_text.delta\",{ending}data: \"delta\":\"你好 🙂\"}}{ending}{ending}"
        );
        let mut whole = SseDecoder::new(4096).unwrap();
        let expected = whole.push(wire.as_bytes()).unwrap();
        assert_eq!(expected.len(), 1);
        assert_eq!(expected[0].id, "frame-1");
        assert_eq!(expected[0].retry_ms, Some(27));
        assert_eq!(
            serde_json::from_str::<Value>(&expected[0].data).unwrap()["delta"],
            "你好 🙂"
        );
        for split in 0..=wire.len() {
            let mut decoder = SseDecoder::new(4096).unwrap();
            let mut actual = decoder.push(&wire.as_bytes()[..split]).unwrap();
            actual.extend(decoder.push(&wire.as_bytes()[split..]).unwrap());
            assert!(actual == expected, "split {split}, line ending {ending:?}");
        }
        let mut decoder = SseDecoder::new(4096).unwrap();
        let mut actual = Vec::new();
        for byte in wire.as_bytes() {
            actual.extend(decoder.push(&[*byte]).unwrap());
        }
        assert!(actual == expected);
    }
}

#[test]
fn sse_ids_retry_comments_empty_data_and_field_case_follow_framing_rules() {
    let wire = b"id: original\nretry: 9\nevent: unused\n\ndata: first\n\nid: ignored\0value\nretry: +10\nData: ignored\ndata:  second\n\nid\nretry: 9999999999999999999999999999\ndata:\n\n: heartbeat\n\n";
    let mut decoder = SseDecoder::new(4096).unwrap();
    let events = decoder.push(wire).unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].event, "message");
    assert_eq!(events[0].id, "original");
    assert_eq!(events[1].id, "original");
    assert_eq!(events[1].data, " second");
    assert_eq!(events[2].id, "");
    assert_eq!(events[2].data, "");
    assert!(events.iter().all(|event| event.retry_ms == Some(9)));
    assert!(!format!("{:?}", events[1]).contains("second"));
}

#[test]
fn sse_limits_include_multiline_frames_and_invalid_utf8_closes_decoder() {
    assert!(matches!(SseDecoder::new(0), Err(Error::InvalidLimit)));
    let mut decoder = SseDecoder::new(20).unwrap();
    assert!(decoder.push(b"data: a\ndata: b\n").unwrap().is_empty());
    assert!(matches!(
        decoder.push(b"data: c\n\n"),
        Err(Error::FrameTooLarge)
    ));
    assert!(matches!(decoder.push(b"\n"), Err(Error::StreamClosed)));
    let mut decoder = SseDecoder::new(20).unwrap();
    assert!(matches!(
        decoder.push(b"data: \xff\n\n"),
        Err(Error::InvalidUtf8)
    ));
    assert!(matches!(
        decoder.push(b"data: ok\n\n"),
        Err(Error::StreamClosed)
    ));
    let mut decoder = SseDecoder::new(20).unwrap();
    assert!(decoder.push(b"data: unfinished\n").unwrap().is_empty());
    decoder.finish();
    assert!(matches!(decoder.push(b"\n"), Err(Error::StreamClosed)));
}

#[test]
fn response_stream_preserves_all_events_and_requires_the_terminal_frame() {
    let source = [
        json!({"type":"response.created","sequence_number":0,"response":{"id":"resp_fixture"}}),
        json!({"type":"response.output_text.delta","sequence_number":1,"delta":"你好"}),
        json!({"type":"provider.future","sequence_number":8,"opaque":{"signature":"fixture=="}}),
        completed("resp_fixture"),
    ];
    let wire: String = source.iter().map(frame).collect();
    for wire in [
        wire.clone(),
        wire.replace('\n', "\r\n"),
        wire.replace('\n', "\r"),
    ] {
        for split in 0..=wire.len() {
            let mut stream = ResponsesStream::new(4096).unwrap();
            let mut events = stream.push(&wire.as_bytes()[..split]).unwrap();
            events.extend(stream.push(&wire.as_bytes()[split..]).unwrap());
            assert_eq!(events.len(), source.len());
            for (event, raw) in events.iter().zip(source.iter()) {
                assert_eq!(event.response.wire(), raw);
            }
            assert_eq!(events[1].response.text_delta(), Some("你好"));
            assert_eq!(stream.finish().unwrap(), StreamState::Completed);
        }
    }
}

#[test]
fn response_terminal_states_keep_interruption_incomplete_and_errors_distinct() {
    for (kind, reason, state) in [
        (
            "response.incomplete",
            "interrupted",
            StreamState::Interrupted,
        ),
        (
            "response.incomplete",
            "max_output_tokens",
            StreamState::Incomplete,
        ),
        (
            "response.failed",
            "rate_limit_exceeded",
            StreamState::Failed,
        ),
        ("error", "server_error", StreamState::Failed),
    ] {
        let mut stream = ResponsesStream::new(4096).unwrap();
        let value = json!({"type":kind,"response":{"id":"fixture","incomplete_details":{"reason":reason},"error":{"code":reason,"message":"fixture-private-error"}},"error":{"code":reason}});
        assert_eq!(
            stream.push(frame(&value).as_bytes()).unwrap()[0]
                .response
                .wire(),
            &value
        );
        assert_eq!(stream.state(), state);
        assert_eq!(stream.finish().unwrap(), state);
        assert!(matches!(
            stream.push(b"data: {}\n\n"),
            Err(Error::StreamClosed)
        ));
        assert_eq!(stream.state(), state);
    }
}

#[test]
fn eof_unterminated_completion_and_done_marker_never_count_as_success() {
    for wire in [
        String::new(),
        frame(&json!({"type":"response.output_text.delta","delta":"partial"})),
        format!("data: {}\n", completed("fixture")),
    ] {
        let mut stream = ResponsesStream::new(4096).unwrap();
        stream.push(wire.as_bytes()).unwrap();
        assert_eq!(stream.finish(), Err(Error::UnexpectedEnd));
        assert_eq!(stream.state(), StreamState::Truncated);
    }
    let mut stream = ResponsesStream::new(4096).unwrap();
    assert!(matches!(
        stream.push(b"data: [DONE]\n\n"),
        Err(Error::InvalidEvent)
    ));
    assert_eq!(stream.state(), StreamState::Invalid);
}

#[test]
fn sequences_and_response_identity_prevent_mixing_or_replaying_events() {
    for (first, second, error) in [
        (
            json!({"type":"response.created","sequence_number":3,"response":{"id":"one"}}),
            json!({"type":"provider.future","sequence_number":3}),
            Error::InvalidSequence,
        ),
        (
            json!({"type":"provider.future","sequence_number":3}),
            json!({"type":"provider.future","sequence_number":2}),
            Error::InvalidSequence,
        ),
        (
            json!({"type":"response.created","response":{"id":"one"}}),
            completed("two"),
            Error::ResponseMismatch,
        ),
    ] {
        let mut stream = ResponsesStream::new(4096).unwrap();
        stream.push(frame(&first).as_bytes()).unwrap();
        assert!(matches!(stream.push(frame(&second).as_bytes()), Err(actual) if actual == error));
        assert_eq!(stream.state(), StreamState::Invalid);
    }
    for invalid in [b"data: {not-json}\n\n".as_slice(), b"event: response.failed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"fixture\"}}\n\n", b"data: {\"type\":\"provider.future\",\"sequence_number\":-1}\n\n"] {
        let mut stream = ResponsesStream::new(4096).unwrap();
        assert!(stream.push(invalid).is_err());
        assert_eq!(stream.state(), StreamState::Invalid);
    }
}

#[test]
fn cancellation_and_post_terminal_frames_cannot_restart_a_stream() {
    let mut stream = ResponsesStream::new(4096).unwrap();
    stream
        .push(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n")
        .unwrap();
    stream.cancel();
    assert_eq!(stream.finish().unwrap(), StreamState::Cancelled);
    assert!(matches!(
        stream.push(frame(&completed("fixture")).as_bytes()),
        Err(Error::StreamClosed)
    ));
    let mut stream = ResponsesStream::new(4096).unwrap();
    let wire = frame(&completed("fixture")) + &frame(&json!({"type":"provider.future"}));
    assert!(matches!(
        stream.push(wire.as_bytes()),
        Err(Error::StreamClosed)
    ));
    assert_eq!(stream.state(), StreamState::Invalid);
    let mut stream = ResponsesStream::new(4096).unwrap();
    stream
        .push(frame(&completed("fixture")).as_bytes())
        .unwrap();
    stream.cancel();
    assert_eq!(stream.finish().unwrap(), StreamState::Completed);
    assert!(matches!(stream.push(b""), Err(Error::StreamClosed)));
    assert_eq!(stream.state(), StreamState::Completed);
}
