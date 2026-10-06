use std::{path::PathBuf, time::Duration};

use caidex_runtime::{AppServer, Error, RequestId};
use serde_json::json;
use tokio::process::Command;

const DEADLINE: Duration = Duration::from_secs(5);

fn fixture(capacity: usize) -> AppServer {
    let python = std::env::var_os("CAIDEX_TEST_PYTHON").unwrap_or_else(|| {
        if cfg!(windows) {
            "python".into()
        } else {
            "python3".into()
        }
    });
    let mut command = Command::new(python);
    command.arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/app_server.py"));
    AppServer::spawn(command, capacity).unwrap()
}

#[tokio::test]
async fn correlates_out_of_order_responses_and_retains_unknown_events() {
    let mut server = fixture(8);
    let client = server.client();
    let (first, second) = tokio::join!(
        client.request("test/first", json!({}), DEADLINE),
        client.request("test/second", json!({}), DEADLINE),
    );
    assert_eq!(first.unwrap(), "first");
    assert_eq!(second.unwrap(), "second");
    let event = server.next_event().await.unwrap();
    assert_eq!(event.method, "future/unknown");
    assert_eq!(event.raw["extension"], json!([1, 2]));
    assert_eq!(event.raw["params"]["opaque"], "signature");
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn routes_approval_to_caller_and_replies_with_original_string_id() {
    let mut server = fixture(8);
    let client = server.client();
    client
        .request("test/approval", json!({}), DEADLINE)
        .await
        .unwrap();
    let event = server.next_event().await.unwrap();
    assert_eq!(
        event.request_id,
        Some(RequestId::String("approval-7".into()))
    );
    assert_eq!(event.raw["extension"], true);
    client
        .respond(
            event.request_id.unwrap(),
            Ok(json!({"decision": "decline"})),
        )
        .await
        .unwrap();
    let reply = server.next_event().await.unwrap();
    assert_eq!(reply.raw["params"]["id"], "approval-7");
    assert_eq!(reply.raw["params"]["result"]["decision"], "decline");
    assert!(reply.raw["params"].get("jsonrpc").is_none());
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_rpc_error_data_and_keeps_connection_usable() {
    let mut server = fixture(8);
    let client = server.client();
    match client
        .request("test/error", json!({}), DEADLINE)
        .await
        .unwrap_err()
    {
        Error::Rpc(-32001, message, Some(data)) => {
            assert_eq!(message, "overloaded");
            assert_eq!(data, json!({"retry": true}));
        }
        error => panic!("unexpected error: {error}"),
    }
    assert_eq!(
        client
            .request("test/echo", json!(null), DEADLINE)
            .await
            .unwrap(),
        json!(null)
    );
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn process_exit_fails_pending_and_future_requests() {
    let mut server = fixture(8);
    let client = server.client();
    assert!(matches!(
        client.request("test/close", json!({}), DEADLINE).await,
        Err(Error::Closed)
    ));
    assert!(matches!(
        client.request("test/echo", json!({}), DEADLINE).await,
        Err(Error::Closed)
    ));
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn timeout_marks_outcome_unknown_and_prevents_automatic_reuse() {
    let mut server = fixture(8);
    let client = server.client();
    assert!(matches!(
        client
            .request("test/hang", json!({}), Duration::from_millis(100))
            .await,
        Err(Error::Timeout)
    ));
    assert!(matches!(
        client.request("test/echo", json!({}), DEADLINE).await,
        Err(Error::Timeout)
    ));
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn overflow_fails_connection_instead_of_dropping_runtime_events() {
    let mut server = fixture(1);
    assert!(matches!(
        server
            .client()
            .request("test/burst", json!({}), DEADLINE)
            .await,
        Err(Error::EventOverflow)
    ));
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn malformed_json_fails_connection() {
    let mut server = fixture(8);
    assert!(matches!(
        server
            .client()
            .request("test/malformed", json!({}), DEADLINE)
            .await,
        Err(Error::Protocol(_))
    ));
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_response_payload_is_not_treated_as_success() {
    let mut server = fixture(8);
    assert!(matches!(
        server
            .client()
            .request("test/missingResult", json!({}), DEADLINE)
            .await,
        Err(Error::Protocol(_))
    ));
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancelled_request_closes_connection_without_replaying_the_action() {
    let mut server = fixture(8);
    let client = server.client();
    let pending = {
        let client = client.clone();
        tokio::spawn(async move { client.request("test/hang", json!({}), DEADLINE).await })
    };
    tokio::time::timeout(DEADLINE, server.next_event())
        .await
        .unwrap()
        .unwrap();
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    assert!(
        tokio::time::timeout(DEADLINE, server.next_event())
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        client.request("test/echo", json!({}), DEADLINE).await,
        Err(Error::Cancelled)
    ));
    server.shutdown().await.unwrap();
}
