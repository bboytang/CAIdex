use super::*;

fn configured(f: &Fixture, b: Arc<Broker<Store>>, l: Limits) -> OpenRouterProvider<Store> {
    history_provider(f, b, l)
        .with_runtime_context()
        .with_reasoning_summary("fixture".into(), "auto".into())
        .unwrap()
        .with_reasoning_context("fixture".into(), "all_turns".into())
        .unwrap()
        .with_reasoning_effort_mapping("fixture".into(), "xhigh".into(), "high".into())
        .unwrap()
        .with_verbosity_instruction("fixture".into(), "low".into(), "Concise".into())
        .unwrap()
        .with_service_tier_mapping("fixture".into(), "priority".into(), "fast".into())
        .unwrap()
}
fn control_wire(stream: bool) -> Value {
    let mut s = advanced_wire(stream);
    s["reasoning"] = json!({"effort":"xhigh","summary":"auto","context":"all_turns"});
    s["include"] = json!(["reasoning.encrypted_content"]);
    s["text"] = json!({"format":{"type":"text"},"verbosity":"low"});
    s["service_tier"] = "priority".into();
    s["instructions"] = "original".into();
    s
}

#[tokio::test]
async fn configuration_requires_exact_route_dual_policy_and_known_unique_values() {
    let f = Fixture::start(vec![]).await;
    let (b, reads) = super::super::super::super::broker(None);
    for mode in 0..3 {
        let p = match mode {
            0 => f.provider(b.clone(), limits()).with_runtime_context(),
            1 => history_provider(&f, b.clone(), limits()),
            _ => f.provider(b.clone(), limits()),
        };
        assert!(
            p.with_reasoning_summary("fixture".into(), "auto".into())
                .is_err()
        );
    }
    for (summary, context) in [("invalid", "all_turns"), ("auto", "invalid")] {
        let p = history_provider(&f, b.clone(), limits()).with_runtime_context();
        if summary == "invalid" {
            assert!(
                p.with_reasoning_summary("fixture".into(), summary.into())
                    .is_err()
            );
        } else {
            assert!(
                p.with_reasoning_context("fixture".into(), context.into())
                    .is_err()
            );
        }
    }
    assert!(
        configured(&f, b.clone(), limits())
            .with_reasoning_summary("fixture".into(), "auto".into())
            .is_err()
    );
    assert!(
        configured(&f, b.clone(), limits())
            .with_reasoning_context("fixture".into(), "all_turns".into())
            .is_err()
    );
    assert!(
        configured(&f, b, limits())
            .with_reasoning_summary("missing".into(), "auto".into())
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn full_native_controls_preserve_summary_context_include_and_compile_other_controls_once() {
    let n = native(mixed());
    let mut f = Fixture::start(vec![Reply::json(n.clone())]).await;
    let (b, _) = super::super::super::super::broker(Some(KEY));
    let p = configured(&f, b, limits());
    let mut s = control_wire(false);
    s["client_metadata"] = json!({"local":"private"});
    s["prompt_cache_key"] = "local-private".into();
    let a = p
        .create_response(canonical(s.clone()), runtime_context())
        .await
        .unwrap()
        .response;
    let request = f.request().await;
    let sent = request.body.unwrap();
    assert_eq!(
        sent["reasoning"],
        json!({"effort":"high","summary":"auto","context":"all_turns"})
    );
    assert_eq!(sent["include"], s["include"]);
    assert_eq!(sent["instructions"], "original\nConcise");
    assert_eq!(sent["service_tier"], "fast");
    assert!(sent.get("text").is_none());
    assert!(sent.get("client_metadata").is_none());
    assert!(!request.headers.contains("executor-private"));
    let h = NativeHistory::from_responses(&a, limits().response_bytes).unwrap();
    assert_eq!(h.native_response(), &n);
    assert_eq!(h.wire()["policy"]["controls"]["reasoning"], s["reasoning"]);
    assert_eq!(h.request(), &sent);
    assert_eq!(h.wire()["policy"]["summaries"], json!(["auto"]));
    assert_eq!(h.wire()["policy"]["contexts"], json!(["all_turns"]));
    assert_eq!(
        p.capabilities("fixture").unwrap().reasoning,
        CapabilitySupport::Unknown
    );
}

#[tokio::test]
async fn native_enum_null_and_empty_controls_do_not_require_effort_or_invent_defaults() {
    let n = native(vec![]);
    let mut f = Fixture::start(vec![Reply::json(n)]).await;
    let (b, _) = super::super::super::super::broker(Some(KEY));
    let mut p = history_provider(&f, b, limits()).with_runtime_context();
    for v in ["auto", "concise", "detailed"] {
        p = p
            .with_reasoning_summary("fixture".into(), v.into())
            .unwrap();
    }
    for v in ["auto", "all_turns", "current_turn"] {
        p = p
            .with_reasoning_context("fixture".into(), v.into())
            .unwrap();
    }
    for (summary, context, include) in [
        (json!("auto"), json!("auto"), json!([])),
        (json!("concise"), json!("all_turns"), Value::Null),
        (
            json!("detailed"),
            json!("current_turn"),
            json!(["reasoning.encrypted_content"]),
        ),
        (Value::Null, Value::Null, Value::Null),
    ] {
        let mut s = advanced_wire(false);
        s["reasoning"] = json!({"summary":summary,"context":context});
        s["include"] = include;
        p.create_response(canonical(s.clone()), RequestContext::default())
            .await
            .unwrap();
        let sent = f.request().await.body.unwrap();
        assert_eq!(sent["reasoning"], s["reasoning"]);
        assert_eq!(sent["include"], s["include"]);
        assert!(sent["reasoning"].get("effort").is_none());
    }
}

#[tokio::test]
async fn default_partial_unconfigured_and_unsupported_routes_stay_closed_before_key() {
    let f = Fixture::start(vec![]).await;
    let (b, reads) = super::super::super::super::broker(None);
    for mode in 0..3 {
        let p = match mode {
            0 => f.provider(b.clone(), limits()).with_runtime_context(),
            1 => history_provider(&f, b.clone(), limits()),
            _ => history_provider(&f, b.clone(), limits()).with_runtime_context(),
        };
        let mut s = advanced_wire(false);
        s["reasoning"] = json!({"summary":"auto","context":"all_turns"});
        assert!(
            p.create_response(canonical(s), RequestContext::default())
                .await
                .is_err()
        );
    }
    for (field, value) in [
        ("reasoning", json!({"summary":"future"})),
        ("reasoning", json!({"context":"future"})),
        ("reasoning", json!({"summary":true})),
        ("reasoning", json!({"context":[]})),
        ("reasoning", json!({"effort":"xhigh","mode":"pro"})),
        (
            "include",
            json!(["reasoning.encrypted_content", "reasoning.encrypted_content"]),
        ),
        ("include", json!("reasoning.encrypted_content")),
        ("include", json!(["output_text.logprobs"])),
    ] {
        let mut s = control_wire(false);
        s[field] = value;
        assert!(
            configured(&f, b.clone(), limits())
                .create_response(canonical(s), runtime_context())
                .await
                .is_err()
        );
    }
    let mut m = model("fixture", "native-fixture");
    m.capabilities.reasoning = CapabilitySupport::Unsupported;
    let p = OpenRouterProvider::new(
        OpenRouterConfig::new(reference())
            .unwrap()
            .with_base_url(&f.base)
            .unwrap(),
        vec![m],
        b,
        limits(),
    )
    .unwrap()
    .with_backend_selection("fixture".into(), "fixture-backend/region".into())
    .unwrap()
    .with_native_history("fixture".into())
    .unwrap()
    .with_runtime_context()
    .with_reasoning_summary("fixture".into(), "auto".into())
    .unwrap()
    .with_reasoning_context("fixture".into(), "all_turns".into())
    .unwrap();
    for field in ["summary", "context", "include"] {
        let mut s = json!({"model":"fixture","input":[]});
        if field == "include" {
            s["include"] = json!(["reasoning.encrypted_content"]);
        } else {
            s["reasoning"][field] = if field == "summary" {
                "auto"
            } else {
                "all_turns"
            }
            .into();
        }
        assert_eq!(
            p.create_response(canonical(s), RequestContext::default())
                .await
                .err()
                .unwrap()
                .code,
            "unsupported_reasoning"
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(f.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn controls_json_to_sse_replay_restores_full_reasoning_and_tool_history() {
    let calls = mixed();
    let a = native(calls.clone());
    let mut z = native(vec![]);
    z["id"] = "next".into();
    z["output"][0]["id"] = "msg_next".into();
    z["output"][1]["id"] = "rs_next".into();
    let mut chunks = full_chunks(&z);
    chunks[0]["response"]["id"] = "next".into();
    let mut f = Fixture::start(vec![Reply::json(a.clone()), Reply::stream(sse(&chunks))]).await;
    let (b, _) = super::super::super::super::broker(Some(KEY));
    let p = configured(&f, b, limits());
    let s = control_wire(false);
    let projected = p
        .create_response(canonical(s.clone()), runtime_context())
        .await
        .unwrap()
        .response;
    f.request().await;
    let mut next = replay(&s, &projected, &calls);
    next["stream"] = true.into();
    let events = delivered(&p, next).await;
    let sent = f.request().await.body.unwrap();
    let mut input = a["output"].as_array().unwrap().clone();
    input.extend(results(&calls));
    assert_eq!(sent["input"], json!(input));
    assert_eq!(
        sent["reasoning"],
        json!({"effort":"high","summary":"auto","context":"all_turns"})
    );
    assert_eq!(sent["instructions"], "original\nConcise");
    let terminal = CanonicalResponse::new(events.last().unwrap()["response"].clone()).unwrap();
    let h = NativeHistory::from_responses(&terminal, limits().response_bytes).unwrap();
    assert_eq!(h.native_response(), &z);
    assert_eq!(h.wire()["chunks"], json!(chunks));
}

#[tokio::test]
async fn source_and_internal_native_controls_and_allowlists_are_history_bound() {
    let f = Fixture::start(vec![Reply::json(native(vec![]))]).await;
    let (b, reads) = super::super::super::super::broker(Some(KEY));
    let p = configured(&f, b.clone(), limits())
        .with_reasoning_summary("fixture".into(), "detailed".into())
        .unwrap();
    let s = control_wire(false);
    let a = p
        .create_response(canonical(s.clone()), runtime_context())
        .await
        .unwrap()
        .response;
    let base = replay(&s, &a, &[]);
    for choices in [
        json!(["auto", "future"]),
        json!(["auto", "auto"]),
        json!([]),
    ] {
        let mut projected = a.wire().clone();
        let mut h = carrier(&projected["output"][0]);
        h["policy"]["summaries"] = choices;
        projected["output"][0]["encrypted_content"] =
            format!("caidex.openrouter.native-history.v1:{h}").into();
        assert!(
            NativeHistory::from_responses(
                &CanonicalResponse::new(projected).unwrap(),
                limits().response_bytes
            )
            .is_err()
        );
    }

    for mode in 0..6 {
        let mut v = base.clone();
        let mut h = carrier(&v["input"][0]);
        match mode {
            0 => v["reasoning"]["summary"] = "detailed".into(),
            1 => v["include"] = json!([]),
            2 => h["request"]["reasoning"]["context"] = "current_turn".into(),
            3 => h["request"]["include"] = json!([]),
            4 => h["request"]["reasoning"]["effort"] = "xhigh".into(),
            _ => h["policy"]["summaries"] = json!(["auto"]),
        }
        if mode >= 2 {
            replace_carrier(&mut v, h);
        }
        assert!(
            p.create_response(canonical(v), runtime_context())
                .await
                .is_err(),
            "{mode}"
        );
    }
    assert!(
        configured(&f, b, limits())
            .create_response(canonical(base), runtime_context())
            .await
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn no_control_policy_preserves_existing_v1_carriers_and_config_changes_require_branch() {
    let mut f = Fixture::start(vec![Reply::json(native(vec![]))]).await;
    let (b, reads) = super::super::super::super::broker(Some(KEY));
    let p = history_provider(&f, b.clone(), limits()).with_runtime_context();
    let s = advanced_wire(false);
    let a = p
        .create_response(canonical(s.clone()), runtime_context())
        .await
        .unwrap()
        .response;
    f.request().await;
    let h = NativeHistory::from_responses(&a, limits().response_bytes).unwrap();
    assert!(h.wire()["policy"].get("summaries").is_none());
    assert!(h.wire()["policy"].get("contexts").is_none());
    let next = replay(&s, &a, &[]);
    p.create_response(canonical(next.clone()), runtime_context())
        .await
        .unwrap();
    f.request().await;
    assert!(
        history_provider(&f, b, limits())
            .with_runtime_context()
            .with_reasoning_summary("fixture".into(), "auto".into())
            .unwrap()
            .create_response(canonical(next), runtime_context())
            .await
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn source_and_compiled_controls_budgets_and_gateway_identity_separation_hold() {
    let f = Fixture::start(vec![]).await;
    let (b, reads) = super::super::super::super::broker(Some(KEY));
    for mode in 0..2 {
        let p = history_provider(
            &f,
            b.clone(),
            Limits {
                request_bytes: 1024,
                ..limits()
            },
        )
        .with_runtime_context()
        .with_reasoning_summary("fixture".into(), "auto".into())
        .unwrap()
        .with_verbosity_instruction("fixture".into(), "low".into(), "g".repeat(2048))
        .unwrap();
        let mut s = advanced_wire(false);
        s["reasoning"] = json!({"summary":"auto"});
        s["include"] = json!(["reasoning.encrypted_content"]);
        if mode == 0 {
            s["client_metadata"] = json!({"large":"x".repeat(2048)});
        } else {
            s["text"] = json!({"verbosity":"low"});
        }
        assert_eq!(
            p.create_response(canonical(s), runtime_context())
                .await
                .err()
                .unwrap()
                .http_status,
            413
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let mut f = Fixture::start(vec![Reply::json(native(vec![]))]).await;
    let p = Arc::new(configured(&f, b.clone(), limits()));
    let g = caidex_model_gateway::start_with_provider(p, b.redactor(), limits())
        .await
        .unwrap();
    let c = reqwest::Client::builder().no_proxy().build().unwrap();
    let r = c
        .post(format!("http://{}/v1/responses", g.address()))
        .bearer_auth(g.token().expose())
        .header("x-client-request-id", "executor-private")
        .header("content-type", "application/json")
        .body(control_wire(false).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let response: Value = serde_json::from_slice(&r.bytes().await.unwrap()).unwrap();
    assert!(!response.to_string().contains(KEY));
    let sent = f.request().await;
    assert!(!sent.headers.contains(g.token().expose()));
    assert!(!sent.headers.contains("executor-private"));
    assert_eq!(
        sent.body.unwrap()["include"],
        json!(["reasoning.encrypted_content"])
    );
    g.shutdown().await.unwrap();
}
