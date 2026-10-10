//! Fixed Runtime process with synthetic Chat Completions, no commercial claims.
use super::*;

#[tokio::test]
#[ignore = "requires pinned Codex; Chat Completions Classic/Lite wire and disk resume"]
async fn classic_and_lite_text_survive_real_runtime_restart() {
    for mode in ["gateway-chat-classic", "gateway-chat-lite"] {
        let mut harness = Harness::start(mode).await;
        let thread = harness.create_thread().await;
        for turn in 0..2 {
            if turn == 1 {
                restart_google_runtime(&mut harness, &thread).await;
            }
            harness
                .runtime
                .client()
                .start_turn(
                    &thread,
                    vec![json!({"type":"text","text":"Offline Chat Completions text fixture"})],
                    json!({}),
                    DEADLINE,
                )
                .await
                .unwrap();
            let mut visible = String::new();
            loop {
                match harness.next().await {
                    RuntimeEvent::Interaction(request) => {
                        panic!("unexpected interaction {}", request.event.raw)
                    }
                    RuntimeEvent::Notification(event)
                        if event.method == "item/agentMessage/delta" =>
                    {
                        visible.push_str(event.raw["params"]["delta"].as_str().unwrap())
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
            assert_eq!(visible, "CAIdex local fixture complete");
        }
        let trace = harness.trace();
        assert_eq!(trace["requests"], 2);
        assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 2);
        assert_eq!(
            trace["nativeRequests"][1]["messages"][trace["nativeRequests"][0]["messages"]
                .as_array()
                .unwrap()
                .len()]["role"],
            "assistant"
        );
        harness.shutdown().await;
    }
}

#[tokio::test]
#[ignore = "requires pinned Codex; Chat Classic/Lite native approval, temp execution, tool result and disk resume"]
async fn classic_and_lite_tools_use_native_approval_and_do_not_reexecute_after_resume() {
    for mode in ["gateway-chat-tools-classic", "gateway-chat-tools-lite"] {
        let mut harness = Harness::start(mode).await;
        let thread = harness.create_thread().await;
        let marker = harness.directory.0.join("project/caidex-native-marker.txt");
        let mut approved = false;
        for turn in 0..2 {
            if turn == 1 {
                restart_google_runtime(&mut harness, &thread).await;
            }
            harness
                .runtime
                .client()
                .start_turn(
                    &thread,
                    vec![json!({"type":"text","text":"Offline Chat Completions tool fixture"})],
                    json!({}),
                    DEADLINE,
                )
                .await
                .unwrap();
            loop {
                match harness.next().await {
                    RuntimeEvent::Interaction(request) => {
                        assert!(turn == 0 && !approved);
                        assert_eq!(request.kind, InteractionKind::CommandApproval);
                        assert!(!marker.exists());
                        harness
                            .runtime
                            .client()
                            .decide_approval(&request.id, ApprovalDecision::Accept)
                            .await
                            .unwrap();
                        approved = true;
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
            assert!(approved);
            if turn == 0 {
                assert_eq!(
                    std::fs::read_to_string(&marker).unwrap().trim(),
                    "CAIDEX_NATIVE_CHAT"
                );
                std::fs::remove_file(&marker).unwrap();
            } else {
                assert!(!marker.exists(), "disk resume repeated completed command");
            }
        }
        let trace = harness.trace();
        assert_eq!(trace["requests"], 3);
        assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 3);
        let prior = trace["nativeRequests"][1]["messages"].as_array().unwrap();
        assert_eq!(
            &trace["nativeRequests"][2]["messages"].as_array().unwrap()[..prior.len()],
            prior.as_slice()
        );
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
        let output = items
            .iter()
            .find(|item| {
                matches!(
                    item["type"].as_str(),
                    Some("custom_tool_call_output" | "function_call_output")
                ) && item["call_id"] == "chat-tool-one"
            })
            .unwrap();
        let text = if let Some(text) = output["output"].as_str() {
            text.to_owned()
        } else {
            output["output"]
                .as_array()
                .unwrap()
                .iter()
                .map(|part| part["text"].as_str().unwrap())
                .collect::<String>()
        };
        let native_result = prior.iter().find(|m| m["role"] == "tool").unwrap();
        assert_eq!(native_result["content"], text);
        let saved_call = items
            .iter()
            .find(|item| {
                matches!(
                    item["type"].as_str(),
                    Some("custom_tool_call" | "function_call")
                ) && item["call_id"] == "chat-tool-one"
            })
            .unwrap();
        let native_call =
            prior.iter().find(|m| m["tool_calls"].is_array()).unwrap()["tool_calls"][0].clone();
        if mode.ends_with("-lite") {
            let args: Value =
                serde_json::from_str(native_call["function"]["arguments"].as_str().unwrap())
                    .unwrap();
            assert_eq!(args["input"], saved_call["input"]);
            assert_eq!(saved_call["namespace"], "functions");
            assert_eq!(saved_call["name"], "exec");
        } else {
            assert_eq!(
                native_call["function"]["arguments"],
                saved_call["arguments"]
            );
            assert_eq!(saved_call["name"], "exec_command");
        }
        harness.shutdown().await;
    }
}

#[tokio::test]
#[ignore = "requires pinned Codex; Chat Classic/Lite Cancel and pending approval interrupt"]
async fn cancel_and_interrupt_pending_native_approval_prevent_execution() {
    for mode in ["gateway-chat-tools-classic", "gateway-chat-tools-lite"] {
        for cancel in [true, false] {
            let mut harness = Harness::start(mode).await;
            let thread = harness.create_thread().await;
            let turn = harness
                .runtime
                .client()
                .start_turn(
                    &thread,
                    vec![json!({"type":"text","text":"Offline pending approval cancellation"})],
                    json!({}),
                    DEADLINE,
                )
                .await
                .unwrap();
            let request = loop {
                match harness.next().await {
                    RuntimeEvent::Interaction(request) => {
                        assert_eq!(request.kind, InteractionKind::CommandApproval);
                        break request;
                    }
                    RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                        panic!("ended before approval {}", event.raw)
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
            if cancel {
                harness
                    .runtime
                    .client()
                    .decide_approval(&request.id, ApprovalDecision::Cancel)
                    .await
                    .unwrap();
            } else {
                harness
                    .runtime
                    .client()
                    .interrupt_turn(&thread, turn["turn"]["id"].as_str().unwrap(), DEADLINE)
                    .await
                    .unwrap();
            }
            loop {
                if let RuntimeEvent::Notification(event) = harness.next().await
                    && event.method == "turn/completed"
                {
                    assert_eq!(
                        event.raw["params"]["turn"]["status"], "interrupted",
                        "{}",
                        event.raw
                    );
                    break;
                }
            }
            assert!(
                !harness
                    .directory
                    .0
                    .join("project/caidex-native-marker.txt")
                    .exists()
            );
            assert!(
                harness
                    .runtime
                    .client()
                    .decide_approval(&request.id, ApprovalDecision::Accept)
                    .await
                    .is_err()
            );
            assert_eq!(harness.trace()["requests"], 1);
            assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 1);
            harness.shutdown().await;
        }
    }
}
