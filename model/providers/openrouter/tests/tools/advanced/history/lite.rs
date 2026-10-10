use super::*;

fn provider(f: &Fixture, b: Arc<Broker<Store>>, l: Limits) -> OpenRouterProvider<Store> {
    let mut m = model("fixture", "native-fixture");
    m.dialects = vec![ResponsesDialect::Classic, ResponsesDialect::Lite];
    OpenRouterProvider::with_lite_options(
        OpenRouterConfig::new(reference())
            .unwrap()
            .with_base_url(&f.base)
            .unwrap(),
        vec![m],
        b,
        l,
        Default::default(),
    )
    .unwrap()
    .with_runtime_context()
    .with_backend_selection("fixture".into(), "fixture-backend/region".into())
    .unwrap()
    .with_native_tools("fixture".into())
    .unwrap()
    .with_advanced_tools("fixture".into())
    .unwrap()
    .with_native_history("fixture".into())
    .unwrap()
    .with_reasoning_summary("fixture".into(), "auto".into())
    .unwrap()
    .with_reasoning_context("fixture".into(), "all_turns".into())
    .unwrap()
}
fn source(stream: bool) -> Value {
    let mut s = advanced_wire(stream);
    let declarations = s.as_object_mut().unwrap().remove("tools").unwrap();
    s["input"] = json!([
        {"type":"additional_tools","id":"at_stable","role":"developer","tools":declarations},
        {"type":"message","id":"msg_stable","role":"developer","content":[{"type":"input_text","text":"Keep rules"}]}
    ]);
    s["parallel_tool_calls"] = false.into();
    s["reasoning"] = json!({"summary":"auto","context":"all_turns"});
    s["include"] = json!(["reasoning.encrypted_content"]);
    s
}
fn lite(s: Value) -> CanonicalRequest {
    CanonicalRequest::new(s, ResponsesDialect::Lite).unwrap()
}
fn reply(n: &Value, stream: bool) -> Reply {
    if stream {
        let mut events = full_chunks(n);
        events[0]["response"]["id"] = n["id"].clone();
        Reply::stream(sse(&events))
    } else {
        Reply::json(n.clone())
    }
}
async fn deliver(p: &OpenRouterProvider<Store>, s: Value) -> CanonicalResponse {
    if s["stream"] != true {
        return p
            .create_response(lite(s), RequestContext::default())
            .await
            .unwrap()
            .response;
    }
    let mut events = p
        .stream_response(lite(s), RequestContext::default())
        .await
        .unwrap()
        .events;
    let mut terminal = None;
    let mut carrier = false;
    while let Some(e) = events.next().await {
        if let ProviderStreamEvent::Model(e) = e.unwrap() {
            let v = e.response.wire();
            if v["item"]["encrypted_content"].is_string() {
                carrier = true;
            }
            if matches!(
                v["item"]["type"].as_str(),
                Some("function_call" | "custom_tool_call")
            ) {
                assert!(carrier);
            }
            if e.response.kind() == "response.completed" {
                terminal = Some(v["response"].clone());
            }
        }
    }
    CanonicalResponse::new(terminal.unwrap()).unwrap()
}
async fn refused(p: &OpenRouterProvider<Store>, s: Value) -> caidex_model_core::ProviderError {
    if s["stream"] != true {
        return p
            .create_response(lite(s), RequestContext::default())
            .await
            .err()
            .expect("must refuse");
    }
    let mut events = match p.stream_response(lite(s), RequestContext::default()).await {
        Ok(r) => r.events,
        Err(e) => return e,
    };
    while let Some(e) = events.next().await {
        match e {
            Err(e) => return e,
            Ok(ProviderStreamEvent::Model(_)) => panic!("no model event before validation"),
            _ => (),
        }
    }
    panic!("must refuse")
}

#[tokio::test]
async fn explicit_constructor_preserves_route_dialects_and_default_rejections() {
    let f = Fixture::start(vec![]).await;
    let (b, reads) = super::super::super::super::broker(None);
    let p = provider(&f, b.clone(), limits());
    assert_eq!(
        p.metadata("fixture").unwrap().dialects,
        [ResponsesDialect::Classic, ResponsesDialect::Lite]
    );
    let mut m = model("fixture", "native-fixture");
    m.dialects = vec![ResponsesDialect::Lite];
    assert!(
        OpenRouterProvider::new(
            OpenRouterConfig::new(reference()).unwrap(),
            vec![m.clone()],
            b.clone(),
            limits()
        )
        .is_err()
    );
    let p = OpenRouterProvider::with_lite_options(
        OpenRouterConfig::new(reference())
            .unwrap()
            .with_base_url(&f.base)
            .unwrap(),
        vec![m],
        b.clone(),
        limits(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        p.create_response(
            canonical(json!({"model":"fixture","input":[]})),
            RequestContext::default()
        )
        .await
        .err()
        .unwrap()
        .http_status,
        400
    );
    for stream in [false, true] {
        assert_eq!(
            refused(&f.provider(b.clone(), limits()), source(stream))
                .await
                .http_status,
            400
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(f.accepted.load(Ordering::SeqCst), 0);
    let f = Fixture::start(vec![Reply::json(json!({"data":[{"id":"native-fixture"}]}))]).await;
    let (b, _) = super::super::super::super::broker(Some(KEY));
    let listed = provider(&f, b, limits()).list_models().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].dialects,
        [ResponsesDialect::Classic, ResponsesDialect::Lite]
    );
    assert_eq!(
        listed[0].capabilities.native_tools,
        CapabilitySupport::Unknown
    );
}

#[tokio::test]
async fn json_and_sse_preserve_native_custom_namespace_prefix_and_controls() {
    for stream in [false, true] {
        let calls = vec![custom_call(Some("functions"), "a")];
        let n = native(calls);
        let mut f = Fixture::start(vec![reply(&n, stream)]).await;
        let (b, _) = super::super::super::super::broker(Some(KEY));
        let s = source(stream);
        let p = provider(&f, b, limits());
        let projected = deliver(&p, s.clone()).await;
        let h = NativeHistory::from_responses(&projected, limits().response_bytes).unwrap();
        assert_eq!(h.native_response(), &n);
        assert_eq!(h.wire()["version"], 2);
        assert_eq!(
            h.wire()["policy"]["lite"]["additional_tools"],
            s["input"][0]
        );
        assert!(
            projected.output()[0]["encrypted_content"]
                .as_str()
                .unwrap()
                .starts_with("caidex.openrouter.native-history.v2:")
        );
        let got = f.request().await;
        assert_eq!(got.body.as_ref().unwrap()["tools"], s["input"][0]["tools"]);
        assert_eq!(got.body.as_ref().unwrap()["input"], json!([s["input"][1]]));
        assert_eq!(got.body.as_ref().unwrap()["reasoning"], s["reasoning"]);
        assert_eq!(got.body.as_ref().unwrap()["include"], s["include"]);
        assert_eq!(got.body.as_ref().unwrap()["parallel_tool_calls"], false);
        assert_eq!(got.body.as_ref().unwrap()["model"], "native-fixture");
        assert!(
            !got.headers
                .contains("x-openai-internal-codex-responses-lite")
        );
    }
}

#[tokio::test]
async fn lite_multiturn_restores_full_native_order_and_stable_developer_ids() {
    for stream in [false, true] {
        let calls = vec![ns_call("functions", "a")];
        let n = native(calls.clone());
        let mut next = native(vec![]);
        next["id"] = "second".into();
        let mut f = Fixture::start(vec![reply(&n, stream), reply(&next, stream)]).await;
        let (b, _) = super::super::super::super::broker(Some(KEY));
        let p = provider(&f, b, limits());
        let s = source(stream);
        let first = deliver(&p, s.clone()).await;
        f.request().await;
        let second = deliver(&p, replay(&s, &first, &calls)).await;
        let got = f.request().await;
        let mut expected = vec![s["input"][1].clone()];
        expected.extend(n["output"].as_array().unwrap().clone());
        expected.extend(results(&calls));
        assert_eq!(got.body.as_ref().unwrap()["input"], json!(expected));
        let h = NativeHistory::from_responses(&second, limits().response_bytes).unwrap();
        assert_eq!(h.request()["input"], got.body.as_ref().unwrap()["input"]);
    }
}

#[tokio::test]
async fn malformed_prefix_and_unsupported_tools_fail_before_credentials() {
    let f = Fixture::start(vec![]).await;
    let (b, reads) = super::super::super::super::broker(Some(KEY));
    let p = provider(&f, b, limits());
    for stream in [false, true] {
        for mode in 0..9 {
            let mut s = source(stream);
            match mode {
                0 => s["input"][0]["id"] = "bad\n".into(),
                1 => s["input"][0]["role"] = "user".into(),
                2 => s["input"][0]["tools"] = json!({}),
                3 => s["input"][0]["future"] = true.into(),
                4 => s["input"].as_array_mut().unwrap().swap(0, 1),
                5 => {
                    let i = s["input"][0].clone();
                    s["input"].as_array_mut().unwrap().push(i);
                }
                6 => s["parallel_tool_calls"] = Value::Null,
                7 => s["input"][0]["tools"][0]["defer_loading"] = true.into(),
                _ => s["input"][0]["tools"][1]["async"] = true.into(),
            }
            assert_eq!(refused(&p, s).await.http_status, 400, "mode {mode}");
        }
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(f.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn lite_single_call_is_checked_before_json_or_sse_delivery() {
    for stream in [false, true] {
        let n = native(mixed());
        let mut f = Fixture::start(vec![reply(&n, stream)]).await;
        let (b, _) = super::super::super::super::broker(Some(KEY));
        assert_eq!(
            refused(&provider(&f, b, limits()), source(stream))
                .await
                .http_status,
            502
        );
        f.request().await;
        for flag in [None, Some(true)] {
            let (b, _) = super::super::super::super::broker(Some(KEY));
            let mut s = source(stream);
            if let Some(v) = flag {
                s["parallel_tool_calls"] = v.into();
            } else {
                s.as_object_mut().unwrap().remove("parallel_tool_calls");
            }
            assert_eq!(
                NativeHistory::from_responses(
                    &deliver(&provider(&f, b, limits()), s).await,
                    limits().response_bytes
                )
                .unwrap()
                .native_response(),
                &n
            );
            f.request().await;
        }
    }
}

#[tokio::test]
async fn replay_rejects_changed_lite_policy_classic_and_public_carrier_forgery() {
    let calls = vec![call("exec", "a")];
    let n = native(calls.clone());
    let mut f = Fixture::start(vec![Reply::json(n)]).await;
    let (b, reads) = super::super::super::super::broker(Some(KEY));
    let p = provider(&f, b, limits());
    let s = source(false);
    let projected = deliver(&p, s.clone()).await;
    f.request().await;
    let saved = replay(&s, &projected, &calls);
    for mode in 0..5 {
        let mut changed = saved.clone();
        match mode {
            0 => changed["input"][0]["id"] = "at_other".into(),
            1 => changed["input"][0]["tools"][1]["format"]["definition"] = "changed".into(),
            2 => changed["parallel_tool_calls"] = true.into(),
            3 => changed["input"][1]["id"] = "msg_other".into(),
            _ => changed["input"][1]["content"][0]["text"] = "changed".into(),
        }
        assert_eq!(refused(&p, changed).await.http_status, 400);
    }
    let mut classic = saved.clone();
    classic["tools"] = classic["input"][0]["tools"].clone();
    classic["input"].as_array_mut().unwrap().remove(0);
    assert_eq!(
        p.create_response(canonical(classic), RequestContext::default())
            .await
            .err()
            .unwrap()
            .http_status,
        400
    );
    for mode in 0..4 {
        let mut v = projected.wire().clone();
        let mut h = carrier(&v["output"][0]);
        match mode {
            0 => h["version"] = 1.into(),
            1 => h["policy"]["lite"]["additional_tools"]["tools"][0]["name"] = "different".into(),
            2 => h["policy"]["lite"]["additional_tools"]["future"] = true.into(),
            _ => h["policy"]["lite"]["future"] = true.into(),
        }
        v["output"][0]["encrypted_content"] =
            format!("caidex.openrouter.native-history.v2:{h}").into();
        assert!(
            NativeHistory::from_responses(
                &CanonicalResponse::new(v).unwrap(),
                limits().response_bytes
            )
            .is_err()
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(f.accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn source_prefix_and_carrier_budgets_fail_without_partial_delivery() {
    let n = native(vec![call("exec", "a")]);
    let mut f = Fixture::start(vec![Reply::json(n.clone())]).await;
    let (b, reads) = super::super::super::super::broker(Some(KEY));
    let s = source(false);
    let mut l = limits();
    l.request_bytes = s.to_string().len() - 1;
    assert_eq!(
        refused(&provider(&f, b.clone(), l), s).await.http_status,
        413
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let mut l = limits();
    l.response_bytes = n.to_string().len() + 20;
    assert_eq!(
        refused(&provider(&f, b, l), source(false))
            .await
            .http_status,
        502
    );
    f.request().await;
}

#[tokio::test]
async fn classic_history_stays_v1_and_lite_without_tools_is_valid() {
    let n = native(vec![]);
    let mut f = Fixture::start(vec![Reply::json(n.clone())]).await;
    let (b, _) = super::super::super::super::broker(Some(KEY));
    let p = provider(&f, b, limits());
    let classic = p
        .create_response(canonical(advanced_wire(false)), RequestContext::default())
        .await
        .unwrap()
        .response;
    f.request().await;
    assert_eq!(
        NativeHistory::from_responses(&classic, limits().response_bytes)
            .unwrap()
            .wire()["version"],
        1
    );
    let s = json!({"model":"fixture","input":[]});
    let (b, _) = super::super::super::super::broker(Some(KEY));
    let mut m = model("fixture", "native-fixture");
    m.dialects = vec![ResponsesDialect::Lite];
    let plain = OpenRouterProvider::with_lite_options(
        OpenRouterConfig::new(reference())
            .unwrap()
            .with_base_url(&f.base)
            .unwrap(),
        vec![m],
        b,
        limits(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        plain
            .create_response(lite(s.clone()), RequestContext::default())
            .await
            .unwrap()
            .response
            .wire(),
        &n
    );
    f.request().await;
    let projected = deliver(&p, s).await;
    f.request().await;
    assert_eq!(
        NativeHistory::from_responses(&projected, limits().response_bytes)
            .unwrap()
            .wire()["policy"]["lite"]["additional_tools"],
        Value::Null
    );
}

#[tokio::test]
async fn lite_cancel_deadline_drop_and_projected_queue_release_single_slot() {
    for mode in ["cancel", "deadline", "drop", "queue"] {
        let n = native(vec![custom_call(Some("functions"), "a")]);
        let good = full_chunks(&n);
        let first = if mode == "queue" {
            Reply::stream(sse(&good))
        } else {
            Reply {
                stall: 2,
                ..Reply::stream(sse(&good[..1]))
            }
        };
        let mut f = Fixture::start(vec![first, Reply::stream(sse(&good))]).await;
        let (b, _) = super::super::super::super::broker(Some(KEY));
        let p = provider(
            &f,
            b,
            Limits {
                in_flight: 1,
                ..limits()
            },
        );
        let token = CancellationToken::default();
        let mut context = RequestContext {
            cancellation: token.clone(),
            ..RequestContext::default()
        };
        if mode == "deadline" {
            context.deadline = Some(std::time::Instant::now() + Duration::from_millis(200));
        }
        let mut events = p
            .stream_response(lite(source(true)), context)
            .await
            .unwrap()
            .events;
        f.request().await;
        if mode == "drop" {
            drop(events);
        } else if mode == "queue" {
            assert!(matches!(
                events.next().await.unwrap().unwrap(),
                ProviderStreamEvent::Model(_)
            ));
            token.cancel();
            assert_eq!(events.next().await.unwrap().err().unwrap().http_status, 503);
            assert!(events.next().await.is_none());
        } else {
            let pending = tokio::spawn(async move {
                let e = events.next().await.unwrap().err().unwrap();
                assert!(events.next().await.is_none());
                e
            });
            if mode == "cancel" {
                tokio::task::yield_now().await;
                token.cancel();
            }
            assert_eq!(
                tokio::time::timeout(WAIT, pending)
                    .await
                    .unwrap()
                    .unwrap()
                    .http_status,
                if mode == "cancel" { 503 } else { 504 }
            );
        }
        f.disconnected().await;
        assert_eq!(
            NativeHistory::from_responses(
                &deliver(&p, source(true)).await,
                limits().response_bytes
            )
            .unwrap()
            .native_response(),
            &n
        );
        f.request().await;
        f.disconnected().await;
    }
}

#[tokio::test]
async fn gateway_lite_header_is_consumed_and_native_credentials_stay_local() {
    let n = native(vec![custom_call(Some("functions"), "a")]);
    let mut f = Fixture::start(vec![Reply::json(n.clone())]).await;
    let (b, _) = super::super::super::super::broker(Some(KEY));
    let p = Arc::new(provider(&f, b.clone(), limits()));
    let gateway = caidex_model_gateway::start_with_provider(p, b.redactor(), limits())
        .await
        .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let r = client
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("x-openai-internal-codex-responses-lite", "true")
        .header("x-client-request-id", "executor-private")
        .header("content-type", "application/json")
        .body(source(false).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let response =
        CanonicalResponse::new(serde_json::from_slice(&r.bytes().await.unwrap()).unwrap()).unwrap();
    assert_eq!(
        NativeHistory::from_responses(&response, limits().response_bytes)
            .unwrap()
            .native_response(),
        &n
    );
    let got = f.request().await;
    for secret in [
        "executor-private",
        gateway.token().expose(),
        "x-openai-internal-codex-responses-lite",
    ] {
        assert!(!got.headers.contains(secret));
    }
    assert!(got.headers.contains(KEY));
    assert!(!response.wire().to_string().contains(KEY));
    gateway.shutdown().await.unwrap();
}
