use super::*;
use caidex_model_core::{CanonicalResponse, ResponsesStream, StreamState};
use caidex_provider_openrouter::NativeHistory;

fn history_provider(f: &Fixture, b: Arc<Broker<Store>>, l: Limits) -> OpenRouterProvider<Store> {
    advanced(f, b, l)
        .with_native_history("fixture".into())
        .unwrap()
}
fn native(calls: Vec<Value>) -> Value {
    let mut v = response(calls);
    v["output"][1] = json!({"type":"reasoning","id":"rs_native","status":"completed","summary":[{"type":"summary_text","text":"简述","future":"summary"}],"content":[{"type":"reasoning_text","text":"原始推理🙂","future":true}],"encrypted_content":"opaque+/==","signature":"native-signature","format":"anthropic-v1","future":{"big":18446744073709551616_u128}});
    v["output"][0]["content"][0]["annotations"] =
        json!([{"type":"future_annotation","opaque":true}]);
    v["model"] = "native-fixture".into();
    v
}
fn results(calls: &[Value]) -> Vec<Value> {
    calls.iter().map(|c| json!({"type":if c["type"]=="custom_tool_call" {"custom_tool_call_output"} else {"function_call_output"},"call_id":c["call_id"],"output":"结果🙂"})).collect()
}
fn replay(source: &Value, projected: &CanonicalResponse, calls: &[Value]) -> Value {
    let mut v = source.clone();
    let mut items = source["input"].as_array().unwrap().clone();
    // Fixed Runtime drops output IDs/status and unknown content fields.
    items.extend(projected.output().iter().map(|i| {
        let mut i = i.clone();
        i.as_object_mut().unwrap().remove("id");
        i.as_object_mut().unwrap().remove("status");
        i
    }));
    items.extend(results(calls));
    v["input"] = json!(items);
    v
}
fn carrier(v: &Value) -> Value {
    serde_json::from_str(
        v["encrypted_content"]
            .as_str()
            .unwrap()
            .split_once(':')
            .unwrap()
            .1,
    )
    .unwrap()
}
fn replace_carrier(v: &mut Value, history: Value) {
    v["input"][0]["encrypted_content"] =
        format!("caidex.openrouter.native-history.v1:{history}").into();
}
fn full_chunks(n: &Value) -> Vec<Value> {
    let mut e = native_chunks(n);
    let terminal = e.pop().unwrap();
    for (index, item) in n["output"].as_array().unwrap().iter().enumerate().take(2) {
        let reasoning = item["type"] == "reasoning";
        let mut added = item.clone();
        added["status"] = "in_progress".into();
        added["content"] = json!([]);
        if reasoning {
            added["summary"] = json!([]);
        }
        e.push(json!({"type":"response.output_item.added","output_index":index,"item":added}));
        for (section, index_key, kind) in if reasoning {
            vec![
                ("content", "content_index", "response.reasoning_text"),
                (
                    "summary",
                    "summary_index",
                    "response.reasoning_summary_text",
                ),
            ]
        } else {
            vec![("content", "content_index", "response.output_text")]
        } {
            for (pi, part) in item[section].as_array().unwrap().iter().enumerate() {
                let mut delta = json!({"type":format!("{kind}.delta"),"output_index":index,"item_id":item["id"],"delta":part["text"]});
                delta[index_key] = pi.into();
                e.push(delta);
                let mut done = json!({"type":format!("{kind}.done"),"output_index":index,"item_id":item["id"],"text":part["text"]});
                done[index_key] = pi.into();
                e.push(done);
            }
        }
        e.push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
    }
    e.push(json!({"type":"response.reasoning.delta","opaque":"future native shape"}));
    e.push(terminal);
    e
}

#[tokio::test]
async fn history_optin_requires_backend_and_default_stays_closed() {
    let f = Fixture::start(vec![]).await;
    let (b, reads) = super::super::super::broker(None);
    assert!(
        f.provider(b.clone(), limits())
            .with_native_history("fixture".into())
            .is_err()
    );
    assert!(
        history_provider(&f, b.clone(), limits())
            .with_native_history("fixture".into())
            .is_err()
    );
    assert!(
        provider(&f, b.clone(), limits())
            .with_native_history("missing".into())
            .is_err()
    );
    let mut source = wire(false);
    source["input"] = json!([native(vec![])["output"][1]]);
    assert!(
        provider(&f, b, limits())
            .create_response(canonical(source), RequestContext::default())
            .await
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(f.accepted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn full_json_history_restores_three_turns_unknown_fields_and_qualified_calls() {
    let calls = mixed();
    let first = native(calls.clone());
    let mut second = native(vec![]);
    second["id"] = "second".into();
    // IDs across responses must not reuse previous calls/items.
    second["output"][0]["id"] = "msg_second".into();
    second["output"][1]["id"] = "rs_second".into();
    let mut f = Fixture::start(vec![
        Reply::json(first.clone()),
        Reply::json(second.clone()),
        Reply::json(second.clone()),
    ])
    .await;
    let (b, _) = super::super::super::broker(Some(KEY));
    let p = history_provider(&f, b, limits());
    let mut source = advanced_wire(false);
    source["input"] = json!([{"role":"user","content":"首轮🙂"}]);
    let a = p
        .create_response(canonical(source.clone()), RequestContext::default())
        .await
        .unwrap()
        .response;
    let request_a = f.request().await.body.unwrap();
    let h = NativeHistory::from_responses(&a, limits().response_bytes).unwrap();
    assert_eq!(h.native_response(), &first);
    assert_eq!(h.request(), &request_a);
    assert!(!format!("{h:?}").contains("native-signature"));
    assert!(!h.wire().to_string().contains(KEY));
    assert_eq!(a.output().len(), 7); // carrier + message + five native calls
    assert!(a.output()[1]["content"][0].get("annotations").is_none());
    let turn2 = replay(&source, &a, &calls);
    let z = p
        .create_response(canonical(turn2.clone()), RequestContext::default())
        .await
        .unwrap()
        .response;
    let request_b = f.request().await.body.unwrap();
    let mut expected = source["input"].as_array().unwrap().clone();
    expected.extend(first["output"].as_array().unwrap().clone());
    expected.extend(results(&calls));
    assert_eq!(request_b["input"], json!(expected));
    let turn3 = replay(&turn2, &z, &[]);
    p.create_response(canonical(turn3), RequestContext::default())
        .await
        .unwrap();
    expected.extend(second["output"].as_array().unwrap().clone());
    assert_eq!(f.request().await.body.unwrap()["input"], json!(expected));
}

#[tokio::test]
async fn history_without_tools_normalizes_string_and_allows_json_to_sse() {
    let n = native(vec![]);
    let chunks = full_chunks(&n);
    let mut f = Fixture::start(vec![Reply::json(n.clone()), Reply::stream(sse(&chunks))]).await;
    let (b, _) = super::super::super::broker(Some(KEY));
    let p = f
        .provider(b, limits())
        .with_backend_selection("fixture".into(), "fixture-backend/region".into())
        .unwrap()
        .with_native_history("fixture".into())
        .unwrap();
    let s = json!({"model":"fixture","input":"hello"});
    let a = p
        .create_response(canonical(s), RequestContext::default())
        .await
        .unwrap()
        .response;
    assert_eq!(
        f.request().await.body.unwrap()["input"],
        json!([{"role":"user","content":"hello"}])
    );
    let mut s =
        json!({"model":"fixture","input":[{"role":"user","content":"hello"}],"stream":true});
    s = replay(&s, &a, &[]);
    let out = delivered(&p, s).await;
    let sent = f.request().await.body.unwrap();
    assert_eq!(sent["input"].as_array().unwrap().len(), 4);
    let terminal = CanonicalResponse::new(out.last().unwrap()["response"].clone()).unwrap();
    let h = NativeHistory::from_responses(&terminal, limits().response_bytes).unwrap();
    assert_eq!(h.wire()["chunks"], json!(chunks));
    assert_eq!(h.native_response(), &n);
}

#[tokio::test]
async fn policy_and_scope_changes_reject_before_credentials() {
    let n = native(vec![]);
    let f = Fixture::start(vec![Reply::json(n)]).await;
    let (b, reads) = super::super::super::broker(Some(KEY));
    let p = history_provider(&f, b.clone(), limits());
    let source = advanced_wire(false);
    let a = p
        .create_response(canonical(source.clone()), RequestContext::default())
        .await
        .unwrap()
        .response;
    let base = replay(&source, &a, &[]);
    for (key, value) in [
        ("instructions", json!("changed")),
        ("temperature", json!(0.1)),
        ("parallel_tool_calls", json!(false)),
        ("tool_choice", json!("none")),
        ("tools", json!([function("exec")])),
    ] {
        let mut s = base.clone();
        s[key] = value;
        assert!(
            p.create_response(canonical(s), RequestContext::default())
                .await
                .is_err(),
            "{key}"
        );
    }
    for field in ["owner", "profile"] {
        let mut r = reference();
        if field == "owner" {
            r.owner = Id::new("other").unwrap();
        } else {
            r.profile = Id::new("other").unwrap();
        }
        let q = f
            .provider_with(r, b.clone(), limits())
            .with_backend_selection("fixture".into(), "fixture-backend/region".into())
            .unwrap()
            .with_native_tools("fixture".into())
            .unwrap()
            .with_advanced_tools("fixture".into())
            .unwrap()
            .with_native_history("fixture".into())
            .unwrap();
        assert!(
            q.create_response(canonical(base.clone()), RequestContext::default())
                .await
                .is_err()
        );
    }
    let other = Fixture::start(vec![]).await;
    assert!(
        history_provider(&other, b.clone(), limits())
            .create_response(canonical(base.clone()), RequestContext::default())
            .await
            .is_err()
    );
    let q = history_provider(&f, b, limits())
        .with_reasoning_effort_mapping("fixture".into(), "high".into(), "high".into())
        .unwrap();
    assert!(
        q.create_response(canonical(base), RequestContext::default())
            .await
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(f.accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn prefix_and_entire_runtime_projection_are_required() {
    let n = native(vec![call("exec", "a")]);
    let f = Fixture::start(vec![Reply::json(n)]).await;
    let (b, reads) = super::super::super::broker(Some(KEY));
    let p = history_provider(&f, b, limits());
    let mut s = advanced_wire(false);
    s["input"] = json!([{"role":"user","content":"original"}]);
    let a = p
        .create_response(canonical(s.clone()), RequestContext::default())
        .await
        .unwrap()
        .response;
    let base = replay(&s, &a, &[call("exec", "a")]);
    for mode in 0..8 {
        let mut v = base.clone();
        match mode {
            0 => v["input"][0]["content"] = "changed".into(),
            1 => {
                v["input"].as_array_mut().unwrap().remove(2);
            }
            2 => v["input"][2]["content"][0]["text"] = "changed".into(),
            3 => v["input"][3]["arguments"] = "{}".into(),
            4 => v["input"][1]["summary"][0]["text"] = "changed".into(),
            5 => v["input"][2] = Value::Null,
            6 => v["input"].as_array_mut().unwrap().swap(2, 3),
            _ => v["input"][4]["type"] = "custom_tool_call_output".into(),
        }
        assert!(
            p.create_response(canonical(v), RequestContext::default())
                .await
                .is_err(),
            "{mode}"
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn malformed_carrier_internal_contract_is_not_trusted() {
    let f = Fixture::start(vec![Reply::json(native(vec![]))]).await;
    let (b, reads) = super::super::super::broker(Some(KEY));
    let p = history_provider(&f, b, limits());
    let s = advanced_wire(false);
    let a = p
        .create_response(canonical(s.clone()), RequestContext::default())
        .await
        .unwrap()
        .response;
    let base = replay(&s, &a, &[]);
    let h = carrier(&base["input"][0]);
    for mode in 0..9 {
        let mut v = base.clone();
        let mut h = h.clone();
        match mode {
            0 => h["version"] = 2.into(),
            1 => h["provider"] = "qwen".into(),
            2 => h["request"]["model"] = "other".into(),
            3 => h["request"]["instructions"] = "forged".into(),
            4 => h["response"]["output"][1]["summary"] = json!([]),
            5 => h["response"]["output"][1]["content"][0]["type"] = "summary_text".into(),
            6 => h["source"] = "sse".into(),
            7 => h["scope"]["credential"]["provider"] = "qwen".into(),
            _ => h["response"]["model"] = "other".into(),
        }
        replace_carrier(&mut v, h);
        assert!(
            p.create_response(canonical(v), RequestContext::default())
                .await
                .is_err(),
            "{mode}"
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn malformed_native_reasoning_and_response_identity_fail_closed() {
    for mode in 0..7 {
        let mut n = native(vec![]);
        match mode {
            0 => n["output"][1]["id"] = Value::Null,
            1 => n["output"][1]["summary"] = json!({}),
            2 => n["output"][1]["content"][0]["type"] = "output_text".into(),
            3 => n["output"][1]["signature"] = true.into(),
            4 => n["model"] = "other".into(),
            5 => n["output"][0]["id"] = "rs_native".into(),
            _ => n["output"][0]["role"] = "user".into(),
        }
        let f = Fixture::start(vec![Reply::json(n)]).await;
        let (b, _) = super::super::super::broker(Some(KEY));
        let error = history_provider(&f, b, limits())
            .create_response(canonical(advanced_wire(false)), RequestContext::default())
            .await
            .err()
            .unwrap();
        assert_eq!(error.http_status, 502, "{mode}");
    }
}

#[tokio::test]
async fn sse_complete_wire_is_retained_and_projection_is_valid_responses() {
    let n = native(mixed());
    let chunks = full_chunks(&n);
    let mut f = Fixture::start(vec![Reply::stream(sse(&chunks))]).await;
    let (b, _) = super::super::super::broker(Some(KEY));
    let out = delivered(&history_provider(&f, b, limits()), advanced_wire(true)).await;
    f.request().await;
    let mut parser = ResponsesStream::new(limits().frame_bytes).unwrap();
    for v in &out {
        parser.push(format!("data: {v}\n\n").as_bytes()).unwrap();
    }
    assert_eq!(parser.finish().unwrap(), StreamState::Completed);
    let terminal = CanonicalResponse::new(out.last().unwrap()["response"].clone()).unwrap();
    let h = NativeHistory::from_responses(&terminal, limits().response_bytes).unwrap();
    assert_eq!(h.native_response(), &n);
    assert_eq!(h.wire()["chunks"], json!(chunks));
    assert_eq!(
        out.iter()
            .filter(|v| v["type"] == "response.output_text.delta")
            .count(),
        1
    );
    assert_eq!(out.iter().filter(|v|v["type"]=="response.output_item.done" && v["item"]["type"]=="reasoning").count(),1);
}

#[tokio::test]
async fn corrupt_sse_text_identity_and_terminal_are_never_delivered() {
    for mode in 0..7 {
        let n = native(mixed());
        let mut e = full_chunks(&n);
        let delta = e
            .iter()
            .position(|v| v["type"] == "response.reasoning_text.delta")
            .unwrap();
        match mode {
            0 => e[delta]["delta"] = "wrong".into(),
            1 => e[delta]["item_id"] = "wrong".into(),
            2 => e[delta]["content_index"] = 7.into(),
            3 => {
                e.remove(delta + 1);
                e[delta]["delta"] = "partial".into();
            }
            4 => e.last_mut().unwrap()["response"]["output"][1]["signature"] = "changed".into(),
            5 => e[1]["item"]["namespace"] = "changed".into(),
            _ => {
                e.pop();
            }
        }
        let f = Fixture::start(vec![Reply::stream(sse(&e))]).await;
        let (b, _) = super::super::super::broker(Some(KEY));
        let p = history_provider(&f, b, limits());
        let mut stream = p
            .stream_response(canonical(advanced_wire(true)), RequestContext::default())
            .await
            .unwrap()
            .events;
        loop {
            match stream.next().await {
                Some(Err(_)) => break,
                Some(Ok(ProviderStreamEvent::Heartbeat)) => {}
                other => panic!("bad stream {mode}: {other:?}"),
            }
        }
    }
}

#[tokio::test]
async fn mapped_controls_compile_once_and_local_attribution_can_change() {
    let n = native(vec![]);
    let mut f = Fixture::start(vec![Reply::json(n)]).await;
    let (b, reads) = super::super::super::broker(Some(KEY));
    let p = history_provider(&f, b, limits())
        .with_runtime_context()
        .with_reasoning_effort_mapping("fixture".into(), "xhigh".into(), "high".into())
        .unwrap()
        .with_verbosity_instruction("fixture".into(), "low".into(), "Concise".into())
        .unwrap()
        .with_service_tier_mapping("fixture".into(), "priority".into(), "fast".into())
        .unwrap();
    let mut s = advanced_wire(false);
    s["instructions"] = "original".into();
    s["reasoning"] = json!({"effort":"xhigh"});
    s["text"] = json!({"verbosity":"low"});
    s["service_tier"] = "priority".into();
    s["client_metadata"] = json!({"turn":"one"});
    s["prompt_cache_key"] = "one".into();
    let a = p
        .create_response(canonical(s.clone()), runtime_context())
        .await
        .unwrap()
        .response;
    let sent = f.request().await.body.unwrap();
    assert_eq!(sent["instructions"], "original\nConcise");
    assert_eq!(sent["reasoning"]["effort"], "high");
    assert_eq!(sent["service_tier"], "fast");
    let mut next = replay(&s, &a, &[]);
    next["client_metadata"] = json!({"turn":"two"});
    next["prompt_cache_key"] = "two".into();
    p.create_response(canonical(next.clone()), runtime_context())
        .await
        .unwrap();
    let sent = f.request().await.body.unwrap();
    assert_eq!(sent["instructions"], "original\nConcise");
    assert_eq!(sent["reasoning"]["effort"], "high");
    for field in ["reasoning", "text", "service_tier", "client_metadata"] {
        let mut wrong = next.clone();
        wrong[field] = json!({"invalid":true});
        assert!(
            p.create_response(canonical(wrong), runtime_context())
                .await
                .is_err()
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn history_source_and_expanded_compilation_budgets_precede_key() {
    let f = Fixture::start(vec![]).await;
    let (b, reads) = super::super::super::broker(Some(KEY));
    let l = Limits {
        request_bytes: 1024,
        ..limits()
    };
    let p = history_provider(&f, b, l)
        .with_verbosity_instruction("fixture".into(), "low".into(), "x".repeat(2048))
        .unwrap();
    for expanded in [false, true] {
        let mut s = advanced_wire(false);
        if expanded {
            s["text"] = json!({"verbosity":"low"});
        } else {
            s["input"] = json!([{"role":"user","content":"x".repeat(2048)}]);
        }
        assert_eq!(
            p.create_response(canonical(s), RequestContext::default())
                .await
                .err()
                .unwrap()
                .http_status,
            413
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn carrier_raw_aggregate_and_projected_frame_budgets_fail_before_delivery() {
    let n = native(vec![]);
    let chunks = full_chunks(&n);
    for mode in 0..4 {
        let l = match mode {
            0 => Limits {
                request_bytes: 2048,
                ..limits()
            },
            1 => Limits {
                response_bytes: n.to_string().len() + 1,
                ..limits()
            },
            2 => Limits {
                frame_bytes: chunks.iter().map(|v| v.to_string().len()).max().unwrap() + 32,
                ..limits()
            },
            _ => Limits {
                response_bytes: chunks.iter().map(|v| v.to_string().len()).sum::<usize>() + 1,
                ..limits()
            },
        };
        let f = Fixture::start(vec![if mode == 0 {
            Reply::json(n.clone())
        } else {
            Reply::stream(sse(&chunks))
        }])
        .await;
        let (b, _) = super::super::super::broker(Some(KEY));
        let p = history_provider(&f, b, l);
        if mode == 0 {
            assert!(
                p.create_response(canonical(advanced_wire(false)), RequestContext::default())
                    .await
                    .is_err()
            );
        } else {
            let mut stream = p
                .stream_response(canonical(advanced_wire(true)), RequestContext::default())
                .await
                .unwrap()
                .events;
            assert!(stream.next().await.unwrap().is_err(), "{mode}");
            assert!(stream.next().await.is_none());
        }
    }
}

#[tokio::test]
async fn failed_and_incomplete_remain_native_without_replay_carriers() {
    for status in ["failed", "incomplete"] {
        let mut n = native(vec![]);
        n["status"] = status.into();
        let chunks = vec![
            json!({"type":"response.created","response":{"id":"fixture","output":[]}}),
            json!({"type":format!("response.{status}"),"response":n}),
        ];
        let f = Fixture::start(vec![Reply::json(n.clone()), Reply::stream(sse(&chunks))]).await;
        let (b, _) = super::super::super::broker(Some(KEY));
        let p = history_provider(&f, b, limits());
        assert_eq!(
            p.create_response(canonical(advanced_wire(false)), RequestContext::default())
                .await
                .unwrap()
                .response
                .wire(),
            &n
        );
        assert_eq!(delivered(&p, advanced_wire(true)).await, chunks);
    }
}

#[tokio::test]
async fn history_without_tools_cancel_deadline_drop_and_slot_release() {
    for mode in ["cancel", "deadline", "drop"] {
        let n = native(vec![]);
        let good = full_chunks(&n);
        let mut f = Fixture::start(vec![
            Reply {
                stall: 2,
                ..Reply::stream(sse(&good[..1]))
            },
            Reply::stream(sse(&good)),
        ])
        .await;
        let (b, _) = super::super::super::broker(Some(KEY));
        let p = f
            .provider(
                b,
                Limits {
                    in_flight: 1,
                    ..limits()
                },
            )
            .with_backend_selection("fixture".into(), "fixture-backend/region".into())
            .unwrap()
            .with_native_history("fixture".into())
            .unwrap();
        let token = CancellationToken::default();
        let mut context = RequestContext {
            cancellation: token.clone(),
            ..RequestContext::default()
        };
        if mode == "deadline" {
            context.deadline = Some(std::time::Instant::now() + Duration::from_millis(200));
        }
        let mut stream = p
            .stream_response(request(true, ResponsesDialect::Classic), context)
            .await
            .unwrap()
            .events;
        f.request().await;
        if mode == "drop" {
            drop(stream);
        } else {
            let pending = tokio::spawn(async move {
                let error = stream.next().await.unwrap().err().unwrap();
                assert!(stream.next().await.is_none());
                error
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
        let out = delivered(&p, json!({"model":"fixture","input":[],"stream":true})).await;
        assert_eq!(out.last().unwrap()["type"], "response.completed");
        f.request().await;
        f.disconnected().await;
    }
}

#[tokio::test]
async fn cancelled_projected_queue_never_delivers_history_or_tools() {
    let n = native(mixed());
    let f = Fixture::start(vec![Reply::stream(sse(&full_chunks(&n)))]).await;
    let (b, _) = super::super::super::broker(Some(KEY));
    let p = history_provider(&f, b, limits());
    let token = CancellationToken::default();
    let mut stream = p
        .stream_response(
            canonical(advanced_wire(true)),
            RequestContext {
                cancellation: token.clone(),
                ..RequestContext::default()
            },
        )
        .await
        .unwrap()
        .events;
    assert!(matches!(
        stream.next().await.unwrap().unwrap(),
        ProviderStreamEvent::Model(_)
    ));
    token.cancel();
    assert_eq!(stream.next().await.unwrap().err().unwrap().http_status, 503);
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn gateway_history_keeps_native_key_and_runtime_identity_local() {
    let n = native(vec![custom_call(Some("functions"), "a")]);
    let mut f = Fixture::start(vec![Reply::json(n.clone())]).await;
    let (b, _) = super::super::super::broker(Some(KEY));
    let p = Arc::new(history_provider(&f, b.clone(), limits()).with_runtime_context());
    let gateway = caidex_model_gateway::start_with_provider(p, b.redactor(), limits())
        .await
        .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let r = client
        .post(format!("http://{}/v1/responses", gateway.address()))
        .bearer_auth(gateway.token().expose())
        .header("x-client-request-id", "executor-private")
        .header("content-type", "application/json")
        .body(advanced_wire(false).to_string())
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
    let captured = f.request().await;
    assert!(!captured.headers.contains("executor-private"));
    assert!(!captured.headers.contains(gateway.token().expose()));
    assert!(captured.headers.contains(KEY));
    assert!(!response.wire().to_string().contains(KEY));
    gateway.shutdown().await.unwrap();
}

#[tokio::test]
async fn route_native_model_backend_and_capability_binding_are_exact() {
    let f = Fixture::start(vec![Reply::json(native(vec![]))]).await;
    let (b, reads) = super::super::super::broker(Some(KEY));
    let p = history_provider(&f, b.clone(), limits());
    let s = advanced_wire(false);
    let a = p
        .create_response(canonical(s.clone()), RequestContext::default())
        .await
        .unwrap()
        .response;
    let base = replay(&s, &a, &[]);
    for mode in 0..4 {
        let mut m = model(
            if mode == 0 { "other" } else { "fixture" },
            if mode == 1 { "other" } else { "native-fixture" },
        );
        if mode == 3 {
            m.capabilities.native_tools = CapabilitySupport::Supported;
        }
        let route = m.id.clone();
        let q = OpenRouterProvider::new(
            OpenRouterConfig::new(reference())
                .unwrap()
                .with_base_url(&f.base)
                .unwrap(),
            vec![m],
            b.clone(),
            limits(),
        )
        .unwrap()
        .with_backend_selection(
            route.clone(),
            if mode == 2 {
                "other".into()
            } else {
                "fixture-backend/region".into()
            },
        )
        .unwrap()
        .with_native_tools(route.clone())
        .unwrap()
        .with_advanced_tools(route.clone())
        .unwrap()
        .with_native_history(route.clone())
        .unwrap();
        let mut source = base.clone();
        source["model"] = route.into();
        assert!(
            q.create_response(canonical(source), RequestContext::default())
                .await
                .is_err(),
            "{mode}"
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn heartbeat_passes_during_history_buffering_and_partial_failed_calls_reject() {
    let n = native(vec![]);
    let good = full_chunks(&n);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            headers.push(socket.read_u8().await.unwrap());
        }
        let headers = String::from_utf8(headers).unwrap();
        let length: usize = headers
            .lines()
            .find_map(|line| {
                let (k, v) = line.split_once(':')?;
                k.eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse().unwrap())
            })
            .unwrap();
        socket.read_exact(&mut vec![0; length]).await.unwrap();
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
        tokio::time::sleep(Duration::from_millis(1100)).await;
        for part in [": heartbeat\n\n".to_string(), sse(&good)] {
            socket
                .write_all(format!("{:x}\r\n{part}\r\n", part.len()).as_bytes())
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        socket.write_all(b"0\r\n\r\n").await.unwrap();
    });
    let (b, _) = super::super::super::broker(Some(KEY));
    let p = OpenRouterProvider::new(
        OpenRouterConfig::new(reference())
            .unwrap()
            .with_base_url(&base)
            .unwrap(),
        vec![model("fixture", "native-fixture")],
        b,
        limits(),
    )
    .unwrap()
    .with_backend_selection("fixture".into(), "fixture-backend/region".into())
    .unwrap()
    .with_native_tools("fixture".into())
    .unwrap()
    .with_advanced_tools("fixture".into())
    .unwrap()
    .with_native_history("fixture".into())
    .unwrap();
    let mut stream = p
        .stream_response(canonical(advanced_wire(true)), RequestContext::default())
        .await
        .unwrap()
        .events;
    assert!(matches!(
        stream.next().await.unwrap().unwrap(),
        ProviderStreamEvent::Heartbeat
    ));
    while let Some(event) = stream.next().await {
        event.unwrap();
    }
    server.await.unwrap();
    let n = native(vec![call("exec", "a")]);
    let mut e = full_chunks(&n);
    e.truncate(2);
    let mut failed = native(vec![]);
    failed["status"] = "failed".into();
    e.push(json!({"type":"response.failed","response":failed}));
    let f = Fixture::start(vec![Reply::stream(sse(&e))]).await;
    let (b, _) = super::super::super::broker(Some(KEY));
    let p = history_provider(&f, b, limits());
    let mut stream = p
        .stream_response(canonical(advanced_wire(true)), RequestContext::default())
        .await
        .unwrap()
        .events;
    assert!(stream.next().await.unwrap().is_err());
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn history_does_not_enable_summary_include_lite_or_hosted_execution() {
    let f = Fixture::start(vec![]).await;
    let (b, reads) = super::super::super::broker(Some(KEY));
    let p = history_provider(&f, b, limits()).with_runtime_context();
    for (key, value) in [
        ("reasoning", json!({"summary":"auto"})),
        ("include", json!(["reasoning.encrypted_content"])),
        ("context", json!("all_turns")),
        ("tools", json!([{"type":"web_search"}])),
    ] {
        let mut s = advanced_wire(false);
        s[key] = value;
        assert!(
            p.create_response(canonical(s), runtime_context())
                .await
                .is_err()
        );
    }
    assert!(
        p.create_response(
            CanonicalRequest::new(
                json!({"model":"fixture","input":[]}),
                ResponsesDialect::Lite
            )
            .unwrap(),
            runtime_context()
        )
        .await
        .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}
