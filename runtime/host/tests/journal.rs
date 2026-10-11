use std::path::PathBuf;

use caidex_host::{Journal, private_directory};
use rusqlite::Connection;
use serde_json::{Value, json};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let ordinal = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "caidex-h1-journal-{}-{unique}-{ordinal}",
            std::process::id()
        ));
        private_directory(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn journal_reopens_with_host_identity_sequences_snapshot_and_unknown_intent() {
    let directory = Directory::new();
    let mut journal = Journal::open(&directory.0).unwrap();
    let host = journal.snapshot().host_id;
    journal.append("host/started", json!({})).unwrap();
    journal
        .append("host/probeStarted", json!({"probe_id": "lost-response"}))
        .unwrap();
    let raw = json!({"method": "thread/started", "params": {"thread": {"id": "thread-1", "extension": {"opaque": [1,2,3]}}}, "future": true});
    let event = journal.append("thread/started", raw.clone()).unwrap();
    assert_eq!((event.seq, event.stream, event.stream_seq), (3, 1, 3));
    assert_eq!(event.data, raw);
    assert_eq!(
        journal.snapshot().threads["thread-1"]["runtime_state"],
        "loaded"
    );
    drop(journal);

    let mut journal = Journal::open(&directory.0).unwrap();
    assert_eq!(journal.snapshot().host_id, host);
    assert_eq!(journal.snapshot().seq, 3);
    assert_eq!(
        journal.snapshot().offline().threads["thread-1"]["runtime_state"],
        "unknown"
    );
    let event = journal.append("host/started", json!({})).unwrap();
    assert_eq!((event.seq, event.stream, event.stream_seq), (4, 2, 1));
    assert_eq!(
        journal.snapshot().threads["thread-1"]["wire"]["extension"]["opaque"],
        json!([1, 2, 3])
    );
    assert_eq!(
        journal.snapshot().threads["thread-1"]["runtime_state"],
        "unknown"
    );
    assert_eq!(
        journal.snapshot().unresolved_probes["lost-response"]["outcome"],
        "unknown"
    );
}

#[test]
fn runtime_reserved_names_preserve_unknown_intents_and_never_change_host_projection() {
    let directory = Directory::new();
    let mut journal = Journal::open(&directory.0).unwrap();
    journal.append("host/started", json!({})).unwrap();
    journal
        .append("host/probeStarted", json!({"probe_id": "lost-response"}))
        .unwrap();
    let before = journal.snapshot();
    for encoded in [
        r#""host\/started""#,
        r#""\u0068ost/stopped""#,
        r#""host/stopping""#,
        r#""host/runtimeUnavailable""#,
        r#""host/probeStarted""#,
        r#""host/probeResult""#,
        r#""host/future""#,
    ] {
        let method: String = serde_json::from_str(encoded).unwrap();
        let raw = json!({"method": method, "probe_id": "lost-response", "params": {"source": "host", "trusted": true, "opaque": [1,2,3]}});
        let event = journal.append_runtime(&method, raw.clone()).unwrap();
        assert_eq!(event.method, "runtime/notification");
        assert_eq!(event.data, raw);
        assert_eq!(event.stream, before.stream);
        assert_eq!(journal.snapshot().lifecycle, before.lifecycle);
        assert_eq!(
            journal.snapshot().unresolved_probes,
            before.unresolved_probes
        );
    }
    let raw = json!({"method": "thread/started", "params": {"thread": {"id": "legitimate", "opaque": true}}});
    assert_eq!(
        journal
            .append_runtime("thread/started", raw.clone())
            .unwrap()
            .data,
        raw
    );
    assert_eq!(
        journal.snapshot().threads["legitimate"]["runtime_state"],
        "loaded"
    );
    let unknown = json!({"method": "future/event", "params": {"opaque": true}});
    assert_eq!(
        journal
            .append_runtime("future/event", unknown.clone())
            .unwrap()
            .data,
        unknown
    );
    let saved = journal.snapshot();
    drop(journal);
    let mut journal = Journal::open(&directory.0).unwrap();
    assert_eq!(journal.snapshot(), saved);
    journal.append("host/started", json!({})).unwrap();
    assert_eq!(journal.snapshot().stream, before.stream + 1);
    assert_eq!(
        journal.snapshot().unresolved_probes,
        before.unresolved_probes
    );
    assert_eq!(
        journal.snapshot().threads["legitimate"]["runtime_state"],
        "unknown"
    );
    journal
        .append("host/probeResult", json!({"probe_id": "lost-response"}))
        .unwrap();
    assert!(journal.snapshot().unresolved_probes.is_empty());
}

#[test]
fn second_owner_is_rejected_and_lock_releases_after_close() {
    let directory = Directory::new();
    let journal = Journal::open(&directory.0).unwrap();
    assert!(Journal::open(&directory.0).is_err());
    drop(journal);
    assert!(Journal::open(&directory.0).is_ok());
}

#[test]
fn newer_schema_wrong_application_unversioned_and_corrupt_databases_are_preserved() {
    for mode in ["future", "other", "unversioned", "corrupt", "snapshot"] {
        let directory = Directory::new();
        let path = directory.0.join("journal.sqlite3");
        if mode == "corrupt" {
            std::fs::write(&path, b"not a SQLite database").unwrap();
        } else if mode == "unversioned" {
            Connection::open(&path)
                .unwrap()
                .execute_batch(
                    "CREATE TABLE user_data (value TEXT); INSERT INTO user_data VALUES ('keep');",
                )
                .unwrap();
        } else {
            drop(Journal::open(&directory.0).unwrap());
            let connection = Connection::open(&path).unwrap();
            match mode {
                "future" => connection.pragma_update(None, "user_version", 999).unwrap(),
                "other" => connection
                    .pragma_update(None, "application_id", 42)
                    .unwrap(),
                "snapshot" => {
                    connection
                        .execute("UPDATE snapshot SET data = '{}'", [])
                        .unwrap();
                }
                _ => unreachable!(),
            }
        }
        let before = std::fs::read(&path).unwrap();
        assert!(Journal::open(&directory.0).is_err(), "{mode}");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "{mode} must not be repaired"
        );
    }
}

#[cfg(unix)]
#[test]
fn unsafe_directory_and_storage_links_are_rejected_without_touching_targets() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = Directory::new();
    std::fs::set_permissions(&directory.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Journal::open(&directory.0).is_err());
    std::fs::set_permissions(&directory.0, std::fs::Permissions::from_mode(0o700)).unwrap();
    let target = directory.0.join("user-data");
    std::fs::write(&target, "keep").unwrap();
    symlink(&target, directory.0.join("journal.sqlite3")).unwrap();
    assert!(Journal::open(&directory.0).is_err());
    assert_eq!(std::fs::read_to_string(target).unwrap(), "keep");
}

#[test]
fn failed_commit_does_not_advance_event_or_snapshot() {
    let directory = Directory::new();
    let mut journal = Journal::open(&directory.0).unwrap();
    journal.append("host/started", json!({})).unwrap();
    let before = journal.snapshot();
    let connection = Connection::open(directory.0.join("journal.sqlite3")).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_snapshot BEFORE UPDATE ON snapshot BEGIN SELECT RAISE(ABORT, 'injected storage failure'); END;").unwrap();
    assert!(
        journal
            .append("host/probeStarted", json!({"probe_id": "never-submitted"}))
            .is_err()
    );
    assert_eq!(journal.snapshot(), before);
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    connection
        .execute_batch("DROP TRIGGER fail_snapshot;")
        .unwrap();
    assert_eq!(
        journal
            .append("future/opaque", json!({"opaque": true}))
            .unwrap()
            .seq,
        2
    );
}

#[test]
fn oversized_event_is_rejected_without_advancing_watermark() {
    let directory = Directory::new();
    let mut journal = Journal::open(&directory.0).unwrap();
    journal.append("host/started", json!({})).unwrap();
    let before = journal.snapshot();
    assert!(
        journal
            .append("oversized", json!("x".repeat(1024 * 1024)))
            .is_err()
    );
    assert_eq!(journal.snapshot(), before);
}

#[test]
fn offline_inspection_never_migrates_or_creates_a_database() {
    let directory = Directory::new();
    let path = directory.0.join("journal.sqlite3");
    assert!(Journal::inspect(&directory.0).is_err());
    assert!(!path.exists());
    let mut journal = Journal::open(&directory.0).unwrap();
    journal.append("host/started", json!({})).unwrap();
    assert!(
        Journal::inspect(&directory.0).is_err(),
        "live owner's lock cannot be bypassed"
    );
    drop(journal);
    let before = std::fs::read(&path).unwrap();
    assert_eq!(Journal::inspect(&directory.0).unwrap().lifecycle, "offline");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let connection = Connection::open(&path).unwrap();
    connection.pragma_update(None, "user_version", 0).unwrap();
    drop(connection);
    let before = std::fs::read(&path).unwrap();
    assert!(Journal::inspect(&directory.0).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn validated_v1_migration_preserves_identity_events_and_read_only_inspection() {
    let directory = Directory::new();
    let mut journal = Journal::open(&directory.0).unwrap();
    journal.append("host/started", json!({})).unwrap();
    journal
        .append(
            "thread/started",
            json!({"params": {"thread": {"id": "historical", "future": true}}}),
        )
        .unwrap();
    let saved = journal.snapshot();
    drop(journal);
    let path = directory.0.join("journal.sqlite3");
    let connection = Connection::open(&path).unwrap();
    let mut old = serde_json::to_value(&saved).unwrap();
    old["version"] = json!(1);
    old.as_object_mut().unwrap().remove("tasks");
    old.as_object_mut().unwrap().remove("operations");
    connection
        .execute("UPDATE snapshot SET data = ?1", [old.to_string()])
        .unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    drop(connection);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(Journal::inspect(&directory.0).unwrap().version, 1);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        bytes,
        "inspect must not migrate"
    );
    let migrated = Journal::open(&directory.0).unwrap();
    let actual = migrated.snapshot();
    assert_eq!(actual.version, 3);
    assert_eq!(actual.host_id, saved.host_id);
    assert_eq!(actual.seq, saved.seq);
    assert_eq!(actual.threads, saved.threads);
    assert!(actual.tasks.is_empty() && actual.operations.is_empty());
    drop(migrated);
    let connection = Connection::open(&path).unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 3);
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM events", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        saved.seq
    );
}

#[test]
fn failed_v1_migration_rolls_back_snapshot_and_version() {
    let directory = Directory::new();
    let mut journal = Journal::open(&directory.0).unwrap();
    journal.append("host/started", json!({})).unwrap();
    let mut snapshot = serde_json::to_value(journal.snapshot()).unwrap();
    snapshot["version"] = json!(1);
    drop(journal);
    let connection = Connection::open(directory.0.join("journal.sqlite3")).unwrap();
    connection
        .execute("UPDATE snapshot SET data=?1", [snapshot.to_string()])
        .unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    connection.execute_batch("CREATE TRIGGER migration_failure BEFORE UPDATE ON snapshot BEGIN SELECT RAISE(ABORT, 'injected migration failure'); END;").unwrap();
    assert!(Journal::open(&directory.0).is_err());
    let actual: String = connection
        .query_row("SELECT data FROM snapshot", [], |row| row.get(0))
        .unwrap();
    assert_eq!(actual, snapshot.to_string());
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn runtime_turn_notifications_require_explicit_matching_thread_and_turn_ids() {
    let directory = Directory::new();
    let mut journal = Journal::open(&directory.0).unwrap();
    journal.append("host/started", json!({})).unwrap();
    let mut task = caidex_host::Task {
        task_id: "task".into(),
        operation_id: "submit".into(),
        submission: caidex_host::Submission {
            prompt: "fixture".into(),
            model: "gpt-5.5".into(),
            provider: "caidex_h2_a".into(),
            parent_task_id: None,
            continue_thread: false,
            review: false,
        },
        status: "submitted".into(),
        thread_id: None,
        turn_id: None,
        actual: serde_json::Value::Null,
        pending: Default::default(),
        artifacts: Default::default(),
        last_seq: 0,
        stream: 1,
        expires_at: 9999999999,
    };
    let operation = caidex_host::Operation {
        operation_id: "submit".into(),
        payload_hash: "a".repeat(64),
        task_id: "task".into(),
        action: "submit".into(),
        outcome: "accepted".into(),
        last_seq: 0,
    };
    journal
        .append("host/task", json!({"task": task, "operation": operation}))
        .unwrap();
    for method in ["turn/started", "turn/completed"] {
        let raw =
            json!({"method": method, "params": {"turn": {"id": "foreign", "status": "completed"}}});
        assert_eq!(
            journal.append_runtime(method, raw.clone()).unwrap().data,
            raw
        );
        assert_eq!(journal.snapshot().tasks["task"].status, "submitted");
        assert!(journal.snapshot().tasks["task"].turn_id.is_none());
    }
    task.thread_id = Some("real-thread".into());
    task.turn_id = Some("real-turn".into());
    task.status = "running".into();
    journal.append("host/task", json!({"task": task})).unwrap();
    for (thread, turn) in [
        ("foreign-thread", "real-turn"),
        ("real-thread", "foreign-turn"),
    ] {
        journal.append_runtime("turn/completed", json!({"params": {"threadId": thread, "turn": {"id": turn, "status": "completed"}}})).unwrap();
        assert_eq!(journal.snapshot().tasks["task"].status, "running");
    }
    journal.append_runtime("turn/completed", json!({"params": {"threadId": "real-thread", "turn": {"id": "real-turn", "status": "completed"}}})).unwrap();
    assert_eq!(journal.snapshot().tasks["task"].status, "completed");
    drop(journal);
    assert_eq!(
        Journal::inspect(&directory.0).unwrap().tasks["task"].status,
        "completed"
    );
}

fn h3_journal(directory: &Directory) -> Journal {
    let mut journal = Journal::open(&directory.0).unwrap();
    journal.append("host/started", json!({})).unwrap();
    let task:caidex_host::Task=serde_json::from_value(json!({"task_id":"task","operation_id":"submit","submission":{"prompt":"fixture","model":"gpt-5.5","provider":"caidex_h2_a"},"status":"blocked","thread_id":"thread","turn_id":"turn","actual":null,"pending":{"77":{"request_id":77,"status":"pending","expires_at":9999999999u64}},"artifacts":{},"last_seq":0,"stream":1,"expires_at":9999999999u64})).unwrap();
    let op = caidex_host::Operation {
        operation_id: "submit".into(),
        payload_hash: "a".repeat(64),
        task_id: "task".into(),
        action: "submit".into(),
        outcome: "accepted".into(),
        last_seq: 0,
    };
    journal
        .append("host/task", json!({"task":task,"operation":op}))
        .unwrap();
    journal
}

#[test]
fn diff_review_and_tool_artifacts_preserve_wire_ids_unknown_fields_and_restart() {
    let directory = Directory::new();
    let mut journal = h3_journal(&directory);
    let raw = json!({"method":"turn/diff/updated","params":{"threadId":"thread","turnId":"turn","diff":"diff --git a/a b/a\n+native change","future":{"opaque":true}},"unknown":17});
    journal
        .append_runtime("turn/diff/updated", raw.clone())
        .unwrap();
    assert_eq!(journal.snapshot().tasks["task"].artifacts["diff"], raw);
    for (thread, turn) in [("foreign", "turn"), ("thread", "foreign")] {
        journal
            .append_runtime(
                "turn/diff/updated",
                json!({"params":{"threadId":thread,"turnId":turn,"diff":"wrong"}}),
            )
            .unwrap();
        assert_eq!(journal.snapshot().tasks["task"].artifacts["diff"], raw);
    }
    for (id, kind) in [
        ("review-in", "enteredReviewMode"),
        ("review-out", "exitedReviewMode"),
        ("tool", "commandExecution"),
    ] {
        let item = json!({"method":"item/completed","params":{"threadId":"thread","turnId":"turn","item":{"id":id,"type":kind,"review":"native review","extension":true}}});
        journal
            .append_runtime("item/completed", item.clone())
            .unwrap();
        assert_eq!(
            journal.snapshot().tasks["task"].artifacts[&format!("item:{id}")],
            item
        );
    }
    journal
        .append_runtime(
            "serverRequest/resolved",
            json!({"params":{"threadId":"foreign","requestId":77}}),
        )
        .unwrap();
    assert_eq!(
        journal.snapshot().tasks["task"].pending["77"]["status"],
        "pending"
    );
    journal
        .append_runtime(
            "serverRequest/resolved",
            json!({"params":{"threadId":"thread","requestId":77}}),
        )
        .unwrap();
    assert_eq!(
        journal.snapshot().tasks["task"].pending["77"]["status"],
        "resolved"
    );
    assert_eq!(journal.snapshot().tasks["task"].status, "running");
    let mut late = journal.snapshot().tasks["task"].clone();
    late.task_id = "late".into();
    late.operation_id = "late-submit".into();
    late.thread_id = Some("late-thread".into());
    late.turn_id = Some("late-turn".into());
    let mut late_operation = journal.snapshot().operations["submit"].clone();
    late_operation.operation_id = late.operation_id.clone();
    late_operation.task_id = late.task_id.clone();
    journal
        .append("host/task", json!({"task":late,"operation":late_operation}))
        .unwrap();
    journal.append_runtime("turn/completed", json!({"params":{"threadId":"late-thread","turn":{"id":"late-turn","status":"completed"}}})).unwrap();
    let late_diff = json!({"method":"turn/diff/updated","params":{"threadId":"late-thread","turnId":"late-turn","diff":"native Diff after terminal event","future":true}});
    journal
        .append_runtime("turn/diff/updated", late_diff.clone())
        .unwrap();
    assert_eq!(journal.snapshot().tasks["late"].status, "completed");
    assert_eq!(
        journal.snapshot().tasks["late"].artifacts["diff"],
        late_diff
    );
    let before = journal.snapshot();
    drop(journal);
    let offline = Journal::inspect(&directory.0).unwrap();
    assert_eq!(offline.tasks["task"].status, "unknown");
    assert_eq!(offline.tasks["late"].artifacts["diff"], late_diff);
    assert_eq!(
        offline.tasks["task"].artifacts,
        before.tasks["task"].artifacts
    );
    let mut journal = Journal::open(&directory.0).unwrap();
    journal.append("host/started", json!({})).unwrap();
    journal
        .append_runtime(
            "turn/diff/updated",
            json!({"params":{"threadId":"thread","turnId":"turn","diff":"stale stream"}}),
        )
        .unwrap();
    assert_eq!(
        journal.snapshot().tasks["task"].artifacts,
        before.tasks["task"].artifacts
    );
}

#[test]
fn artifact_capacity_failure_rolls_back_event_and_snapshot() {
    let directory = Directory::new();
    let mut journal = h3_journal(&directory);
    for i in 0..64 {
        journal.append_runtime("item/completed",json!({"params":{"threadId":"thread","turnId":"turn","item":{"id":i.to_string(),"type":"future"}}})).unwrap();
    }
    let before = journal.snapshot();
    assert!(
        journal
            .append_runtime(
                "turn/diff/updated",
                json!({"params":{"threadId":"thread","turnId":"turn","diff":"overflow"}})
            )
            .is_err()
    );
    assert_eq!(journal.snapshot(), before);
    let connection = Connection::open(directory.0.join("journal.sqlite3")).unwrap();
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, before.seq);
    let byte_directory = Directory::new();
    let mut journal = h3_journal(&byte_directory);
    journal
        .append_runtime(
            "turn/diff/updated",
            json!({"params":{"threadId":"thread","turnId":"turn","diff":"x".repeat(600*1024)}}),
        )
        .unwrap();
    let before = journal.snapshot();
    assert!(journal.append_runtime("item/completed", json!({"params":{"threadId":"thread","turnId":"turn","item":{"id":"too-large","type":"agentMessage","text":"y".repeat(600*1024)}}})).is_err());
    assert_eq!(
        journal.snapshot(),
        before,
        "total byte limit rolls back, not just item count"
    );
}

#[test]
fn v2_upgrade_preserves_operation_hash_and_identity_without_inspect_writes() {
    let directory = Directory::new();
    let journal = h3_journal(&directory);
    let saved = journal.snapshot();
    drop(journal);
    let connection = Connection::open(directory.0.join("journal.sqlite3")).unwrap();
    let mut wire = serde_json::to_value(&saved).unwrap();
    wire["version"] = json!(2);
    wire.as_object_mut().unwrap().remove("clients");
    wire["tasks"]["task"]
        .as_object_mut()
        .unwrap()
        .remove("artifacts");
    connection
        .execute("UPDATE snapshot SET data=?1", [wire.to_string()])
        .unwrap();
    connection.pragma_update(None, "user_version", 2).unwrap();
    drop(connection);
    let bytes = std::fs::read(directory.0.join("journal.sqlite3")).unwrap();
    let inspected = Journal::inspect(&directory.0).unwrap();
    assert_eq!(inspected.version, 2);
    assert_eq!(
        bytes,
        std::fs::read(directory.0.join("journal.sqlite3")).unwrap()
    );
    let connection = Connection::open(directory.0.join("journal.sqlite3")).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_v2 BEFORE UPDATE ON snapshot BEGIN SELECT RAISE(ABORT,'injected v2 migration failure'); END;").unwrap();
    assert!(Journal::open(&directory.0).is_err());
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 2);
    let unchanged: String = connection
        .query_row("SELECT data FROM snapshot", [], |r| r.get(0))
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&unchanged).unwrap(), wire);
    connection.execute_batch("DROP TRIGGER fail_v2").unwrap();
    drop(connection);
    let journal = Journal::open(&directory.0).unwrap();
    let upgraded = journal.snapshot();
    assert_eq!(upgraded.version, 3);
    assert_eq!(upgraded.host_id, saved.host_id);
    assert_eq!(upgraded.seq, saved.seq);
    assert_eq!(upgraded.operations, saved.operations);
    assert!(upgraded.clients.is_empty() && upgraded.tasks["task"].artifacts.is_empty());
}
