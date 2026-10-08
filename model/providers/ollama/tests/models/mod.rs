use super::*;
use caidex_model_core::{CapabilitySupport, ModelCapabilities};
use caidex_provider_ollama::ModelDetails;

fn details() -> Value {
    let mut wire = json!({"capabilities":["completion","tools","thinking","vision","future_capability"],
        "thinking":{"values":[false,"low","high"],"default":"low","future":"retain"},
        "model_info":{"general.architecture":"fixture","fixture.context_length":131072,
            "future_big":18446744073709551616_u128},
        "parameters":"num_ctx 2048", "template":"private template", "remote_host":"https://untrusted.invalid/ignored"});
    wire["future"] = serde_json::from_str(r#"{"number":0.12345678901234567890123456789}"#).unwrap();
    wire
}
fn config(fixture: &Fixture, authenticated: bool) -> OllamaConfig {
    OllamaConfig::new(&fixture.base, authenticated.then(reference))
        .unwrap()
        .with_show_endpoint(&format!("{}/show", fixture.base.replace("/v1", "/api")))
        .unwrap()
}

#[test]
fn native_details_validate_controls_preserve_unknown_metadata_and_keep_unknowns_unknown() {
    let original = details();
    let parsed = ModelDetails::parse("native-fixture".into(), original.clone()).unwrap();
    assert_eq!(parsed.wire(), &original);
    assert_eq!(
        parsed.wire()["future"]["number"].to_string(),
        "0.12345678901234567890123456789"
    );
    assert_eq!(parsed.native_model(), "native-fixture");
    assert_eq!(
        parsed.thinking_values(),
        Some([json!(false), json!("low"), json!("high")].as_slice())
    );
    assert_eq!(parsed.thinking_default(), Some(&json!("low")));
    assert_eq!(
        parsed.declared_capabilities().text,
        CapabilitySupport::Supported
    );
    assert_eq!(
        parsed.declared_capabilities().vision,
        CapabilitySupport::Supported
    );
    assert_eq!(
        parsed.declared_capabilities().reasoning,
        CapabilitySupport::Supported
    );
    assert_eq!(
        parsed.declared_capabilities().native_tools,
        CapabilitySupport::Supported
    );
    assert_eq!(parsed.declared_capabilities().context_window, None);
    assert_eq!(
        parsed.declared_capabilities().parallel_tools,
        CapabilitySupport::Unknown
    );
    assert!(!format!("{parsed:?}").contains("private template"));
    let unknown = ModelDetails::parse("native-fixture".into(), json!({"model_info":{}})).unwrap();
    assert_eq!(
        unknown.declared_capabilities(),
        &ModelCapabilities::default()
    );
    assert!(unknown.thinking_values().is_none());
    let remote = ModelDetails::parse(
        "native-fixture".into(),
        json!({"model_info":null,"remote_host":"https://untrusted.invalid"}),
    )
    .unwrap();
    assert_eq!(
        remote.declared_capabilities(),
        &ModelCapabilities::default()
    );
    let disabled = ModelDetails::parse(
        "native-fixture".into(),
        json!({"thinking":{"values":[false],"default":false}}),
    )
    .unwrap();
    assert_eq!(
        disabled.declared_capabilities().reasoning,
        CapabilitySupport::Unsupported
    );
    for bad in [
        json!({"thinking":{"values":[],"default":false}}),
        json!({"thinking":{"values":[true,true],"default":true}}),
        json!({"thinking":{"values":[true],"default":false}}),
        json!({"thinking":{"values":[1],"default":1}}),
        json!({"thinking":{"values":[""],"default":""}}),
        json!({"thinking":{"values":["low"]}}),
        json!({"capabilities":["tools","tools"]}),
        json!({"capabilities":null}),
        json!({"model_info":[]}),
        json!({"error":KEY}),
    ] {
        assert_eq!(
            ModelDetails::parse("native-fixture".into(), bad)
                .err()
                .unwrap()
                .code,
            "ollama_invalid_model_details"
        );
    }
}

#[tokio::test]
async fn show_posts_native_model_on_fixed_same_origin_and_installs_only_bound_catalog_evidence() {
    for authenticated in [false, true] {
        let mut fixture =
            Fixture::start(vec![Reply::json(details()), Reply::json(catalog())]).await;
        let (broker, reads) = broker();
        let provider = OllamaProvider::new(
            config(&fixture, authenticated),
            vec![model("fixture", "native-fixture")],
            broker,
            limits(),
        )
        .unwrap();
        let native = provider
            .show_model("fixture", RequestContext::default())
            .await
            .unwrap();
        assert_eq!(native.wire(), &details());
        let captured = fixture.request().await;
        assert!(captured.headers.starts_with("POST /proxy/api/show "));
        assert_eq!(captured.header("content-type"), Some("application/json"));
        assert_eq!(captured.header("accept"), Some("application/json"));
        assert_eq!(captured.body, Some(json!({"model":"native-fixture"})));
        assert_eq!(
            captured.header("authorization"),
            authenticated.then_some(format!("Bearer {KEY}")).as_deref()
        );
        assert!(
            captured
                .header("x-openai-internal-codex-responses-lite")
                .is_none()
        );
        let provider = provider
            .with_model_details(vec![("fixture".into(), native)])
            .unwrap();
        assert_eq!(
            provider.capabilities("fixture").unwrap().reasoning,
            CapabilitySupport::Supported
        );
        assert_eq!(
            provider.metadata("fixture").unwrap().source,
            EvidenceSource::ProviderCatalog
        );
        assert!(
            provider
                .metadata("fixture")
                .unwrap()
                .codex_compatibility
                .is_none()
        );
        assert_eq!(
            reads.load(Ordering::SeqCst),
            if authenticated { 1 } else { 0 }
        );
        assert_eq!(
            provider.list_models().await.unwrap()[0]
                .capabilities
                .reasoning,
            CapabilitySupport::Supported
        );
        assert!(
            fixture
                .request()
                .await
                .headers
                .starts_with("GET /proxy/v1/models ")
        );
    }
    for endpoint in [
        "https://other.invalid/api/show",
        "http://127.0.0.1:12345/api/show",
        "https://example.invalid/api/show?key=secret",
    ] {
        assert!(
            OllamaConfig::new("https://example.invalid/v1", Some(reference()))
                .unwrap()
                .with_show_endpoint(endpoint)
                .is_err()
        );
    }
    let fixture = Fixture::start(vec![Reply::json(details())]).await;
    let (broker, reads) = broker();
    let provider = fixture.provider(broker, true);
    assert_eq!(
        provider
            .show_model("fixture", RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "ollama_show_endpoint_not_configured"
    );
    let foreign = ModelDetails::parse("another-model".into(), details()).unwrap();
    assert_eq!(
        provider
            .with_model_details(vec![("fixture".into(), foreign)])
            .err()
            .unwrap()
            .code,
        "ollama_invalid_model_binding"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn advertised_thinking_controls_compile_exactly_without_native_effort_fallback() {
    for streaming in [false, true] {
        let reply = if streaming {
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
            .with_model_details(vec![(
                "fixture".into(),
                ModelDetails::parse("native-fixture".into(), details()).unwrap(),
            )])
            .unwrap();
        for (extra, expected) in [
            (json!({"reasoning":{"effort":"high"}}), json!("high")),
            (json!({"reasoning":{"effort":"none"}}), json!(false)),
            (json!({"think":"low"}), json!("low")),
            (json!({"think":null}), Value::Null),
        ] {
            let mut wire = json!({"model":"fixture","input":"hello","stream":streaming});
            wire.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            if streaming {
                let mut events = provider
                    .stream_response(request(wire), RequestContext::default())
                    .await
                    .unwrap()
                    .events;
                let mut terminal = false;
                while let Some(event) = events.next().await {
                    if let ProviderStreamEvent::Model(event) = event.unwrap() {
                        terminal |= event.response.terminal() == Some(StreamState::Completed);
                    }
                }
                assert!(terminal);
            } else {
                provider
                    .create_response(request(wire), RequestContext::default())
                    .await
                    .unwrap();
            }
            let captured = fixture.request().await.body.unwrap();
            assert_eq!(captured["think"], expected);
            assert!(captured.get("reasoning").is_none());
        }
        assert_eq!(reads.load(Ordering::SeqCst), 4);
        for extra in [
            json!({"reasoning":{"effort":"medium"}}),
            json!({"reasoning":{"effort":"minimal"}}),
            json!({"reasoning":{"effort":"xhigh"}}),
            json!({"reasoning":{"effort":"high","summary":"auto"}}),
            json!({"think":true}),
            json!({"think":123}),
            json!({"think":" HIGH "}),
            json!({"think":"high","reasoning":{"effort":"low"}}),
        ] {
            let mut wire = json!({"model":"fixture","input":"hello","stream":streaming});
            wire.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let error = if streaming {
                provider
                    .stream_response(request(wire), RequestContext::default())
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
            assert_eq!(error.http_status, 400);
        }
        assert_eq!(reads.load(Ordering::SeqCst), 4);
        assert_eq!(fixture.accepted.load(Ordering::SeqCst), 4);
    }
}

#[tokio::test]
async fn boolean_unknown_and_restricted_declarations_do_not_invent_effort_or_loosen_policy() {
    let mut fixture = Fixture::start(vec![Reply::json(response_wire())]).await;
    let (broker, reads) = broker();
    let provider = fixture
        .provider(broker.clone(), true)
        .with_model_details(vec![(
            "fixture".into(),
            ModelDetails::parse(
                "native-fixture".into(),
                json!({"thinking":{"values":[false,true],"default":true}}),
            )
            .unwrap(),
        )])
        .unwrap();
    for extra in [
        json!({"think":true}),
        json!({"think":false}),
        json!({"reasoning":{"effort":"none"}}),
        json!({}),
        json!({"reasoning":null}),
        json!({"reasoning":{}}),
        json!({"reasoning":{"effort":null}}),
    ] {
        let mut wire = json!({"model":"fixture","input":"hello"});
        wire.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        provider
            .create_response(request(wire), RequestContext::default())
            .await
            .unwrap();
        let captured = fixture.request().await.body.unwrap();
        let expected = if extra.get("think").is_some() {
            extra.get("think")
        } else if extra["reasoning"]["effort"] == "none" {
            Some(&Value::Bool(false))
        } else {
            None
        };
        assert_eq!(captured.get("think"), expected);
        assert!(captured.get("reasoning").is_none());
    }
    let mut configured = model("fixture", "native-fixture");
    configured.capabilities.reasoning = CapabilitySupport::Unsupported;
    configured.capabilities.vision = CapabilitySupport::Unsupported;
    configured.capabilities.native_tools = CapabilitySupport::Unsupported;
    let restricted = OllamaProvider::new(
        config(&fixture, true),
        vec![configured],
        broker.clone(),
        limits(),
    )
    .unwrap()
    .with_model_details(vec![(
        "fixture".into(),
        ModelDetails::parse("native-fixture".into(), details()).unwrap(),
    )])
    .unwrap();
    let caps = restricted.capabilities("fixture").unwrap();
    assert_eq!(caps.reasoning, CapabilitySupport::Unsupported);
    assert_eq!(caps.vision, CapabilitySupport::Unsupported);
    assert_eq!(caps.native_tools, CapabilitySupport::Unsupported);
    let unknown = fixture.provider(broker.clone(), true);
    let disabled = fixture
        .provider(broker.clone(), true)
        .with_model_details(vec![(
            "fixture".into(),
            ModelDetails::parse(
                "native-fixture".into(),
                json!({"thinking":{"values":[false],"default":false}}),
            )
            .unwrap(),
        )])
        .unwrap();
    for (provider, extra) in [
        (&provider, json!({"reasoning":{"effort":"high"}})),
        (&provider, json!({"reasoning":{"effort":"medium"}})),
        (&provider, json!({"reasoning":{"effort":true}})),
        (&provider, json!({"reasoning":[]})),
        (&unknown, json!({"think":true})),
        (&unknown, json!({"reasoning":{"effort":"high"}})),
        (&restricted, json!({"think":"high"})),
        (&disabled, json!({"think":true})),
    ] {
        let mut wire = json!({"model":"fixture","input":"hello"});
        wire.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert_eq!(
            provider
                .create_response(request(wire), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            400
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 7);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 7);
    // No-thinking models may explicitly disable; a policy restriction likewise
    // cannot be bypassed to enable thinking, but does not forbid disabling it.
    for provider in [&restricted, &disabled] {
        provider
            .create_response(
                request(json!({"model":"fixture","input":"hello","think":false})),
                RequestContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(fixture.request().await.body.unwrap()["think"], false);
    }
    let snapshot = ModelDetails::parse("native-fixture".into(), details()).unwrap();
    assert_eq!(
        fixture
            .provider(broker, true)
            .with_model_details(vec![
                ("fixture".into(), snapshot.clone()),
                ("fixture".into(), snapshot)
            ])
            .err()
            .unwrap()
            .code,
        "ollama_invalid_model_binding"
    );
}

#[tokio::test]
async fn show_bounds_invalid_payload_and_http_errors_share_safe_transport_without_retry() {
    for (reply, limits, expected) in [
        (
            Reply::json(details()),
            Limits {
                request_bytes: 1,
                ..limits()
            },
            "invalid_or_oversized_body",
        ),
        (
            Reply::json(details()),
            Limits {
                response_bytes: 1,
                ..limits()
            },
            "provider_response_too_large",
        ),
        (
            Reply {
                content_type: "text/plain",
                ..Reply::json(details())
            },
            limits(),
            "provider_invalid_content_type",
        ),
        (
            Reply::json(json!({"thinking":{"values":[true],"default":false}})),
            limits(),
            "ollama_invalid_model_details",
        ),
        (
            Reply::json(json!({"error":KEY})),
            limits(),
            "ollama_invalid_model_details",
        ),
        (
            Reply {
                body: b"{".to_vec(),
                ..Reply::json(Value::Null)
            },
            limits(),
            "provider_invalid_response",
        ),
        (
            Reply {
                status: 401,
                ..Reply::json(json!({"error":KEY}))
            },
            limits(),
            "provider_authentication_failed",
        ),
        (
            Reply {
                status: 429,
                headers: "Retry-After: 2\r\n".into(),
                ..Reply::json(json!({"error":KEY}))
            },
            limits(),
            "provider_rate_limited",
        ),
        (
            Reply {
                status: 302,
                headers: "Location: https://untrusted.invalid/redirect\r\n".into(),
                ..Reply::json(Value::Null)
            },
            limits(),
            "provider_redirect_blocked",
        ),
    ] {
        let local = expected == "invalid_or_oversized_body";
        let mut fixture = Fixture::start(vec![reply]).await;
        let (broker, reads) = broker();
        let provider = OllamaProvider::new(
            config(&fixture, true),
            vec![model("fixture", "native-fixture")],
            broker,
            limits,
        )
        .unwrap();
        let error = provider
            .show_model("fixture", RequestContext::default())
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, expected);
        assert!(!format!("{error:?}").contains(KEY));
        if expected == "provider_rate_limited" {
            assert_eq!(error.retry_after_seconds, Some(2));
        }
        if !local {
            fixture.request().await;
        }
        assert_eq!(reads.load(Ordering::SeqCst), usize::from(!local));
        assert_eq!(fixture.accepted.load(Ordering::SeqCst), usize::from(!local));
    }
}

#[tokio::test]
async fn show_cancellation_drop_deadline_and_slot_are_shared_with_inference() {
    let mut fixture = Fixture::start(vec![Reply {
        stall: 1,
        ..Reply::json(details())
    }])
    .await;
    let (broker, reads) = broker();
    let provider = OllamaProvider::new(
        config(&fixture, true),
        vec![model("fixture", "native-fixture")],
        broker,
        limits(),
    )
    .unwrap();
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let mut headers = ContextHeaders::default();
    headers
        .insert("session_id", "private".into(), REQUEST_HEADERS)
        .unwrap();
    for (context, code) in [
        (
            RequestContext {
                cancellation: cancelled,
                ..Default::default()
            },
            "provider_cancelled",
        ),
        (
            RequestContext {
                deadline: Some(std::time::Instant::now()),
                ..Default::default()
            },
            "provider_timeout",
        ),
        (
            RequestContext {
                headers,
                ..Default::default()
            },
            "ollama_unsupported_context_headers",
        ),
    ] {
        assert_eq!(
            provider
                .show_model("fixture", context)
                .await
                .err()
                .unwrap()
                .code,
            code
        );
    }
    assert_eq!(
        provider
            .show_model("absent", RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "unknown_model"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
    let cancellation = CancellationToken::new();
    let mut pending = Box::pin(provider.show_model(
        "fixture",
        RequestContext {
            cancellation: cancellation.clone(),
            ..Default::default()
        },
    ));
    tokio::select! {biased; result=&mut pending=>panic!("unexpected {result:?}"), _=fixture.request()=>{}}
    assert_eq!(
        provider
            .show_model("fixture", RequestContext::default())
            .await
            .err()
            .unwrap()
            .code,
        "provider_busy"
    );
    assert_eq!(
        provider
            .create_response(
                request(json!({"model":"fixture","input":"hello"})),
                RequestContext::default()
            )
            .await
            .err()
            .unwrap()
            .code,
        "provider_busy"
    );
    cancellation.cancel();
    assert_eq!(pending.await.err().unwrap().code, "provider_cancelled");
    fixture.disconnected().await;
    let mut pending = Box::pin(provider.show_model("fixture", RequestContext::default()));
    tokio::select! {biased; result=&mut pending=>panic!("unexpected {result:?}"), _=fixture.request()=>{}}
    drop(pending);
    fixture.disconnected().await;
    let mut pending = Box::pin(provider.show_model(
        "fixture",
        RequestContext {
            deadline: Some(std::time::Instant::now() + Duration::from_millis(200)),
            ..Default::default()
        },
    ));
    tokio::select! {biased; result=&mut pending=>panic!("unexpected {result:?}"), _=fixture.request()=>{}}
    assert_eq!(pending.await.err().unwrap().code, "provider_timeout");
    fixture.disconnected().await;
    // Exercise bounded JSON transfer, not only request-header waiting.
    let mut body_fixture = Fixture::start(vec![Reply {
        stall: 2,
        ..Reply::json(details())
    }])
    .await;
    let (broker, _) = super::broker();
    let body_provider = OllamaProvider::new(
        config(&body_fixture, false),
        vec![model("fixture", "native-fixture")],
        broker,
        Limits {
            idle_timeout: Duration::from_millis(200),
            ..limits()
        },
    )
    .unwrap();
    let mut pending = Box::pin(body_provider.show_model("fixture", RequestContext::default()));
    tokio::select! {biased; result=&mut pending=>panic!("unexpected {result:?}"), _=body_fixture.request()=>{}}
    assert_eq!(pending.await.err().unwrap().code, "provider_timeout");
    body_fixture.disconnected().await;
    assert_eq!(reads.load(Ordering::SeqCst), 3);
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 3);
}
