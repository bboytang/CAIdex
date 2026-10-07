use super::*;
use caidex_model_core::{
    CanonicalRequest, ContextHeaders, ModelProvider, REQUEST_HEADERS, ResponsesDialect,
};
use caidex_provider_anthropic::AnthropicProvider;

pub(super) fn local_context() -> RequestContext {
    let mut headers = ContextHeaders::default();
    for name in ["session_id", "x-client-request-id", "x-codex-turn-metadata"] {
        headers
            .insert(name, format!("LOCAL_ONLY_{name}"), REQUEST_HEADERS)
            .unwrap();
    }
    RequestContext {
        headers,
        ..Default::default()
    }
}

#[tokio::test]
async fn local_runtime_context_stays_local_and_native_request_id_reaches_provider_json() {
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let (base, mut requests, _, task) = fixture_with_headers(
            vec![(200, page("native", false).to_string()), (200, reply().to_string())], false,
            "request-id: req_native\r\nx-request-id: wrong-name\r\nx-codex-turn-state: never-native\r\n",
        ).await;
        let (client, reads) = client_with_context(&base, Some(KEY), Limits::default(), true);
        let provider =
            AnthropicProvider::new(client, vec![super::provider::profile()], 10).unwrap();
        assert_eq!(
            provider.discover_models(local_context()).await.unwrap()[0].id(),
            "native"
        );
        let response = provider
            .create_response(
                CanonicalRequest::new(
                    json!({"model":"alias","input":[{"role":"user","content":"hello"}]}),
                    dialect,
                )
                .unwrap(),
                local_context(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers.get("x-request-id"), Some("req_native"));
        assert!(response.headers.get("x-codex-turn-state").is_none());
        assert!(!format!("{:?}", response.headers).contains("req_native"));
        assert_eq!(response.response.wire()["status"], "completed");
        for _ in 0..2 {
            let (head, body) = received(&mut requests).await;
            for name in REQUEST_HEADERS {
                assert!(!head.to_ascii_lowercase().contains(name));
            }
            assert!(!head.contains("LOCAL_ONLY"));
            assert!(!String::from_utf8(body).unwrap().contains("LOCAL_ONLY"));
        }
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        task.await.unwrap();
    }
}

#[tokio::test]
async fn native_context_opt_in_and_turn_state_rejection_happen_before_broker_reads() {
    for enabled in [false, true] {
        let (client, reads) = client_with_context(
            "http://127.0.0.1:1/v1",
            Some(KEY),
            Limits::default(),
            enabled,
        );
        let mut context = local_context();
        if enabled {
            context
                .headers
                .insert(
                    "x-codex-turn-state",
                    "private-routing".into(),
                    REQUEST_HEADERS,
                )
                .unwrap();
        }
        assert_eq!(
            client
                .create_message("native", request(), context)
                .await
                .unwrap_err()
                .code,
            "unsupported_native_context_header"
        );
        let mut wrong_direction = RequestContext::default();
        wrong_direction
            .headers
            .insert(
                "x-request-id",
                "not-request-context".into(),
                caidex_model_core::RESPONSE_HEADERS,
            )
            .unwrap();
        assert_eq!(
            client
                .discover_models(10, wrong_direction)
                .await
                .unwrap_err()
                .code,
            "unsupported_native_context_header"
        );
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }
    assert!(!format!("{:?}", local_context()).contains("LOCAL_ONLY"));
}

#[tokio::test]
async fn native_json_request_id_is_optional_but_invalid_values_fail_safely() {
    for headers in [
        "request-id: \r\n".into(),
        "request-id: req_one\r\nrequest-id: req_two\r\n".into(),
        format!("request-id: {}\r\n", "a".repeat(8193)),
        "request-id: 非ASCII\r\n".into(),
        "x-request-id: spoof\r\nx-codex-turn-state: spoof\r\n".into(),
    ] {
        let valid = headers.starts_with("x-request-id:");
        let (base, mut requests, _, task) =
            fixture_with_headers(vec![(200, reply().to_string())], false, &headers).await;
        let (client, _) = client(&base, Some(KEY), Limits::default());
        let result = client
            .create_message_with_headers("native", request(), RequestContext::default())
            .await;
        if valid {
            assert!(result.unwrap().1.iter().next().is_none());
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.http_status, 502);
            assert_eq!(error.code, "provider_invalid_context_header");
            assert!(!format!("{error:?}").contains("req_one"));
        }
        received(&mut requests).await;
        task.await.unwrap();
    }
}
