use std::path::PathBuf;

use caidex_host::{Journal, private_directory};
use rusqlite::Connection;
use serde_json::json;

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
    assert_eq!(actual.version, 2);
    assert_eq!(actual.host_id, saved.host_id);
    assert_eq!(actual.seq, saved.seq);
    assert_eq!(actual.threads, saved.threads);
    assert!(actual.tasks.is_empty() && actual.operations.is_empty());
    drop(migrated);
    let connection = Connection::open(&path).unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 2);
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
        },
        status: "submitted".into(),
        thread_id: None,
        turn_id: None,
        actual: serde_json::Value::Null,
        pending: Default::default(),
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
