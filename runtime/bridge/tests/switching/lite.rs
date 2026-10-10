//! Synthetic OpenAI Lite route combinations, not commercial compatibility.
use super::*;

fn assert_lite_requests(trace: &Value, models: &[&str]) {
    let requests = trace["wireRequests"].as_array().unwrap();
    assert_eq!(requests.len(), models.len());
    for (request, model) in requests.iter().zip(models) {
        assert_eq!(request["body"]["model"], *model);
        assert_eq!(request["liteHeader"], "true");
        assert!(request["body"].get("tools").is_none());
        assert!(request["body"].get("instructions").is_none());
        let developer = &request["body"]["input"][0];
        assert_eq!(developer["role"], "developer");
        assert_eq!(developer["type"], "additional_tools");
        assert!(developer["id"].as_str().unwrap().starts_with("at_"));
        let tools = developer["tools"].as_array().unwrap();
        assert!(!tools.is_empty());
    }
}

#[tokio::test]
#[ignore = "requires pinned Codex; Lite model override, disk resume and linked fork isolation"]
async fn lite_model_switch_disk_resume_and_fork_keep_dialect_and_parent_history() {
    let mut harness = Harness::start("gateway-model-switching-lite").await;
    let parent = harness.create_thread().await;
    for (text, options) in [
        ("CAIDEX_LITE_FIRST", json!({})),
        ("CAIDEX_LITE_SECOND", json!({"model":"gpt-6-sol"})),
    ] {
        assert_eq!(
            turn(&mut harness, &parent, text, options).await["status"],
            "completed"
        );
    }
    restart_google_runtime(&mut harness, &parent).await;
    assert_eq!(
        turn(&mut harness, &parent, "CAIDEX_LITE_RESUMED", json!({})).await["status"],
        "completed"
    );
    let result = harness
        .runtime
        .client()
        .fork_thread(&parent, json!({"model":"gpt-6.1-sol"}), DEADLINE)
        .await
        .unwrap();
    let branch = result["thread"]["id"].as_str().unwrap().to_owned();
    assert_ne!(branch, parent);
    assert_eq!(result["thread"]["forkedFromId"], parent);
    assert_eq!(harness.trace()["requests"], 3, "fork must not infer");
    assert_eq!(
        turn(&mut harness, &branch, "CAIDEX_LITE_BRANCH_ONLY", json!({})).await["status"],
        "completed"
    );
    assert_eq!(
        turn(&mut harness, &parent, "CAIDEX_LITE_PARENT_ONLY", json!({})).await["status"],
        "completed"
    );
    let trace = harness.trace();
    assert_lite_requests(
        &trace,
        &[
            "native-switch-0",
            "native-switch-1",
            "native-switch-1",
            "native-switch-0",
            "native-switch-1",
        ],
    );
    let requests = trace["wireRequests"].as_array().unwrap();
    let resumed = requests[2]["body"]["input"].to_string();
    assert!(resumed.contains("CAIDEX_LITE_FIRST") && resumed.contains("CAIDEX_LITE_SECOND"));
    assert!(resumed.contains("CAIDEX_OPAQUE_REASONING+/=="));
    assert!(
        requests[3]["body"]["input"]
            .to_string()
            .contains("CAIDEX_LITE_RESUMED")
    );
    assert!(
        !requests[4]["body"]["input"]
            .to_string()
            .contains("CAIDEX_LITE_BRANCH_ONLY")
    );
    assert_eq!(
        requests[1]["body"]["input"][0],
        requests[2]["body"]["input"][0]
    );
    let client = harness.runtime.client();
    for id in [&parent, &branch] {
        assert_eq!(
            client.read_thread(id, true, DEADLINE).await.unwrap()["thread"]["turns"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }
    assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 5);
    assert_eq!(trace["gatewayCredentialMatched"], true);
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; Lite route rejects unknown model and explicit recovery retains Lite"]
async fn lite_unknown_model_rejects_before_credentials_and_explicit_recovery_keeps_lite() {
    let mut harness = Harness::start("gateway-model-switching-lite").await;
    let thread = harness.create_thread().await;
    assert_eq!(
        turn(
            &mut harness,
            &thread,
            "CAIDEX_LITE_UNKNOWN",
            json!({"model":"caidex-unregistered-lite-fixture"})
        )
        .await["status"],
        "failed"
    );
    assert_eq!(harness.trace()["requests"], 0);
    assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 0);
    assert_eq!(
        turn(
            &mut harness,
            &thread,
            "CAIDEX_LITE_RECOVER",
            json!({"model":"gpt-6.1-sol"})
        )
        .await["status"],
        "completed"
    );
    assert_lite_requests(&harness.trace(), &["native-switch-0"]);
    assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 1);
    harness.shutdown().await;
}
