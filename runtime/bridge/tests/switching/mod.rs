//! Verified offline OpenAI combinations, not commercial model compatibility.
use super::*;
mod cross_provider;

async fn turn(harness: &mut Harness, thread: &str, text: &str, options: Value) -> Value {
    let response = harness
        .runtime
        .client()
        .start_turn(
            thread,
            vec![json!({"type":"text","text":text})],
            options,
            DEADLINE,
        )
        .await
        .unwrap();
    let id = response["turn"]["id"].clone();
    loop {
        match harness.next().await {
            RuntimeEvent::Interaction(event) => {
                panic!("unexpected interaction: {}", event.event.raw)
            }
            RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                assert_eq!(event.raw["params"]["threadId"], thread);
                assert_eq!(event.raw["params"]["turn"]["id"], id);
                return event.raw["params"]["turn"].clone();
            }
            _ => (),
        }
    }
}

#[tokio::test]
#[ignore = "requires pinned Codex; ModelRouter same-provider turn boundary and disk resume"]
async fn same_provider_model_override_persists_across_turns_and_disk_resume() {
    let mut harness = Harness::start("gateway-model-switching").await;
    let thread = harness.create_thread().await;
    assert_eq!(
        turn(&mut harness, &thread, "CAIDEX_SWITCH_FIRST", json!({})).await["status"],
        "completed"
    );
    assert_eq!(
        turn(
            &mut harness,
            &thread,
            "CAIDEX_SWITCH_SECOND",
            json!({"model":"gpt-5.1-codex"})
        )
        .await["status"],
        "completed"
    );
    restart_google_runtime(&mut harness, &thread).await;
    assert_eq!(
        turn(&mut harness, &thread, "CAIDEX_SWITCH_RESUMED", json!({})).await["status"],
        "completed"
    );
    let trace = harness.trace();
    let requests = trace["wireRequests"].as_array().unwrap();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests
            .iter()
            .map(|r| r["body"]["model"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["native-switch-0", "native-switch-1", "native-switch-1"]
    );
    let restored = requests[2]["body"]["input"].to_string();
    assert!(restored.contains("CAIDEX_SWITCH_FIRST") && restored.contains("CAIDEX_SWITCH_SECOND"));
    assert!(restored.contains("CAIDEX_OPAQUE_REASONING+/=="));
    let read = harness
        .runtime
        .client()
        .read_thread(&thread, true, DEADLINE)
        .await
        .unwrap();
    assert_eq!(read["thread"]["turns"].as_array().unwrap().len(), 3);
    assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 3);
    assert_eq!(trace["gatewayCredentialMatched"], true);
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; same-provider model fork linkage and parent isolation"]
async fn same_provider_fork_model_override_links_branch_without_mutating_parent() {
    let mut harness = Harness::start("gateway-model-switching").await;
    let parent = harness.create_thread().await;
    assert_eq!(
        turn(&mut harness, &parent, "CAIDEX_PARENT_HISTORY", json!({})).await["status"],
        "completed"
    );
    let result = harness
        .runtime
        .client()
        .fork_thread(&parent, json!({"model":"gpt-5.1-codex"}), DEADLINE)
        .await
        .unwrap();
    let branch = result["thread"]["id"].as_str().unwrap().to_owned();
    assert_ne!(branch, parent);
    assert_eq!(result["thread"]["forkedFromId"], parent);
    assert_eq!(harness.trace()["requests"], 1, "fork must not infer");
    assert_eq!(
        turn(&mut harness, &branch, "CAIDEX_BRANCH_ONLY", json!({})).await["status"],
        "completed"
    );
    assert_eq!(
        turn(&mut harness, &parent, "CAIDEX_PARENT_ONLY", json!({})).await["status"],
        "completed"
    );
    let trace = harness.trace();
    let requests = trace["wireRequests"].as_array().unwrap();
    assert_eq!(
        requests
            .iter()
            .map(|r| r["body"]["model"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["native-switch-0", "native-switch-1", "native-switch-0"]
    );
    assert!(
        requests[1]["body"]["input"]
            .to_string()
            .contains("CAIDEX_PARENT_HISTORY")
    );
    assert!(
        !requests[2]["body"]["input"]
            .to_string()
            .contains("CAIDEX_BRANCH_ONLY")
    );
    let client = harness.runtime.client();
    for id in [&parent, &branch] {
        assert_eq!(
            client.read_thread(id, true, DEADLINE).await.unwrap()["thread"]["turns"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; unregistered model fails without inference or credential fallback"]
async fn unknown_model_override_fails_before_key_and_explicit_recovery_keeps_thread() {
    let mut harness = Harness::start("gateway-model-switching").await;
    let thread = harness.create_thread().await;
    let failed = turn(
        &mut harness,
        &thread,
        "CAIDEX_UNKNOWN_MODEL",
        json!({"model":"caidex-unregistered-fixture"}),
    )
    .await;
    assert_eq!(failed["status"], "failed");
    assert_eq!(harness.trace()["requests"], 0);
    assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 0);
    assert_eq!(
        turn(
            &mut harness,
            &thread,
            "CAIDEX_EXPLICIT_RECOVERY",
            json!({"model":"gpt-5.5"})
        )
        .await["status"],
        "completed"
    );
    assert_eq!(harness.trace()["requests"], 1);
    assert_eq!(
        harness.trace()["wireRequests"][0]["body"]["model"],
        "native-switch-0"
    );
    assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 1);
    harness.shutdown().await;
}
