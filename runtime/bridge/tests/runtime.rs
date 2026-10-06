use std::{collections::HashMap, path::PathBuf, time::Duration};

use caidex_runtime::{
    AppServer, ApprovalDecision, ClientOptions, Error, InteractionKind, RequestId, RpcError,
    Runtime, RuntimeEvent, protocol_surface,
};
use serde_json::{Value, json};
use tokio::process::Command;

const DEADLINE: Duration = Duration::from_secs(5);

async fn fixture(experimental: bool, capacity: usize) -> Runtime {
    let python = std::env::var_os("CAIDEX_TEST_PYTHON")
        .unwrap_or_else(|| if cfg!(windows) { "python" } else { "python3" }.into());
    let mut command = Command::new(python);
    command.arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime_peer.py"));
    Runtime::connect(
        AppServer::spawn(command, 256).unwrap(),
        ClientOptions {
            capabilities: json!({"experimentalApi": experimental, "extensions": {"future": {"opaque": true}}}),
            ..Default::default()
        },
        DEADLINE,
        capacity,
    )
    .await
    .unwrap()
}

async fn next(runtime: &mut Runtime) -> RuntimeEvent {
    tokio::time::timeout(DEADLINE, runtime.next_event())
        .await
        .unwrap()
        .unwrap()
}

async fn inject(runtime: &Runtime, events: Value) {
    runtime
        .client()
        .call(
            "config/read",
            Some(json!({"fixtureEvents": events})),
            DEADLINE,
        )
        .await
        .unwrap();
}

#[test]
fn catalog_retains_full_pinned_surface_without_claiming_provider_support() {
    let catalog = protocol_surface();
    assert_eq!(catalog.client_requests.len(), 167);
    assert_eq!(catalog.server_requests.len(), 11);
    assert_eq!(catalog.notifications.len(), 83);
    assert_eq!(
        catalog
            .client_requests
            .values()
            .filter(|m| !m.experimental)
            .count(),
        104
    );
    assert!(!catalog.client_requests["turn/steer"].experimental);
    assert!(catalog.client_requests["thread/realtime/start"].experimental);
}

#[tokio::test]
async fn handshake_preserves_declared_capabilities_and_gates_experimental_methods() {
    let mut runtime = fixture(false, 32).await;
    assert!(!runtime.info().experimental_api_requested);
    let RuntimeEvent::Notification(init) = next(&mut runtime).await else {
        panic!("expected init notification")
    };
    assert_eq!(
        init.raw["params"]["capabilities"]["extensions"]["future"]["opaque"],
        true
    );
    let client = runtime.client();
    for method in ["initialize", "thread/realtime/start", "future/unknown"] {
        assert!(matches!(
            client.call(method, Some(json!({})), DEADLINE).await,
            Err(Error::Unavailable(_))
        ));
    }
    assert!(matches!(
        client.call("turn/start", None, DEADLINE).await,
        Err(Error::Protocol(_))
    ));
    let result = client
        .call("account/rateLimits/read", None, DEADLINE)
        .await
        .unwrap();
    assert!(result["wire"].get("params").is_none());
    runtime.shutdown().await.unwrap();

    let mut runtime = fixture(true, 32).await;
    runtime
        .client()
        .call(
            "thread/realtime/start",
            Some(json!({"threadId": "t"})),
            DEADLINE,
        )
        .await
        .unwrap();
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn thread_turn_steer_and_interrupt_keep_ids_options_and_runtime_events() {
    let mut runtime = fixture(true, 32).await;
    let client = runtime.client();
    let started = client
        .start_thread(json!({"cwd": "/fixture"}), DEADLINE)
        .await
        .unwrap();
    assert_eq!(started["thread"]["extension"]["opaque"], "retain");
    let thread = started["thread"]["id"].as_str().unwrap();
    let resumed = client
        .resume_thread(
            thread,
            json!({"threadId": "wrong", "model": "fixture"}),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(resumed["received"]["threadId"], thread);
    assert_eq!(resumed["received"]["model"], "fixture");
    let forked = client
        .fork_thread(thread, json!({}), DEADLINE)
        .await
        .unwrap();
    assert_ne!(forked["thread"]["id"], thread);
    client.read_thread(thread, false, DEADLINE).await.unwrap();
    let input = vec![
        json!({"type": "text", "text": "fixture"}),
        json!({"type": "image", "fileId": "opaque-image"}),
    ];
    let turn = client
        .start_turn(
            thread,
            input.clone(),
            json!({"threadId": "wrong", "input": [], "outputSchema": {"opaque": true}}),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(turn["received"]["input"], json!(input));
    assert_eq!(turn["received"]["threadId"], thread);
    assert_eq!(turn["received"]["outputSchema"]["opaque"], true);
    assert!(matches!(
        client
            .steer_turn(thread, "stale-turn", vec![], DEADLINE)
            .await,
        Err(Error::Rpc(-32602, _, _))
    ));
    let steered = client
        .steer_turn(
            thread,
            "turn-1",
            vec![json!({"type": "text", "text": "follow-up"})],
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(steered["turnId"], "turn-1");
    client
        .interrupt_turn(thread, "turn-1", DEADLINE)
        .await
        .unwrap();
    let mut methods = vec![];
    while methods.last().is_none_or(|m| m != "turn/completed") {
        let RuntimeEvent::Notification(event) = next(&mut runtime).await else {
            panic!("expected notification")
        };
        if event.method == "item/agentMessage/delta" {
            assert_eq!(event.raw["params"]["opaque"], "retain");
        }
        if event.method == "turn/completed" {
            assert_eq!(event.raw["params"]["turn"]["status"], "interrupted");
        }
        methods.push(event.method);
    }
    assert!(methods.contains(&"turn/started".into()));
    assert!(methods.contains(&"item/agentMessage/delta".into()));
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn approvals_wait_for_explicit_decision_and_only_one_local_response_wins() {
    let mut runtime = fixture(true, 32).await;
    next(&mut runtime).await;
    inject(&runtime, json!([{"id": "approval-1", "method": "item/commandExecution/requestApproval", "params": {"threadId": "t", "turnId": "turn", "itemId": "item", "availableDecisions": ["decline", "cancel"]}, "extension": {"opaque": true}}])).await;
    let RuntimeEvent::Interaction(request) = next(&mut runtime).await else {
        panic!("expected interaction")
    };
    assert_eq!(request.kind, InteractionKind::CommandApproval);
    assert_eq!(request.thread_id(), Some("t"));
    assert_eq!(request.turn_id(), Some("turn"));
    assert_eq!(request.event.raw["extension"]["opaque"], true);
    let client = runtime.client();
    assert_eq!(client.pending_interactions().len(), 1);
    assert!(matches!(
        client
            .decide_approval(&request.id, ApprovalDecision::Accept)
            .await,
        Err(Error::Protocol(_))
    ));
    assert_eq!(client.pending_interactions().len(), 1);
    let (first, second) = tokio::join!(
        client.decide_approval(&request.id, ApprovalDecision::Decline),
        client.decide_approval(&request.id, ApprovalDecision::Cancel)
    );
    assert!(first.is_ok() ^ second.is_ok());
    assert!(matches!(first, Err(Error::NotPending)) || matches!(second, Err(Error::NotPending)));
    let RuntimeEvent::Notification(reply) = next(&mut runtime).await else {
        panic!("expected reply trace")
    };
    assert_eq!(reply.raw["params"]["id"], "approval-1");
    assert!(client.pending_interactions().is_empty());
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn question_answers_use_question_ids_and_reject_wrong_request_kinds() {
    let mut runtime = fixture(true, 32).await;
    next(&mut runtime).await;
    inject(&runtime, json!([{"id": 12, "method": "item/tool/requestUserInput", "params": {"threadId": "t", "turnId": "turn", "itemId": "i", "isBlocking": false, "questions": [{"id": "choice", "header": "Choice", "question": "Choose", "isSecret": true}]}}])).await;
    let RuntimeEvent::Interaction(request) = next(&mut runtime).await else {
        panic!("expected question")
    };
    let client = runtime.client();
    assert!(matches!(
        client
            .decide_approval(&request.id, ApprovalDecision::Accept)
            .await,
        Err(Error::Protocol(_))
    ));
    assert!(matches!(
        client
            .answer_questions(
                &request.id,
                HashMap::from([("wrong".into(), vec!["a".into()])])
            )
            .await,
        Err(Error::Protocol(_))
    ));
    client
        .answer_questions(
            &request.id,
            HashMap::from([("choice".into(), vec!["a".into(), "free text".into()])]),
        )
        .await
        .unwrap();
    let RuntimeEvent::Notification(reply) = next(&mut runtime).await else {
        panic!("expected reply")
    };
    assert_eq!(reply.raw["params"]["id"], 12);
    assert_eq!(
        reply.raw["params"]["result"],
        json!({"answers": {"choice": {"answers": ["a", "free text"]}}})
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn resolution_revokes_pending_request_without_answering_it_again() {
    let mut runtime = fixture(true, 32).await;
    next(&mut runtime).await;
    inject(&runtime, json!([
        {"id": 9, "method": "item/fileChange/requestApproval", "params": {"threadId": "t", "turnId": "turn", "itemId": "i"}},
        {"method": "serverRequest/resolved", "params": {"threadId": "t", "requestId": 9}}
    ])).await;
    next(&mut runtime).await;
    let RuntimeEvent::Notification(resolved) = next(&mut runtime).await else {
        panic!("expected resolution")
    };
    assert_eq!(resolved.method, "serverRequest/resolved");
    assert!(matches!(
        runtime
            .client()
            .decide_approval(&RequestId::Integer(9), ApprovalDecision::Accept)
            .await,
        Err(Error::NotPending)
    ));
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn turn_completion_revokes_only_its_thread_requests_including_legacy_approvals() {
    let mut runtime = fixture(true, 32).await;
    next(&mut runtime).await;
    inject(&runtime, json!([
        {"id": 1, "method": "item/commandExecution/requestApproval", "params": {"threadId": "t1", "turnId": "turn"}},
        {"id": 2, "method": "execCommandApproval", "params": {"conversationId": "t1"}},
        {"id": 3, "method": "item/fileChange/requestApproval", "params": {"threadId": "t2"}},
        {"id": 4, "method": "account/chatgptAuthTokens/refresh", "params": {}},
        {"method": "turn/completed", "params": {"threadId": "t1", "turn": {"id": "turn", "status": "interrupted"}}}
    ])).await;
    for _ in 0..5 {
        next(&mut runtime).await;
    }
    let client = runtime.client();
    for id in [1, 2] {
        assert!(matches!(
            client.reply(&RequestId::Integer(id), Ok(json!({}))).await,
            Err(Error::NotPending)
        ));
    }
    let pending = client.pending_interactions();
    assert_eq!(pending.len(), 2);
    assert!(
        pending
            .iter()
            .any(|request| request.id == RequestId::Integer(3))
    );
    assert!(
        pending
            .iter()
            .any(|request| request.id == RequestId::Integer(4))
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn every_pinned_server_request_and_unknown_request_is_forwarded_without_auto_reply() {
    let mut runtime = fixture(true, 32).await;
    next(&mut runtime).await;
    let methods: Vec<_> = protocol_surface()
        .server_requests
        .keys()
        .chain(std::iter::once(&"future/request".to_owned()))
        .cloned()
        .collect();
    let events: Vec<_> = methods.iter().enumerate().map(|(id, method)| json!({"id": format!("request-{id}"), "method": method, "params": {"opaque": "signature"}})).collect();
    inject(&runtime, json!(events)).await;
    for method in &methods {
        let RuntimeEvent::Interaction(request) = next(&mut runtime).await else {
            panic!("expected request")
        };
        assert_eq!(&request.event.method, method);
        assert_eq!(request.event.raw["params"]["opaque"], "signature");
        assert_eq!(
            request.kind == InteractionKind::Unknown,
            method == "future/request"
        );
    }
    let client = runtime.client();
    assert_eq!(client.pending_interactions().len(), methods.len());
    client
        .reply(
            &RequestId::String("request-11".into()),
            Err(RpcError {
                code: -32601,
                message: "unsupported by this client".into(),
                data: Some(json!({"opaque": true})),
            }),
        )
        .await
        .unwrap();
    let RuntimeEvent::Notification(reply) = next(&mut runtime).await else {
        panic!("expected error reply")
    };
    assert_eq!(reply.raw["params"]["error"]["code"], -32601);
    assert_eq!(reply.raw["params"]["error"]["data"]["opaque"], true);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn duplicate_server_ids_fail_closed_instead_of_overwriting_approval() {
    let mut runtime = fixture(true, 32).await;
    next(&mut runtime).await;
    let error = runtime
        .client()
        .call(
            "config/read",
            Some(json!({"fixtureEvents": [
                {"id": "duplicate", "method": "item/fileChange/requestApproval", "params": {}},
                {"id": "duplicate", "method": "item/tool/requestUserInput", "params": {}}
            ]})),
            DEADLINE,
        )
        .await;
    // The fixture response may race with the dispatcher, but the connection closes.
    assert!(error.is_ok() || matches!(error, Err(Error::Protocol(_))));
    while tokio::time::timeout(DEADLINE, runtime.next_event())
        .await
        .unwrap()
        .is_some()
    {}
    assert!(matches!(
        runtime
            .client()
            .call("thread/list", Some(json!({})), DEADLINE)
            .await,
        Err(Error::Protocol(_))
    ));
    assert!(runtime.client().pending_interactions().is_empty());
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn facade_overflow_closes_connection_and_clears_pending_interactions() {
    let mut runtime = fixture(true, 1).await;
    next(&mut runtime).await;
    let _ = runtime
        .client()
        .call(
            "config/read",
            Some(json!({"fixtureEvents": [
                {"id": 1, "method": "item/fileChange/requestApproval", "params": {}},
                {"id": 2, "method": "item/fileChange/requestApproval", "params": {}}
            ]})),
            DEADLINE,
        )
        .await;
    while tokio::time::timeout(DEADLINE, runtime.next_event())
        .await
        .unwrap()
        .is_some()
    {}
    assert!(matches!(
        runtime
            .client()
            .call("thread/list", Some(json!({})), DEADLINE)
            .await,
        Err(Error::EventOverflow)
    ));
    assert!(runtime.client().pending_interactions().is_empty());
    runtime.shutdown().await.unwrap();
}
