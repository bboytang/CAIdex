use std::{collections::BTreeMap, fs::File, path::Path, time::Duration};

use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Error, Result, random_id};

const APPLICATION_ID: i64 = 0x43414948; // CAIH: distinct from Chat/Memory databases.
const SCHEMA_VERSION: i64 = 2;
pub(crate) const REPLAY_LIMIT: i64 = 128;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Event {
    pub host_id: String,
    pub seq: i64,
    pub stream: i64,
    pub stream_seq: i64,
    pub method: String,
    pub data: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Snapshot {
    pub version: i64,
    pub host_id: String,
    pub seq: i64,
    pub stream: i64,
    pub stream_seq: i64,
    pub lifecycle: String,
    /// Cached wire data is not evidence that an old thread is currently loaded.
    pub threads: BTreeMap<String, Value>,
    pub unresolved_probes: BTreeMap<String, Value>,
    pub tasks: BTreeMap<String, crate::Task>,
    pub operations: BTreeMap<String, crate::Operation>,
}

impl Snapshot {
    fn invalidate_runtime(&mut self) {
        for task in self.tasks.values_mut() {
            task.invalidate();
            task.last_seq = self.seq;
        }
        for operation in self.operations.values_mut() {
            if operation.action != "submit"
                && matches!(
                    operation.outcome.as_str(),
                    "accepted" | "sending" | "requested"
                )
            {
                operation.outcome = "unknown".into();
                operation.last_seq = self.seq;
            }
        }
        for thread in self.threads.values_mut() {
            thread["runtime_state"] = json!("unknown");
        }
    }

    fn apply(&mut self, method: &str, data: &Value) {
        match method {
            "host/started" => {
                self.invalidate_runtime();
                self.lifecycle = "running".into();
            }
            "host/stopping" => self.lifecycle = "stopping".into(),
            "host/stopped" | "host/runtimeUnavailable" => {
                self.invalidate_runtime();
                self.lifecycle = if method == "host/stopped" {
                    "stopped"
                } else {
                    "unavailable"
                }
                .into();
            }
            "host/task" => {
                if let Ok(mut task) = serde_json::from_value::<crate::Task>(data["task"].clone()) {
                    task.last_seq = self.seq;
                    self.tasks.insert(task.task_id.clone(), task);
                }
                if let Ok(mut operation) =
                    serde_json::from_value::<crate::Operation>(data["operation"].clone())
                {
                    operation.last_seq = self.seq;
                    self.operations
                        .insert(operation.operation_id.clone(), operation);
                }
            }
            "thread/settings/updated" => {
                if let Some(thread) = data.pointer("/params/threadId").and_then(Value::as_str) {
                    let settings = &data["params"]["threadSettings"];
                    for task in self.tasks.values_mut().filter(|task| {
                        task.stream == self.stream
                            && task.thread_id.as_deref() == Some(thread)
                            && !task.terminal()
                            && task.status != "unknown"
                    }) {
                        task.actual = json!({"model": settings["model"], "provider": settings["modelProvider"], "policy": settings["approvalPolicy"], "reviewer": settings["approvalsReviewer"], "sandbox": settings["sandboxPolicy"], "cwd": settings["cwd"]});
                        task.last_seq = self.seq;
                    }
                }
            }
            "turn/started" | "turn/completed" => {
                let thread = data.pointer("/params/threadId").and_then(Value::as_str);
                let turn = data.pointer("/params/turn/id").and_then(Value::as_str);
                for task in self.tasks.values_mut().filter(|task| {
                    task.stream == self.stream
                        && task.thread_id.as_deref() == thread
                        && !task.terminal()
                        && task.status != "unknown"
                }) {
                    if let Some(turn) = turn {
                        if task.turn_id.as_deref().is_some_and(|known| known != turn) {
                            continue;
                        }
                        task.turn_id = Some(turn.into());
                        if method == "turn/completed" {
                            let status =
                                data.pointer("/params/turn/status").and_then(Value::as_str);
                            task.status = match status {
                                Some("completed") => "completed",
                                Some("failed") => "failed",
                                Some("interrupted") => "cancelled",
                                _ => "unknown",
                            }
                            .into();
                            for pending in task.pending.values_mut() {
                                pending["status"] = json!("unavailable");
                            }
                            for operation in self.operations.values_mut().filter(|operation| {
                                operation.task_id == task.task_id
                                    && operation.action == "cancel"
                                    && matches!(
                                        operation.outcome.as_str(),
                                        "accepted" | "sending" | "requested"
                                    )
                            }) {
                                operation.outcome = "confirmed".into();
                                operation.last_seq = self.seq;
                            }
                        } else if task.status == "submitted" {
                            task.status = "running".into();
                        }
                        task.last_seq = self.seq;
                    }
                }
            }
            "host/probeStarted" => {
                if let Some(id) = data["probe_id"].as_str() {
                    self.unresolved_probes
                        .insert(id.into(), json!({"outcome": "unknown"}));
                }
            }
            "host/probeResult" => {
                if let Some(id) = data["probe_id"].as_str() {
                    self.unresolved_probes.remove(id);
                }
            }
            "thread/started" => {
                if let Some(id) = data.pointer("/params/thread/id").and_then(Value::as_str) {
                    self.threads.insert(
                        id.into(),
                        json!({"runtime_state": "loaded", "wire": data["params"]["thread"]}),
                    );
                }
            }
            "thread/closed" | "thread/deleted" => {
                if let Some(id) = data.pointer("/params/threadId").and_then(Value::as_str)
                    && let Some(thread) = self.threads.get_mut(id)
                {
                    thread["runtime_state"] = json!("unloaded");
                }
            }
            _ => (),
        }
    }

    /// Offline inspection must not present a cached running flag as a live Host.
    pub fn offline(mut self) -> Self {
        self.invalidate_runtime();
        self.lifecycle = "offline".into();
        self
    }
}

struct StorageLock(File);

impl Drop for StorageLock {
    fn drop(&mut self) {
        // A fork/duplicate may keep the open file description alive after close.
        let _ = self.0.unlock();
    }
}

pub struct Journal {
    connection: Connection,
    snapshot: Snapshot,
    // Fields drop in declaration order: close SQLite before releasing ownership.
    _lock: StorageLock,
}

impl Journal {
    fn lock_storage(directory: &Path) -> Result<StorageLock> {
        crate::private_directory(directory)?;
        // Existing links/files are never replaced. The private directory is the
        // OS-user trust boundary; another process as that user already has access.
        for name in [
            "host.lock",
            "journal.sqlite3",
            "journal.sqlite3-wal",
            "journal.sqlite3-shm",
        ] {
            if let Ok(metadata) = std::fs::symlink_metadata(directory.join(name))
                && (!metadata.is_file() || metadata.is_symlink())
            {
                return Err(Error::Refused("unsafe Host storage entry"));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if let Ok(metadata) = std::fs::symlink_metadata(directory.join(name))
                    && (metadata.nlink() != 1 || metadata.uid() != unsafe { libc::geteuid() })
                {
                    return Err(Error::Refused(
                        "Host storage must be owned and have one link",
                    ));
                }
            }
        }
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("host.lock"))?;
        lock.try_lock()
            .map_err(|_| Error::Refused("Host directory is already in use"))?;
        Ok(StorageLock(lock))
    }

    pub fn open(directory: &Path) -> Result<Self> {
        let lock = Self::lock_storage(directory)?;
        let mut connection = Connection::open_with_flags(
            directory.join("journal.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(Duration::from_millis(100))?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let application: i64 =
            connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
        let snapshot = if version == 0 && application == 0 {
            let tables: i64 =
                connection.query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0))?;
            if tables != 0 {
                return Err(Error::Refused("unversioned nonempty database"));
            }
            let initial = Snapshot {
                version: SCHEMA_VERSION,
                host_id: random_id()?,
                seq: 0,
                stream: 0,
                stream_seq: 0,
                lifecycle: "new".into(),
                threads: BTreeMap::new(),
                unresolved_probes: BTreeMap::new(),
                tasks: BTreeMap::new(),
                operations: BTreeMap::new(),
            };
            let transaction = connection.transaction()?;
            transaction.execute_batch("CREATE TABLE events (seq INTEGER PRIMARY KEY CHECK(seq > 0), stream INTEGER NOT NULL CHECK(stream > 0), stream_seq INTEGER NOT NULL CHECK(stream_seq > 0), method TEXT NOT NULL, data TEXT NOT NULL, UNIQUE(stream, stream_seq)); CREATE TABLE snapshot (id INTEGER PRIMARY KEY CHECK(id = 1), data TEXT NOT NULL);")?;
            transaction.execute(
                "INSERT INTO snapshot VALUES (1, ?1)",
                [serde_json::to_string(&initial)?],
            )?;
            transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            transaction.commit()?;
            initial
        } else {
            let mut snapshot = Self::load_snapshot(&connection)?;
            if version == 1 {
                snapshot.version = SCHEMA_VERSION;
                let transaction = connection.transaction()?;
                transaction.execute(
                    "UPDATE snapshot SET data = ?1 WHERE id = 1",
                    [serde_json::to_string(&snapshot)?],
                )?;
                transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                transaction.commit()?;
            }
            snapshot
        };
        let mode: String = connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        if mode != "wal" {
            return Err(Error::Refused("SQLite WAL unavailable"));
        }
        connection.pragma_update(None, "synchronous", "FULL")?;
        Ok(Self {
            connection,
            snapshot,
            _lock: lock,
        })
    }

    fn load_snapshot(connection: &Connection) -> Result<Snapshot> {
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let application: i64 =
            connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
        if !matches!(version, 1 | SCHEMA_VERSION) || application != APPLICATION_ID {
            return Err(Error::Refused(
                "unsupported journal schema/application; no downgrade or repair",
            ));
        }
        let check: String = connection.pragma_query_value(None, "quick_check", |row| row.get(0))?;
        if check != "ok" {
            return Err(Error::Refused("journal integrity check failed"));
        }
        let text: String =
            connection.query_row("SELECT data FROM snapshot WHERE id = 1", [], |row| {
                row.get(0)
            })?;
        let mut wire: Value = serde_json::from_str(&text)?;
        if version == 1 {
            let object = wire
                .as_object_mut()
                .ok_or(Error::Refused("invalid v1 snapshot"))?;
            object.entry("tasks").or_insert_with(|| json!({}));
            object.entry("operations").or_insert_with(|| json!({}));
        }
        let snapshot: Snapshot = serde_json::from_value(wire)?;
        let (count, seq): (i64, i64) = connection.query_row(
            "SELECT COUNT(*), COALESCE(MAX(seq), 0) FROM events",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if snapshot.version != version
            || snapshot.host_id.len() != 32
            || !snapshot
                .host_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || snapshot.seq != seq
            || count != seq
        {
            return Err(Error::Refused("journal/snapshot watermark mismatch"));
        }
        if snapshot.tasks.iter().any(|(id, task)| {
            task.task_id != *id
                || task.last_seq <= 0
                || task.last_seq > seq
                || task.stream <= 0
                || task.stream > snapshot.stream
                || !matches!(
                    task.status.as_str(),
                    "submitted"
                        | "running"
                        | "blocked"
                        | "cancel-requested"
                        | "completed"
                        | "failed"
                        | "cancelled"
                        | "unknown"
                )
                || snapshot
                    .operations
                    .get(&task.operation_id)
                    .is_none_or(|operation| {
                        operation.task_id != *id || operation.action != "submit"
                    })
        }) || snapshot.operations.iter().any(|(id, operation)| {
            operation.operation_id != *id
                || operation.last_seq <= 0
                || operation.last_seq > seq
                || !snapshot.tasks.contains_key(&operation.task_id)
                || operation.payload_hash.len() != 64
                || !operation
                    .payload_hash
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
        }) {
            return Err(Error::Refused("invalid task/operation projection"));
        }
        let tail: (i64, i64) = if seq == 0 {
            (0, 0)
        } else {
            connection.query_row(
                "SELECT stream, stream_seq FROM events WHERE seq = ?1",
                [seq],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?
        };
        if tail != (snapshot.stream, snapshot.stream_seq) {
            return Err(Error::Refused("journal/snapshot stream mismatch"));
        }
        Ok(snapshot)
    }

    /// Inspect an existing database without migration or SQLite writes.
    pub fn inspect(directory: &Path) -> Result<Snapshot> {
        if !directory.join("journal.sqlite3").is_file() {
            return Err(Error::Refused("no existing journal"));
        }
        let _lock = Self::lock_storage(directory)?;
        let connection = Connection::open_with_flags(
            directory.join("journal.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        Ok(Self::load_snapshot(&connection)?.offline())
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.clone()
    }

    /// Append external Runtime notifications without granting Host authority.
    /// Reserved names retain their complete raw envelope under a neutral method.
    pub fn append_runtime(&mut self, method: &str, raw: Value) -> Result<Event> {
        let method = if method.starts_with("host/") {
            "runtime/notification"
        } else {
            method
        };
        self.append(method, raw)
    }

    /// Trusted caller entry point. Event and projection advance in one transaction. Callers may publish only
    /// the returned event; an error leaves the in-memory watermark unchanged.
    pub fn append(&mut self, method: &str, data: Value) -> Result<Event> {
        if serde_json::to_vec(&data)?.len() > 1024 * 1024 {
            return Err(Error::Refused("journal event exceeds 1 MiB"));
        }
        let mut snapshot = self.snapshot.clone();
        snapshot.seq = snapshot
            .seq
            .checked_add(1)
            .ok_or(Error::Refused("sequence exhausted"))?;
        if method == "host/started" {
            snapshot.stream = snapshot
                .stream
                .checked_add(1)
                .ok_or(Error::Refused("stream exhausted"))?;
            snapshot.stream_seq = 0;
        }
        snapshot.stream_seq = snapshot
            .stream_seq
            .checked_add(1)
            .ok_or(Error::Refused("stream sequence exhausted"))?;
        snapshot.apply(method, &data);
        let event = Event {
            host_id: snapshot.host_id.clone(),
            seq: snapshot.seq,
            stream: snapshot.stream,
            stream_seq: snapshot.stream_seq,
            method: method.into(),
            data,
        };
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO events VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                event.seq,
                event.stream,
                event.stream_seq,
                event.method,
                serde_json::to_string(&event.data)?
            ],
        )?;
        transaction.execute(
            "UPDATE snapshot SET data = ?1 WHERE id = 1",
            [serde_json::to_string(&snapshot)?],
        )?;
        transaction.commit()?;
        self.snapshot = snapshot;
        Ok(event)
    }

    pub(crate) fn recovery(&self, host: &str, after: Option<i64>) -> Result<Value> {
        if host != self.snapshot.host_id {
            return Err(Error::Refused("cursor belongs to another Host"));
        }
        let Some(after) = after else {
            return Ok(json!({"mode": "snapshot", "snapshot": self.snapshot}));
        };
        if after < 0 || after > self.snapshot.seq {
            return Err(Error::Refused("cursor outside journal"));
        }
        if self.snapshot.seq - after > REPLAY_LIMIT {
            return Ok(json!({"mode": "snapshot", "snapshot": self.snapshot}));
        }
        let mut statement = self.connection.prepare(
            "SELECT seq, stream, stream_seq, method, data FROM events WHERE seq > ?1 ORDER BY seq",
        )?;
        let rows = statement.query_map([after], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut events = Vec::new();
        for row in rows {
            let (seq, stream, stream_seq, method, data) = row?;
            events.push(Event {
                host_id: host.into(),
                seq,
                stream,
                stream_seq,
                method,
                data: serde_json::from_str(&data)?,
            });
        }
        Ok(
            json!({"mode": "replay", "host_id": host, "through_seq": self.snapshot.seq, "events": events}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_close_releases_lock_even_when_a_duplicate_handle_survives() {
        let directory = std::env::temp_dir().join(format!(
            "caidex-h1-lock-{}-{}",
            std::process::id(),
            random_id().unwrap()
        ));
        let journal = Journal::open(&directory).unwrap();
        let duplicate = journal._lock.0.try_clone().unwrap();
        assert!(Journal::open(&directory).is_err());
        drop(journal);
        let replacement = Journal::open(&directory).unwrap();
        drop(duplicate);
        assert!(Journal::open(&directory).is_err());
        drop(replacement);
        drop(Journal::open(&directory).unwrap());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn actual_sqlite_full_rolls_back_event_and_snapshot() {
        let directory = std::env::temp_dir().join(format!(
            "caidex-h1-full-{}-{}",
            std::process::id(),
            random_id().unwrap()
        ));
        let mut journal = Journal::open(&directory).unwrap();
        journal.append("host/started", json!({})).unwrap();
        let before = journal.snapshot();
        let pages: i64 = journal
            .connection
            .pragma_query_value(None, "page_count", |row| row.get(0))
            .unwrap();
        journal
            .connection
            .pragma_update(None, "max_page_count", pages)
            .unwrap();
        let error = journal
            .append("large", json!("x".repeat(128 * 1024)))
            .unwrap_err();
        assert!(
            matches!(error, Error::Sqlite(rusqlite::Error::SqliteFailure(code, _)) if code.code == rusqlite::ErrorCode::DiskFull)
        );
        assert_eq!(journal.snapshot(), before);
        let count: i64 = journal
            .connection
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, before.seq);
        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
