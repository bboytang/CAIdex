use std::{path::PathBuf, time::Duration};

use caidex_host::{Journal, private_directory, serve};
use caidex_runtime::{AppServer, ClientOptions, Runtime, RuntimeClient};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    process::Command,
    task::JoinHandle,
};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const DEADLINE: Duration = Duration::from_secs(8);

struct Harness {
    directory: PathBuf,
    task: JoinHandle<caidex_host::Result<()>>,
    address: std::net::SocketAddr,
    host: String,
    runtime: RuntimeClient,
}

impl Harness {
    async fn start(mode: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "caidex-h1-service-{}-{unique}-{sequence}",
            std::process::id()
        ));
        Self::at(directory, mode).await
    }
    async fn at(directory: PathBuf, mode: &str) -> Self {
        private_directory(&directory).unwrap();
        let journal = Journal::open(&directory).unwrap();
        let host = journal.snapshot().host_id;
        let python = std::env::var_os("CAIDEX_TEST_PYTHON")
            .unwrap_or_else(|| if cfg!(windows) { "python" } else { "python3" }.into());
        let mut command = Command::new(python);
        command
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/peer.py"))
            .arg(mode)
            .arg(directory.join("marker"));
        let runtime = Runtime::connect(
            AppServer::spawn(command, 512).unwrap(),
            ClientOptions::default(),
            DEADLINE,
            512,
        )
        .await
        .unwrap();
        let client = runtime.client();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let project = directory.clone();
        let task = tokio::spawn(serve(listener, runtime, journal, TOKEN.into(), project));
        Self {
            directory,
            task,
            address,
            host,
            runtime: client,
        }
    }
    async fn inject(&self, events: Value) {
        self.runtime
            .call(
                "config/read",
                Some(json!({"fixtureEvents": events})),
                DEADLINE,
            )
            .await
            .unwrap();
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.task.abort(); /* Directory retained until tests explicitly release the task. */
    }
}

struct Client {
    lines: tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>,
    write: tokio::net::tcp::OwnedWriteHalf,
    id: u64,
    events: Vec<Value>,
}
impl Client {
    async fn connect(address: std::net::SocketAddr) -> Self {
        let (read, write) = TcpStream::connect(address).await.unwrap().into_split();
        let mut client = Self {
            lines: BufReader::new(read).lines(),
            write,
            id: 0,
            events: Vec::new(),
        };
        client.send(json!({"protocol": 1, "token": TOKEN})).await;
        assert_eq!(client.next().await["authorized"], true);
        client
    }
    async fn send(&mut self, value: Value) {
        self.write
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
    }
    async fn next(&mut self) -> Value {
        let line = tokio::time::timeout(DEADLINE, self.lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .expect("connected Host");
        serde_json::from_str(&line).unwrap()
    }
    async fn call(&mut self, mut value: Value) -> Value {
        self.id += 1;
        value["id"] = json!(self.id);
        self.send(value).await;
        loop {
            let reply = self.next().await;
            if reply["id"] == self.id {
                return reply;
            }
            if let Some(event) = reply.get("event") {
                self.events.push(event.clone());
            }
        }
    }
    async fn event(&mut self, method: &str) -> Value {
        if let Some(index) = self
            .events
            .iter()
            .position(|event| event["method"] == method)
        {
            return self.events.remove(index);
        }
        loop {
            let reply = self.next().await;
            if reply.pointer("/event/method").and_then(Value::as_str) == Some(method) {
                return reply["event"].clone();
            }
        }
    }
}

#[tokio::test]
async fn two_clients_replay_detach_live_events_and_snapshot_have_one_committed_watermark() {
    let mut harness = Harness::start("normal").await;
    let mut a = Client::connect(harness.address).await;
    let mut b = Client::connect(harness.address).await;
    let attach = a
        .call(json!({"method": "attach", "host_id": harness.host}))
        .await;
    let seq = attach["result"]["snapshot"]["seq"].as_i64().unwrap();
    b.call(json!({"method": "attach", "host_id": harness.host, "after": seq}))
        .await;
    a.call(json!({"method": "detach"})).await;
    drop(a);
    let result = b.call(json!({"method": "probe"})).await;
    assert_eq!(result["result"]["outcome"], "confirmed");
    let live = b.event("thread/started").await;
    let connection = rusqlite::Connection::open(harness.directory.join("journal.sqlite3")).unwrap();
    let committed: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM events WHERE seq = ?1",
            [live["seq"].as_i64().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        committed, 1,
        "broadcast must already be visible to an independent DB reader"
    );
    drop(b);
    let mut a = Client::connect(harness.address).await;
    let replay_a = a
        .call(json!({"method": "attach", "host_id": harness.host, "after": seq}))
        .await;
    let mut b = Client::connect(harness.address).await;
    let replay_b = b
        .call(json!({"method": "attach", "host_id": harness.host, "after": seq}))
        .await;
    assert_eq!(replay_a["result"], replay_b["result"]);
    let events = replay_a["result"]["events"].as_array().unwrap();
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event["seq"], seq + 1 + index as i64);
    }
    assert!(
        events
            .iter()
            .any(|event| event["method"] == "thread/started")
    );
    assert!(
        a.call(json!({"method": "attach", "host_id": "other", "after": seq}))
            .await
            .get("error")
            .is_some()
    );
    assert!(
        a.call(json!({"method": "attach", "host_id": harness.host, "after": i64::MAX}))
            .await
            .get("error")
            .is_some()
    );

    a.call(json!({"method": "detach"})).await;
    b.call(json!({"method": "detach"})).await;
    harness
        .inject(json!(
            (0..140)
                .map(|i| json!({"method": "future/event", "params": {"index": i}}))
                .collect::<Vec<_>>()
        ))
        .await;
    loop {
        let snapshot = a.call(json!({"method": "snapshot"})).await;
        if snapshot["result"]["seq"].as_i64().unwrap()
            >= events.last().unwrap()["seq"].as_i64().unwrap() + 140
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    let recovery = a
        .call(json!({"method": "attach", "host_id": harness.host, "after": seq}))
        .await;
    assert_eq!(recovery["result"]["mode"], "snapshot");
    assert_eq!(
        recovery["result"]["snapshot"]["threads"]["fixture-thread"]["wire"]["unknown"],
        "keep"
    );
    a.call(json!({"method": "shutdown"})).await;
    assert!(
        tokio::time::timeout(DEADLINE, &mut harness.task)
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
    drop(connection);
    std::fs::remove_dir_all(&harness.directory).unwrap();
}

#[tokio::test]
async fn abrupt_host_loss_preserves_unknown_probe_and_restart_never_resends_it() {
    let mut harness = Harness::start("delay").await;
    let mut client = Client::connect(harness.address).await;
    client.send(json!({"id": 1, "method": "probe"})).await;
    tokio::time::timeout(DEADLINE, async {
        while !harness.directory.join("marker").exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    harness.task.abort();
    assert!((&mut harness.task).await.unwrap_err().is_cancelled());
    drop(client);
    let journal = Journal::open(&harness.directory).unwrap();
    assert_eq!(journal.snapshot().unresolved_probes.len(), 1);
    let host = journal.snapshot().host_id;
    let seq = journal.snapshot().seq;
    drop(journal);
    let directory = harness.directory.clone();
    let mut restarted = Harness::at(directory.clone(), "normal").await;
    let mut client = Client::connect(restarted.address).await;
    let snapshot = client.call(json!({"method": "snapshot"})).await["result"].clone();
    assert_eq!(snapshot["host_id"], host);
    assert_eq!(snapshot["seq"], seq + 1);
    assert_eq!(snapshot["stream"], 2);
    assert_eq!(snapshot["unresolved_probes"].as_object().unwrap().len(), 1);
    client.call(json!({"method": "shutdown"})).await;
    assert!(
        tokio::time::timeout(DEADLINE, &mut restarted.task)
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("marker")).unwrap(),
        "thread/start\n"
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn runtime_loss_marks_unavailable_and_does_not_claim_probe_failed() {
    let mut harness = Harness::start("exit").await;
    let mut client = Client::connect(harness.address).await;
    client.send(json!({"id": 1, "method": "probe"})).await;
    assert!(
        tokio::time::timeout(DEADLINE, &mut harness.task)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    let journal = Journal::open(&harness.directory).unwrap();
    assert_eq!(journal.snapshot().lifecycle, "unavailable");
    assert_eq!(journal.snapshot().unresolved_probes.len(), 1);
    drop(journal);
    std::fs::remove_dir_all(&harness.directory).unwrap();
}

#[tokio::test]
async fn unauthorized_clients_unavailable_actions_and_auth_payloads_are_rejected() {
    let mut harness = Harness::start("normal").await;
    let (read, mut write) = TcpStream::connect(harness.address)
        .await
        .unwrap()
        .into_split();
    write
        .write_all(b"{\"protocol\":1,\"token\":\"account-token-is-not-host-authorization\"}\n")
        .await
        .unwrap();
    let rejected = BufReader::new(read)
        .lines()
        .next_line()
        .await
        .unwrap()
        .unwrap();
    assert!(rejected.contains("rejected"));
    let mut client = Client::connect(harness.address).await;
    client
        .send(json!({"id": 2, "method": "turn/start", "params": {"input": "never execute"}}))
        .await;
    assert!(client.next().await.get("error").is_some());
    client.send(json!({"id": 3, "method": "probe", "cwd": "unapproved-project", "apiKey": "DO_NOT_IMPORT"})).await;
    assert!(client.next().await.get("error").is_some());
    assert!(!harness.directory.join("marker").exists());
    harness.inject(json!([{"id": "auth-request", "method": "account/chatgptAuthTokens/refresh", "params": {"accessToken": "NEVER_PERSIST_THIS"}}])).await;
    assert!(
        tokio::time::timeout(DEADLINE, &mut harness.task)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    let connection = rusqlite::Connection::open(harness.directory.join("journal.sqlite3")).unwrap();
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM events WHERE data LIKE '%NEVER_PERSIST_THIS%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    drop(connection);
    std::fs::remove_dir_all(&harness.directory).unwrap();
}

#[tokio::test]
async fn runtime_host_namespace_cannot_control_lifecycle_stream_or_recovery() {
    let mut harness = Harness::start("normal").await;
    let mut client = Client::connect(harness.address).await;
    client
        .call(json!({"method": "attach", "host_id": harness.host}))
        .await;
    client.call(json!({"method": "probe"})).await;
    client.event("thread/started").await;
    let before = client.call(json!({"method": "snapshot"})).await["result"].clone();
    let raw: Vec<Value> = ["host/started", "host/stopped", "host/stopping", "host/runtimeUnavailable", "host/probeStarted", "host/probeResult", "host/future"]
        .into_iter()
        .map(|method| json!({"method": method, "params": {"future": {"opaque": [1,2,3]}}, "probe_id": "forged", "source": "host", "trusted": true}))
        .collect();
    harness.inject(json!(raw)).await;
    let after = tokio::time::timeout(DEADLINE, async {
        loop {
            let snapshot = client.call(json!({"method": "snapshot"})).await["result"].clone();
            if snapshot["seq"].as_i64().unwrap()
                >= before["seq"].as_i64().unwrap() + raw.len() as i64
            {
                break snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(after["lifecycle"], "running");
    assert_eq!(after["stream"], before["stream"]);
    assert_eq!(after["threads"], before["threads"]);
    assert_eq!(after["unresolved_probes"], before["unresolved_probes"]);
    for (index, original) in raw.iter().enumerate() {
        let event = client.event("runtime/notification").await;
        assert_eq!(event["data"], *original);
        assert_eq!(
            event["seq"],
            before["seq"].as_i64().unwrap() + index as i64 + 1
        );
        assert_eq!(event["stream"], before["stream"]);
    }
    let replay = client
        .call(json!({"method": "attach", "host_id": harness.host, "after": before["seq"]}))
        .await;
    assert_eq!(
        replay["result"]["events"].as_array().unwrap().len(),
        raw.len()
    );
    for (event, original) in replay["result"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .zip(&raw)
    {
        assert_eq!(event["method"], "runtime/notification");
        assert_eq!(event["data"], *original);
    }
    client.call(json!({"method": "shutdown"})).await;
    assert!(
        tokio::time::timeout(DEADLINE, &mut harness.task)
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
    drop(client);
    let mut restarted = Harness::at(harness.directory.clone(), "normal").await;
    let mut client = Client::connect(restarted.address).await;
    let recovered = client.call(json!({"method": "snapshot"})).await["result"].clone();
    assert_eq!(recovered["stream"], before["stream"].as_i64().unwrap() + 1);
    assert_eq!(recovered["lifecycle"], "running");
    assert_eq!(
        recovered["threads"]["fixture-thread"]["runtime_state"],
        "unknown"
    );
    assert_eq!(
        std::fs::read_to_string(harness.directory.join("marker")).unwrap(),
        "thread/start\n"
    );
    client.call(json!({"method": "shutdown"})).await;
    assert!(
        tokio::time::timeout(DEADLINE, &mut restarted.task)
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
    std::fs::remove_dir_all(&harness.directory).unwrap();
}

#[tokio::test]
async fn failed_intent_commit_neither_calls_runtime_nor_broadcasts_an_event() {
    let mut harness = Harness::start("normal").await;
    let mut client = Client::connect(harness.address).await;
    let recovery = client
        .call(json!({"method": "attach", "host_id": harness.host}))
        .await;
    let seq = recovery["result"]["snapshot"]["seq"].as_i64().unwrap();
    let connection = rusqlite::Connection::open(harness.directory.join("journal.sqlite3")).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_intent BEFORE INSERT ON events WHEN NEW.method = 'host/probeStarted' BEGIN SELECT RAISE(ABORT, 'injected failure'); END;").unwrap();
    client.send(json!({"id": 1, "method": "probe"})).await;
    assert!(
        tokio::time::timeout(DEADLINE, &mut harness.task)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert!(
        client.lines.next_line().await.unwrap().is_none(),
        "failed intent must not be published"
    );
    assert!(
        !harness.directory.join("marker").exists(),
        "failed commit must not execute Runtime request"
    );
    let max: i64 = connection
        .query_row("SELECT MAX(seq) FROM events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(seq, max);
    drop(connection);
    std::fs::remove_dir_all(&harness.directory).unwrap();
}

#[tokio::test]
async fn partial_client_frame_survives_interleaved_event_broadcast() {
    let mut harness = Harness::start("normal").await;
    let mut client = Client::connect(harness.address).await;
    client
        .call(json!({"method": "attach", "host_id": harness.host}))
        .await;
    client
        .write
        .write_all(b"{\"id\":42,\"method\":")
        .await
        .unwrap();
    harness
        .inject(json!([{"method": "future/event", "params": {"opaque": "keep"}}]))
        .await;
    client.event("future/event").await;
    client.write.write_all(b"\"snapshot\"}\n").await.unwrap();
    let reply = client.next().await;
    assert_eq!(reply["id"], 42);
    assert_eq!(reply["result"]["lifecycle"], "running");
    client.call(json!({"method": "shutdown"})).await;
    assert!(
        tokio::time::timeout(DEADLINE, &mut harness.task)
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
    std::fs::remove_dir_all(&harness.directory).unwrap();
}
