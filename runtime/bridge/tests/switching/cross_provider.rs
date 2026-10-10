//! Explicit visible-text handoff only; Host association/storage is not implemented here.
use super::*;

async fn source() -> (Harness, String, Value) {
    let mut harness = Harness::start("gateway-model-switching").await;
    let thread = harness.create_thread().await;
    assert_eq!(
        turn(
            &mut harness,
            &thread,
            "CAIDEX_SOURCE_VISIBLE_INPUT",
            json!({})
        )
        .await["status"],
        "completed"
    );
    let read = harness
        .runtime
        .client()
        .read_thread(&thread, true, DEADLINE)
        .await
        .unwrap();
    (harness, thread, read)
}

#[tokio::test]
#[ignore = "requires pinned Codex; explicit OpenAI visible-text handoff to a fresh OpenRouter thread"]
async fn fresh_provider_thread_receives_visible_text_without_source_opaque_history() {
    let (mut source, source_id, source_read) = source().await;
    let visible: Vec<&str> = source_read["thread"]["turns"][0]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["type"] == "agentMessage")
        .map(|item| item["text"].as_str().unwrap())
        .collect();
    assert_eq!(visible, ["CAIdex local fixture complete"]);
    let handoff = format!(
        "Explicit offline handoff from source thread {source_id}; visible assistant text: {}",
        visible.join("\n")
    );
    let mut target = Harness::start("gateway-openrouter-tools-classic").await;
    let target_id = target.create_thread().await;
    assert_ne!(target_id, source_id);
    assert_eq!(target.trace()["requests"], 0);
    assert_eq!(target.credential_reads.load(Ordering::SeqCst), 0);
    let marker = target.directory.0.join("project/caidex-native-marker.txt");
    let client = target.runtime.client();
    client
        .start_turn(
            &target_id,
            vec![json!({"type":"text","text":handoff})],
            json!({}),
            DEADLINE,
        )
        .await
        .unwrap();
    let mut approved = false;
    loop {
        match target.next().await {
            RuntimeEvent::Interaction(request) => {
                assert!(!approved && !marker.exists());
                assert_eq!(request.kind, InteractionKind::CommandApproval);
                client
                    .decide_approval(&request.id, ApprovalDecision::Accept)
                    .await
                    .unwrap();
                approved = true;
            }
            RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                assert_eq!(event.raw["params"]["turn"]["status"], "completed");
                break;
            }
            _ => (),
        }
    }
    assert!(approved);
    assert_eq!(
        std::fs::read_to_string(&marker).unwrap().trim(),
        "CAIDEX_NATIVE_OPENROUTER"
    );
    assert!(
        !source
            .directory
            .0
            .join("project/caidex-native-marker.txt")
            .exists()
    );
    std::fs::remove_file(&marker).unwrap();
    restart_google_runtime(&mut target, &target_id).await;
    assert_eq!(
        turn(
            &mut target,
            &target_id,
            "CAIDEX_TARGET_DISK_CONTINUATION",
            json!({})
        )
        .await["status"],
        "completed"
    );
    assert!(!marker.exists(), "restart must not repeat execution");
    let trace = target.trace();
    assert_eq!(trace["requests"], 3);
    let wire = trace["nativeRequests"].to_string();
    assert!(wire.contains(&source_id) && wire.contains("CAIdex local fixture complete"));
    assert!(!wire.contains("CAIDEX_OPAQUE_REASONING+/=="));
    assert!(!wire.contains("CAIDEX_FUTURE_SIGNATURE=="));
    assert!(!wire.contains("CAIDEX_SOURCE_VISIBLE_INPUT"));
    let read = target
        .runtime
        .client()
        .read_thread(&target_id, true, DEADLINE)
        .await
        .unwrap();
    assert!(
        read["thread"]["forkedFromId"].is_null(),
        "fresh thread is not a raw fork"
    );
    assert_eq!(read["thread"]["turns"].as_array().unwrap().len(), 2);
    assert!(read.to_string().contains(&source_id));
    assert_eq!(
        source
            .runtime
            .client()
            .read_thread(&source_id, true, DEADLINE)
            .await
            .unwrap()["thread"]["turns"],
        source_read["thread"]["turns"]
    );
    assert_eq!(source.trace()["requests"], 1);
    assert_eq!(source.credential_reads.load(Ordering::SeqCst), 1);
    assert_eq!(target.credential_reads.load(Ordering::SeqCst), 3);
    target.shutdown().await;
    source.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; target Gateway rejects actual source disk reasoning before credentials"]
async fn foreign_provider_disk_reasoning_is_rejected_before_target_key_or_inference() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (mut source, _, read) = source().await;
    let path = PathBuf::from(read["thread"]["path"].as_str().unwrap());
    assert!(
        path.canonicalize()
            .unwrap()
            .starts_with(source.directory.0.join("data").canonicalize().unwrap())
    );
    let rollout = std::fs::read_to_string(path).unwrap();
    let reasoning = rollout
        .lines()
        .filter_map(|line| {
            let item: Value = serde_json::from_str(line).unwrap();
            (item["type"] == "response_item" && item["payload"]["type"] == "reasoning")
                .then(|| item["payload"].clone())
        })
        .next()
        .unwrap();
    assert_eq!(
        reasoning["encrypted_content"],
        "CAIDEX_OPAQUE_REASONING+/=="
    );
    let mut target = Harness::start("gateway-openrouter-tools-classic").await;
    let gateway = target.gateway.as_ref().unwrap();
    let body =
        json!({"model":"caidex-openrouter-classic-fixture","input":[reasoning],"stream":false})
            .to_string();
    let mut socket = tokio::net::TcpStream::connect(gateway.address())
        .await
        .unwrap();
    let request = format!(
        "POST /v1/responses HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        gateway.address(),
        gateway.token().expose(),
        body.len(),
        body
    );
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(DEADLINE, socket.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    let response = String::from_utf8(response).unwrap();
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    assert!(!response.contains("CAIDEX_OPAQUE_REASONING+/=="));
    assert!(!response.contains("CAIDEX_GATEWAY_PROVIDER_TEST_KEY"));
    assert_eq!(target.credential_reads.load(Ordering::SeqCst), 0);
    assert_eq!(target.trace()["requests"], 0);
    assert_eq!(source.trace()["requests"], 1);
    target.shutdown().await;
    source.shutdown().await;
}
