use super::*;
use caidex_provider_anthropic::NativeStreamEvent;
use futures_util::StreamExt;

const ORGANIZATION: &str = "11111111-2222-3333-4444-555555555555";
const OTHER_ORGANIZATION: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
const REPLACEMENT_KEY: &str = "SYNTHETIC_REPLACEMENT_KEY";
struct RotatingStore(Arc<AtomicUsize>);
impl SecretStore for RotatingStore {
    fn get(&self, _: &CredentialRef) -> caidex_credentials::Result<Option<Secret>> {
        let reads = self.0.fetch_add(1, Ordering::SeqCst);
        Secret::new(if reads == 0 { KEY } else { REPLACEMENT_KEY }.into()).map(Some)
    }
    fn set(&self, _: &CredentialRef, _: &Secret) -> caidex_credentials::Result<()> {
        unreachable!()
    }
    fn remove(&self, _: &CredentialRef) -> caidex_credentials::Result<bool> {
        unreachable!()
    }
}
fn organization(id: &str) -> String {
    json!({"type":"organization","id":id,"name":"fixture", "future":{"keep":true}}).to_string()
}
fn scoped_client(base: &str, limits: Limits) -> (AnthropicClient<Store>, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let broker = Arc::new(Broker::new(
        Id::new("executor").unwrap(),
        Store {
            reads: reads.clone(),
            key: Some(KEY),
        },
    ));
    let config = AnthropicConfig::new(reference())
        .unwrap()
        .with_base_url(base)
        .unwrap()
        .with_expected_organization(ORGANIZATION)
        .unwrap();
    (AnthropicClient::new(config, broker, limits).unwrap(), reads)
}
fn native_stream() -> String {
    let events = [
        json!({"type":"message_start","message":{"type":"message","role":"assistant","id":"msg-fixture","model":"native","content":[],"usage":{"input_tokens":2,"output_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}),
        json!({"type":"message_stop"}),
    ];
    events
        .into_iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect()
}

#[tokio::test]
async fn organization_is_authenticated_not_cached_by_credential_reference() {
    let headers =
        format!("anthropic-organization-id: {ORGANIZATION}\r\nrequest-id: req-inference\r\n");
    let (base, mut requests, _, task) = fixture_with_responses(
        vec![
            (
                200,
                organization(ORGANIZATION),
                "application/json".into(),
                String::new(),
            ),
            (200, reply().to_string(), "application/json".into(), headers),
            (
                200,
                organization(OTHER_ORGANIZATION),
                "application/json".into(),
                String::new(),
            ),
        ],
        false,
    )
    .await;
    let reads = Arc::new(AtomicUsize::new(0));
    let broker = Arc::new(Broker::new(
        Id::new("executor").unwrap(),
        RotatingStore(reads.clone()),
    ));
    let config = AnthropicConfig::new(reference())
        .unwrap()
        .with_base_url(&base)
        .unwrap()
        .with_expected_organization(ORGANIZATION)
        .unwrap();
    assert!(!format!("{config:?}").contains(ORGANIZATION));
    let client = AnthropicClient::new(config, broker, Limits::default()).unwrap();
    let (native, headers) = client
        .create_message_with_headers("native", request(), RequestContext::default())
        .await
        .unwrap();
    assert_eq!(native.id(), "msg-fixture");
    assert_eq!(
        headers.iter().collect::<Vec<_>>(),
        [("x-request-id", "req-inference")]
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    for path in [
        "GET /proxy/v1/organizations/me ",
        "POST /proxy/v1/messages ",
    ] {
        let (head, body) = received(&mut requests).await;
        assert!(head.starts_with(path));
        assert!(
            head.to_ascii_lowercase()
                .contains("x-api-key: synthetic_anthropic_key")
        );
        assert!(
            !head
                .to_ascii_lowercase()
                .contains("anthropic-organization-id:")
        );
        if path.starts_with("POST") {
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap()["messages"],
                request()["messages"]
            );
        }
    }
    let error = client
        .create_message("native", request(), RequestContext::default())
        .await
        .unwrap_err();
    assert_eq!(
        (error.http_status, error.code),
        (400, "anthropic_organization_mismatch")
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    let (head, _) = received(&mut requests).await;
    assert!(head.starts_with("GET /proxy/v1/organizations/me "));
    assert!(
        head.to_ascii_lowercase()
            .contains("x-api-key: synthetic_replacement_key")
    );
    task.await.unwrap();
    assert!(requests.recv().await.is_none());
}

#[tokio::test]
async fn organization_json_and_sse_headers_are_required_before_delivery() {
    for stream in [false, true] {
        for (headers, expected) in [
            (
                format!("anthropic-organization-id: {ORGANIZATION}\r\n"),
                None,
            ),
            (String::new(), Some("anthropic_invalid_organization")),
            (
                format!("anthropic-organization-id: {OTHER_ORGANIZATION}\r\n"),
                Some("anthropic_response_organization_mismatch"),
            ),
            (
                format!(
                    "anthropic-organization-id: {ORGANIZATION}\r\nanthropic-organization-id: {ORGANIZATION}\r\n"
                ),
                Some("anthropic_invalid_organization"),
            ),
            (
                "anthropic-organization-id: \r\n".into(),
                Some("anthropic_invalid_organization"),
            ),
            (
                "anthropic-organization-id: bad scope\r\n".into(),
                Some("anthropic_invalid_organization"),
            ),
            (
                format!("anthropic-organization-id: {}\r\n", "a".repeat(1025)),
                Some("anthropic_invalid_organization"),
            ),
            (
                "anthropic-organization-id: 组织\r\n".into(),
                Some("anthropic_invalid_organization"),
            ),
        ] {
            let body = if stream {
                native_stream()
            } else {
                reply().to_string()
            };
            let media = if stream {
                "text/event-stream"
            } else {
                "application/json"
            };
            let (base, mut requests, _, task) = fixture_with_responses(
                vec![
                    (
                        200,
                        organization(ORGANIZATION),
                        "application/json".into(),
                        String::new(),
                    ),
                    (200, body, media.into(), headers),
                ],
                false,
            )
            .await;
            let (client, reads) = scoped_client(&base, Limits::default());
            let result = if stream {
                match client
                    .stream_message("native", request(), RequestContext::default())
                    .await
                {
                    Ok(mut delivery) => {
                        assert!(delivery.headers().iter().next().is_none());
                        let mut completed = false;
                        while let Some(event) = delivery.next().await {
                            completed |= matches!(event.unwrap(), NativeStreamEvent::Completed(_));
                        }
                        assert!(completed);
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            } else {
                client
                    .create_message("native", request(), RequestContext::default())
                    .await
                    .map(|_| ())
            };
            match expected {
                Some(code) => {
                    let error = result.unwrap_err();
                    assert_eq!((error.http_status, error.code), (502, code));
                }
                None => result.unwrap(),
            }
            assert_eq!(reads.load(Ordering::SeqCst), 1);
            assert!(
                received(&mut requests)
                    .await
                    .0
                    .starts_with("GET /proxy/v1/organizations/me ")
            );
            assert!(
                received(&mut requests)
                    .await
                    .0
                    .starts_with("POST /proxy/v1/messages ")
            );
            task.await.unwrap();
            assert!(requests.recv().await.is_none());
        }
    }
}

#[tokio::test]
async fn organization_bad_identity_or_http_failure_never_posts_or_retries() {
    for (status, body, headers, code) in [
        (
            200,
            json!({"type":"organization","id":ORGANIZATION}).to_string(),
            String::new(),
            "anthropic_invalid_organization",
        ),
        (
            200,
            organization("bad scope"),
            String::new(),
            "anthropic_invalid_organization",
        ),
        (
            200,
            organization(ORGANIZATION),
            format!("anthropic-organization-id: {OTHER_ORGANIZATION}\r\n"),
            "anthropic_invalid_organization",
        ),
        (
            403,
            format!("{{\"error\":\"{KEY}\"}}"),
            String::new(),
            "provider_authentication_failed",
        ),
        (
            429,
            String::from("private diagnostic"),
            String::new(),
            "provider_rate_limited",
        ),
        (
            302,
            String::new(),
            "location: https://example.com/private\r\n".into(),
            "provider_redirect_blocked",
        ),
    ] {
        let (base, mut requests, _, task) = fixture_with_responses(
            vec![(status, body, "application/json".into(), headers)],
            false,
        )
        .await;
        let (client, reads) = scoped_client(&base, Limits::default());
        let error = client
            .create_message("native", request(), RequestContext::default())
            .await
            .unwrap_err();
        assert_eq!(error.code, code);
        assert!(!format!("{error:?}").contains(KEY));
        if status == 429 {
            assert_eq!(error.retry_after_seconds, Some(7));
        }
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert!(
            received(&mut requests)
                .await
                .0
                .starts_with("GET /proxy/v1/organizations/me ")
        );
        task.await.unwrap();
        assert!(requests.recv().await.is_none());
    }
}

#[tokio::test]
async fn organization_lookup_obeys_cancellation_deadline_and_byte_budget() {
    let (base, mut requests, _, task) =
        fixture(vec![(200, organization(ORGANIZATION))], false).await;
    let (unscoped, reads) = client(&base, Some(KEY), Limits::default());
    assert_eq!(
        unscoped
            .current_organization(RequestContext::default())
            .await
            .unwrap(),
        ORGANIZATION
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert!(
        received(&mut requests)
            .await
            .0
            .starts_with("GET /proxy/v1/organizations/me ")
    );
    task.await.unwrap();

    let scope = format!("anthropic-organization-id: {ORGANIZATION}\r\n");
    let (base, mut requests, _, task) = fixture_with_responses(
        vec![
            (
                200,
                organization(ORGANIZATION),
                "application/json".into(),
                String::new(),
            ),
            (
                200,
                page("first", true).to_string(),
                "application/json".into(),
                scope.clone(),
            ),
            (
                200,
                organization(ORGANIZATION),
                "application/json".into(),
                String::new(),
            ),
            (
                200,
                page("last", false).to_string(),
                "application/json".into(),
                scope,
            ),
        ],
        false,
    )
    .await;
    let (scoped, reads) = scoped_client(&base, Limits::default());
    assert_eq!(
        scoped
            .discover_models(2, RequestContext::default())
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    for path in [
        "GET /proxy/v1/organizations/me ",
        "GET /proxy/v1/models?limit=1000 ",
        "GET /proxy/v1/organizations/me ",
        "GET /proxy/v1/models?limit=1000&after_id=first ",
    ] {
        assert!(received(&mut requests).await.0.starts_with(path));
    }
    task.await.unwrap();

    let (base, mut requests, mut closed, task) =
        fixture(vec![(200, organization(ORGANIZATION))], true).await;
    let (client, reads) = scoped_client(
        &base,
        Limits {
            in_flight: 1,
            ..Limits::default()
        },
    );
    let cancellation = CancellationToken::new();
    let context = RequestContext {
        cancellation: cancellation.clone(),
        ..Default::default()
    };
    let pending = client.create_message("native", request(), context);
    tokio::pin!(pending);
    tokio::select! {
        result = &mut pending => panic!("preflight unexpectedly finished: {result:?}"),
        head = received(&mut requests) => assert!(head.0.starts_with("GET /proxy/v1/organizations/me ")),
    }
    cancellation.cancel();
    assert_eq!(pending.await.unwrap_err().code, "provider_cancelled");
    received(&mut closed).await;
    task.await.unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_ne!(
        client
            .current_organization(RequestContext::default())
            .await
            .unwrap_err()
            .code,
        "provider_busy"
    );

    for limit in [None, Some(16)] {
        let (base, mut requests, mut closed, task) =
            fixture(vec![(200, organization(ORGANIZATION))], limit.is_none()).await;
        let limits = Limits {
            response_bytes: limit.unwrap_or(Limits::default().response_bytes),
            ..Limits::default()
        };
        let (client, _) = scoped_client(&base, limits);
        let context = RequestContext {
            deadline: Some(std::time::Instant::now() + Duration::from_millis(200)),
            ..Default::default()
        };
        let error = client.current_organization(context).await.unwrap_err();
        assert_eq!(
            error.code,
            if limit.is_none() {
                "provider_timeout"
            } else {
                "provider_oversized_response"
            }
        );
        assert!(
            received(&mut requests)
                .await
                .0
                .starts_with("GET /proxy/v1/organizations/me ")
        );
        if limit.is_none() {
            received(&mut closed).await;
        }
        task.await.unwrap();
    }
    let (base, mut requests, mut closed, task) = fixture(vec![(200, String::new())], true).await;
    let (client, _) = scoped_client(
        &base,
        Limits {
            header_timeout: Duration::from_millis(200),
            ..Limits::default()
        },
    );
    let error = tokio::time::timeout(
        Duration::from_secs(3),
        client.current_organization(RequestContext::default()),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(error.code, "provider_timeout");
    assert!(
        received(&mut requests)
            .await
            .0
            .starts_with("GET /proxy/v1/organizations/me ")
    );
    received(&mut closed).await;
    task.await.unwrap();
}
