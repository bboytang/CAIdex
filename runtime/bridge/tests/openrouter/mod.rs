//! Real pinned OpenRouter adapter integration; synthetic model replies only.
use super::*;

#[tokio::test]
#[ignore = "requires pinned Codex; native OpenRouter Classic approval, temp command, disk resume"]
async fn classic_executes_tool_and_replays_after_restart() {
    real_history("gateway-openrouter-tools-classic").await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; native OpenRouter Lite Code Mode approval, temp command, disk resume"]
async fn lite_executes_tool_and_replays_after_restart() {
    real_history("gateway-openrouter-tools-lite").await;
}

async fn real_history(mode: &str) {
    let lite = mode.ends_with("-lite");
    let prefix = if lite {
        "caidex.openrouter.native-history.v2:"
    } else {
        "caidex.openrouter.native-history.v1:"
    };
    let mut harness = Harness::start(mode).await;
    let thread = harness.create_thread().await;
    let marker = harness.directory.0.join("project/caidex-native-marker.txt");
    let mut approved = false;
    for turn in 0..2 {
        if turn == 1 {
            // This existing helper only restarts/resumes the isolated app-server.
            restart_google_runtime(&mut harness, &thread).await;
        }
        let client = harness.runtime.client();
        client.start_turn(&thread,vec![json!({"type":"text","text":"Offline OpenRouter fixture; resume exact disk history on second turn"})],json!({}),DEADLINE).await.unwrap();
        let mut visible = String::new();
        loop {
            match harness.next().await {
                RuntimeEvent::Interaction(request) => {
                    assert!(
                        turn == 0 && !approved,
                        "unexpected/repeated execution: {}",
                        request.event.raw
                    );
                    assert_eq!(request.kind, InteractionKind::CommandApproval);
                    assert!(!marker.exists(), "command executed before approval");
                    client
                        .decide_approval(&request.id, ApprovalDecision::Accept)
                        .await
                        .unwrap();
                    approved = true;
                }
                RuntimeEvent::Notification(event) if event.method == "item/agentMessage/delta" => {
                    visible.push_str(event.raw["params"]["delta"].as_str().unwrap());
                }
                RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                    assert_eq!(
                        event.raw["params"]["turn"]["status"], "completed",
                        "{}",
                        event.raw
                    );
                    break;
                }
                _ => (),
            }
        }
        assert!(approved, "must reach the actual Runtime approval");
        assert_eq!(visible, "CAIdex local fixture complete");
        if turn == 0 {
            assert_eq!(
                std::fs::read_to_string(&marker).unwrap().trim(),
                "CAIDEX_NATIVE_OPENROUTER"
            );
            std::fs::remove_file(&marker).unwrap();
        } else {
            assert!(!marker.exists(), "resume repeated the completed write");
        }
    }
    let trace = harness.trace();
    assert_eq!(trace["requests"], 3);
    assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 3);
    assert_eq!(trace["gatewayCredentialMatched"], true);
    assert_eq!(trace["liteHeaders"], json!([null, null, null]));
    let requests = trace["nativeRequests"].as_array().unwrap();
    for request in requests {
        assert_eq!(request["model"], "native-fixture");
        assert!(
            request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .all(|tool| matches!(
                    tool["type"].as_str(),
                    Some("function" | "custom" | "namespace")
                ))
        );
        assert_eq!(
            request["provider"],
            json!({"only":["fixture-backend/region"],"require_parameters":true,"allow_fallbacks":false})
        );
        assert_eq!(request["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(request["reasoning"]["summary"], "auto");
        assert_eq!(
            request["reasoning"]["context"],
            if lite {
                json!("all_turns")
            } else {
                Value::Null
            }
        );
        assert_eq!(request["parallel_tool_calls"], !lite);
        assert_eq!(
            request["reasoning"]["effort"],
            if lite { "low" } else { "high" }
        );
        assert_eq!(request["tools"], requests[0]["tools"]);
        assert_eq!(request["instructions"], requests[0]["instructions"]);
        for key in ["client_metadata", "prompt_cache_key"] {
            assert!(request.get(key).is_none());
        }
        assert!(!request.to_string().contains(prefix));
        assert!(
            request["input"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["type"] != "additional_tools")
        );
    }
    for turn in 0..2 {
        let previous = requests[turn]["input"].as_array().unwrap();
        let next = requests[turn + 1]["input"].as_array().unwrap();
        assert_eq!(&next[..previous.len()], previous);
        let raw = trace["nativeResponses"][turn]["output"].as_array().unwrap();
        assert_eq!(&next[previous.len()..previous.len() + raw.len()], raw);
    }
    let raw_call = &trace["nativeResponses"][0]["output"][1];
    let result = requests[1]["input"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["type"]
                == if lite {
                    "custom_tool_call_output"
                } else {
                    "function_call_output"
                }
                && item["call_id"] == "openrouter-tool-one"
        })
        .unwrap();
    assert!(!result["output"].is_null());
    let read = harness
        .runtime
        .client()
        .read_thread(&thread, true, DEADLINE)
        .await
        .unwrap();
    let rollout = std::fs::read_to_string(read["thread"]["path"].as_str().unwrap()).unwrap();
    let items: Vec<Value> = rollout
        .lines()
        .filter_map(|line| {
            let e: Value = serde_json::from_str(line).unwrap();
            (e["type"] == "response_item").then(|| e["payload"].clone())
        })
        .collect();
    let call = items
        .iter()
        .find(|item| {
            item["type"]
                == if lite {
                    "custom_tool_call"
                } else {
                    "function_call"
                }
                && item["call_id"] == "openrouter-tool-one"
        })
        .unwrap();
    if lite {
        assert_eq!(call["namespace"], "functions");
        assert_eq!(call["name"], "exec");
        assert_eq!(call["input"], raw_call["input"]);
    } else {
        assert_eq!(call["name"], "exec_command");
        assert_eq!(call["arguments"], raw_call["arguments"]);
    }
    let saved = items
        .iter()
        .find(|item| {
            item["type"]
                == if lite {
                    "custom_tool_call_output"
                } else {
                    "function_call_output"
                }
                && item["call_id"] == "openrouter-tool-one"
        })
        .unwrap();
    assert_eq!(saved["output"], result["output"]);
    let records: Vec<Value> = items
        .iter()
        .filter_map(|item| {
            let c = item["encrypted_content"].as_str()?.strip_prefix(prefix)?;
            Some(serde_json::from_str(c).unwrap())
        })
        .collect();
    assert_eq!(records.len(), 3);
    for (index, (record, native)) in records
        .iter()
        .zip(trace["nativeResponses"].as_array().unwrap())
        .enumerate()
    {
        assert_eq!(record["response"], *native);
        assert_eq!(record["request"], requests[index]);
        assert_eq!(record["chunks"], trace["nativeChunks"][index]);
        if lite {
            let prefix = &record["policy"]["lite"]["additional_tools"];
            assert_eq!(prefix["tools"], requests[0]["tools"]);
            assert!(prefix["id"].as_str().unwrap().starts_with("at_"));
            assert_eq!(prefix, &records[0]["policy"]["lite"]["additional_tools"]);
        } else {
            assert!(record["policy"].get("lite").is_none());
        }
        assert_eq!(record["request"]["model"], "native-fixture");
        assert_eq!(record["provider"], "openrouter");
        assert_eq!(record["version"], if lite { 2 } else { 1 });
    }
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; OpenRouter default and incomplete policies must refuse before Key/POST"]
async fn rejects_defaults_and_partial_policies_before_authentication() {
    for (mode, code) in [
        (
            "gateway-openrouter-classic",
            "openrouter_unsupported_context",
        ),
        (
            "gateway-openrouter-context-classic",
            "openrouter_unsupported_request",
        ),
        (
            "gateway-openrouter-native-tools-classic",
            "openrouter_invalid_tools",
        ),
        ("gateway-openrouter-lite", "unsupported_dialect"),
    ] {
        let mut harness = Harness::start(mode).await;
        let thread = harness.create_thread().await;
        harness
            .runtime
            .client()
            .start_turn(
                &thread,
                vec![json!({"type":"text","text":"Offline native OpenRouter defaults fixture"})],
                json!({}),
                DEADLINE,
            )
            .await
            .unwrap();
        loop {
            match harness.next().await {
                RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                    assert_eq!(event.raw["params"]["turn"]["status"], "failed");
                    assert!(
                        event.raw["params"]["turn"]["error"]["message"]
                            .as_str()
                            .unwrap()
                            .contains(code),
                        "{}",
                        event.raw
                    );
                    break;
                }
                RuntimeEvent::Interaction(request) => panic!(
                    "unsupported native request executed a tool: {}",
                    request.event.raw
                ),
                _ => {}
            }
        }
        assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 0);
        assert_eq!(harness.trace()["requests"], 0);
        harness.shutdown().await;
    }
}

#[tokio::test]
#[ignore = "requires pinned Codex; OpenRouter Lite multi-call rejection before execution/carrier"]
async fn lite_rejects_multiple_calls_before_execution() {
    let mut harness = Harness::start("gateway-openrouter-multi-lite").await;
    let thread = harness.create_thread().await;
    harness
        .runtime
        .client()
        .start_turn(
            &thread,
            vec![
                json!({"type":"text","text":"Offline OpenRouter multiple-call rejection fixture"}),
            ],
            json!({}),
            DEADLINE,
        )
        .await
        .unwrap();
    loop {
        match harness.next().await {
            RuntimeEvent::Interaction(request) => panic!(
                "rejected native generation requested approval: {}",
                request.event.raw
            ),
            RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                assert_eq!(event.raw["params"]["turn"]["status"], "failed");
                assert!(
                    event.raw["params"]["turn"]["error"]["message"]
                        .as_str()
                        .unwrap()
                        .contains("openrouter_invalid_native_tools"),
                    "{}",
                    event.raw
                );
                break;
            }
            _ => (),
        }
    }
    let trace = harness.trace();
    assert_eq!(trace["requests"], 1);
    assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 1);
    let calls: Vec<_> = trace["nativeResponses"][0]["output"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            matches!(
                item["type"].as_str(),
                Some("function_call" | "custom_tool_call")
            )
        })
        .collect();
    assert_eq!(calls.len(), 2);
    assert_ne!(calls[0]["call_id"], calls[1]["call_id"]);
    assert!(
        !harness
            .directory
            .0
            .join("project/caidex-native-marker.txt")
            .exists()
    );
    let read = harness
        .runtime
        .client()
        .read_thread(&thread, true, DEADLINE)
        .await
        .unwrap();
    let rollout = std::fs::read_to_string(read["thread"]["path"].as_str().unwrap()).unwrap();
    for line in rollout.lines() {
        let entry: Value = serde_json::from_str(line).unwrap();
        if entry["type"] == "response_item" {
            assert!(!matches!(
                entry["payload"]["type"].as_str(),
                Some("custom_tool_call" | "function_call" | "reasoning")
            ));
        }
    }
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; OpenRouter Classic/Lite interrupt closes native sockets"]
async fn interrupt_closes_native_sockets() {
    for mode in [
        "gateway-openrouter-stall-classic",
        "gateway-openrouter-stall-lite",
    ] {
        let mut harness = Harness::start(mode).await;
        let thread = harness.create_thread().await;
        let client = harness.runtime.client();
        let turn = client
            .start_turn(
                &thread,
                vec![json!({"type":"text","text":"Offline OpenRouter partial tool stream cancellation"})],
                json!({}),
                DEADLINE,
            )
            .await
            .unwrap();
        let streaming = harness.directory.0.join("gateway-streaming");
        tokio::time::timeout(DEADLINE, async {
            while !streaming.exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let trace = harness.trace();
        assert_eq!(trace["gatewayCredentialMatched"], true);
        let chunks = trace["nativeChunks"][0].as_array().unwrap();
        assert!(chunks.iter().any(|c| matches!(
            c["type"].as_str(),
            Some("response.function_call_arguments.done" | "response.custom_tool_call_input.done")
        )));
        assert!(chunks.iter().all(|c| c["type"] != "response.completed"));
        client
            .interrupt_turn(&thread, turn["turn"]["id"].as_str().unwrap(), DEADLINE)
            .await
            .unwrap();
        loop {
            match harness.next().await {
                RuntimeEvent::Interaction(request) => {
                    panic!("partial stream requested execution: {}", request.event.raw)
                }
                RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                    assert_eq!(event.raw["params"]["turn"]["status"], "interrupted");
                    break;
                }
                _ => (),
            }
        }
        let disconnected = harness.directory.0.join("gateway-disconnected");
        tokio::time::timeout(DEADLINE, async {
            while !disconnected.exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(harness.trace()["gatewayDisconnected"], true);
        assert_eq!(harness.trace()["requests"], 1);
        assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 1);
        assert!(
            !harness
                .directory
                .0
                .join("project/caidex-native-marker.txt")
                .exists()
        );
        let read = client.read_thread(&thread, true, DEADLINE).await.unwrap();
        let rollout = std::fs::read_to_string(read["thread"]["path"].as_str().unwrap()).unwrap();
        for line in rollout.lines() {
            let entry: Value = serde_json::from_str(line).unwrap();
            if entry["type"] == "response_item" {
                assert!(!matches!(
                    entry["payload"]["type"].as_str(),
                    Some("custom_tool_call" | "function_call")
                ));
                assert!(
                    !entry["payload"]["encrypted_content"]
                        .as_str()
                        .is_some_and(|s| s.starts_with("caidex.openrouter.native-history"))
                );
            }
        }
        harness.shutdown().await;
    }
}

#[tokio::test]
#[ignore = "requires pinned Codex; OpenRouter Classic/Lite pending approval cancellation"]
async fn interrupt_waiting_approval_prevents_execution() {
    for mode in [
        "gateway-openrouter-tools-classic",
        "gateway-openrouter-tools-lite",
    ] {
        interrupt_waiting_approval(mode).await;
    }
}

async fn interrupt_waiting_approval(mode: &str) {
    let mut harness = Harness::start(mode).await;
    let thread = harness.create_thread().await;
    let client = harness.runtime.client();
    let turn = client
        .start_turn(
            &thread,
            vec![
                json!({"type":"text","text":"Offline OpenRouter cancel pending approval fixture"}),
            ],
            json!({}),
            DEADLINE,
        )
        .await
        .unwrap();
    let request_id = loop {
        match harness.next().await {
            RuntimeEvent::Interaction(request) => {
                assert_eq!(request.kind, InteractionKind::CommandApproval);
                break request.id;
            }
            RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                panic!("turn ended before actual approval: {}", event.raw)
            }
            _ => (),
        }
    };
    assert!(
        !harness
            .directory
            .0
            .join("project/caidex-native-marker.txt")
            .exists()
    );
    client
        .interrupt_turn(&thread, turn["turn"]["id"].as_str().unwrap(), DEADLINE)
        .await
        .unwrap();
    loop {
        if let RuntimeEvent::Notification(event) = harness.next().await
            && event.method == "turn/completed"
        {
            assert_eq!(event.raw["params"]["turn"]["status"], "interrupted");
            break;
        }
    }
    assert!(matches!(
        client
            .decide_approval(&request_id, ApprovalDecision::Accept)
            .await,
        Err(Error::NotPending)
    ));
    assert!(
        !harness
            .directory
            .0
            .join("project/caidex-native-marker.txt")
            .exists()
    );
    assert_eq!(harness.trace()["requests"], 1);
    assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 1);
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; OpenRouter Classic/Lite available approval cancellation prevents real tool side effects"]
async fn approval_cancel_prevents_execution() {
    for mode in [
        "gateway-openrouter-tools-classic",
        "gateway-openrouter-tools-lite",
    ] {
        let mut harness = Harness::start(mode).await;
        let thread = harness.create_thread().await;
        let client = harness.runtime.client();
        client
            .start_turn(
                &thread,
                vec![json!({"type":"text","text":"Offline OpenRouter approval cancellation fixture"})],
                json!({}),
                DEADLINE,
            )
            .await
            .unwrap();
        let mut cancelled = false;
        loop {
            match harness.next().await {
                RuntimeEvent::Interaction(request) => {
                    assert!(!cancelled);
                    assert_eq!(request.kind, InteractionKind::CommandApproval);
                    assert!(
                        !harness
                            .directory
                            .0
                            .join("project/caidex-native-marker.txt")
                            .exists()
                    );
                    let allowed = request.event.raw["params"]["availableDecisions"]
                        .as_array()
                        .unwrap();
                    assert!(allowed.contains(&json!("cancel")));
                    assert!(!allowed.contains(&json!("decline")));
                    assert!(matches!(
                        client
                            .decide_approval(&request.id, ApprovalDecision::Decline)
                            .await,
                        Err(Error::Protocol(_))
                    ));
                    client
                        .decide_approval(&request.id, ApprovalDecision::Cancel)
                        .await
                        .unwrap();
                    cancelled = true;
                }
                RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                    assert_eq!(
                        event.raw["params"]["turn"]["status"], "interrupted",
                        "{}",
                        event.raw
                    );
                    break;
                }
                _ => (),
            }
        }
        assert!(cancelled);
        assert!(
            !harness
                .directory
                .0
                .join("project/caidex-native-marker.txt")
                .exists()
        );
        let trace = harness.trace();
        assert_eq!(trace["requests"], 1);
        assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 1);
        harness.shutdown().await;
    }
}
