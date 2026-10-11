//! Host-owned inference only. No Runtime calls; operation metadata is separate from journal.
use crate::{Error, Result, Snapshot};
use caidex_chat::{
    LocalModel,
    memory::{self, *},
};
use caidex_credentials::{Id, Redactor, Secret};
use caidex_model_core::{CancellationToken, CredentialRequirement};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};
use tokio::{sync::mpsc, task::JoinSet};

#[derive(Clone)]
pub struct MemoryProfile {
    pub id: String,
    pub model: LocalModel,
    pub purpose: Purpose,
    pub space: Option<Space>,
}
/// Host operator policy, independent of the client-local Memory permissions and account session.
pub struct MemoryGrant {
    pub client_id: String,
    pub permissions: Vec<Permission>,
    /// Exact completed task IDs and local Memory scopes; never a whole journal grant.
    pub evidence: BTreeMap<String, Scope>,
}
pub(crate) enum Command {
    GrantEvidence {
        host_id: String,
        client_id: String,
        scope: Scope,
        task_id: String,
    },
    Models {
        host_id: String,
    },
    Submit {
        host_id: String,
        operation_id: String,
        request: Box<InferenceRequest>,
    },
    Release {
        host_id: String,
        operation_id: String,
    },
    Status {
        host_id: String,
        operation_id: String,
    },
    Cancel {
        host_id: String,
        operation_id: String,
    },
    Evidence {
        host_id: String,
        scope: Scope,
        task_id: String,
    },
}
struct Running {
    cancel: CancellationToken,
    request: InferenceRequest,
    output: Option<InferenceOutput>,
    finished: Option<std::time::Instant>,
}
pub(crate) struct Completion {
    client: String,
    id: String,
    result: caidex_chat::Result<InferenceOutput>,
}
pub struct MemoryService {
    db: Connection,
    _lock: std::fs::File,
    owner: Id,
    profiles: BTreeMap<String, MemoryProfile>,
    grants: BTreeMap<String, MemoryGrant>,
    guards: Vec<Arc<Redactor>>,
    running: BTreeMap<(String, String), Running>,
    pub(crate) jobs: JoinSet<()>,
    pub(crate) receive: mpsc::Receiver<Completion>,
    send: mpsc::Sender<Completion>,
}
impl MemoryService {
    /// Open while the enclosing Host holds the journal directory's exclusive lock.
    /// Profiles, egress policies and keys are explicitly supplied by the Host operator.
    pub fn open(
        directory: &Path,
        owner: Id,
        profiles: Vec<MemoryProfile>,
        grants: Vec<MemoryGrant>,
    ) -> Result<Self> {
        crate::private_directory(directory)?;
        for name in [
            "memory-operations.lock",
            "memory-operations.sqlite3",
            "memory-operations.sqlite3-journal",
        ] {
            if let Ok(meta) = std::fs::symlink_metadata(directory.join(name)) {
                if !meta.is_file() || meta.is_symlink() {
                    return Err(Error::Refused("unsafe Memory storage entry"));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if meta.nlink() != 1 || meta.uid() != unsafe { libc::geteuid() } {
                        return Err(Error::Refused(
                            "Memory storage must be owned and have one link",
                        ));
                    }
                }
            }
        }
        let lock = std::fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("memory-operations.lock"))?;
        lock.try_lock()
            .map_err(|_| Error::Refused("Memory ledger already in use"))?;
        let path = directory.join("memory-operations.sqlite3");
        let mut db = Connection::open(path)?;
        db.busy_timeout(Duration::from_secs(1))?;
        db.pragma_update(None, "journal_mode", "DELETE")?;
        db.pragma_update(None, "synchronous", "FULL")?;
        db.pragma_update(None, "max_page_count", 1024)?;
        let app: i64 = db.pragma_query_value(None, "application_id", |r| r.get(0))?;
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if app == 0 && version == 0 {
            let tables: i64 = db.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type='table'",
                [],
                |r| r.get(0),
            )?;
            if tables != 0 {
                return Err(Error::Refused("unrecognized Memory database"));
            }
            let tx = db.transaction()?;
            tx.execute_batch("CREATE TABLE operations(client TEXT NOT NULL,id TEXT NOT NULL,hash TEXT NOT NULL,state TEXT NOT NULL CHECK(state IN('running','done','failed','cancelled','unknown')),PRIMARY KEY(client,id)); PRAGMA application_id=1128352077; PRAGMA user_version=1;")?;
            tx.commit()?;
        } else if app != 1128352077 || version != 1 {
            return Err(Error::Refused("unknown Memory ledger schema"));
        }
        // Results are deliberately not stored on Host. A restarted process cannot promise delivery.
        db.execute(
            "UPDATE operations SET state='unknown' WHERE state IN('running','done')",
            [],
        )?;
        if profiles.len() > 16 || grants.len() > 64 {
            return Err(Error::Refused("Memory policy capacity exceeded"));
        }
        let mut registered = BTreeMap::new();
        let mut guards = vec![];
        for profile in profiles {
            if profile.id.is_empty()
                || profile.id.len() > 64
                || !profile
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
                || profile.purpose == Purpose::Chat
                || (profile.purpose == Purpose::Embedding) != profile.space.is_some()
                || registered.contains_key(&profile.id)
            {
                return Err(Error::Refused("invalid Memory profile"));
            }
            check_owner(&profile.model, &owner)?;
            let target = profile
                .model
                .provider
                .memory_target(&profile.model.model, profile.space.is_some())
                .map_err(|_| Error::Refused("Memory target unavailable"))?;
            if let Some(space) = &profile.space {
                space
                    .id()
                    .map_err(|_| Error::Refused("invalid embedding space"))?;
                if space.target != target {
                    return Err(Error::Refused("embedding target mismatch"));
                }
            }
            if let Some(guard) = profile.model.provider.memory_guard(&profile.model.model) {
                guards.push(guard);
            }
            registered.insert(profile.id.clone(), profile);
        }
        let mut bindings = BTreeMap::new();
        for grant in grants {
            if grant.client_id.is_empty()
                || grant.client_id.len() > 64
                || grant.permissions.len() > 1024
                || grant.evidence.len() > 1024
                || bindings.contains_key(&grant.client_id)
            {
                return Err(Error::Refused("invalid Memory binding"));
            }
            bindings.insert(grant.client_id.clone(), grant);
        }
        let (send, receive) = mpsc::channel(8);
        Ok(Self {
            db,
            _lock: lock,
            owner,
            profiles: registered,
            grants: bindings,
            guards,
            running: BTreeMap::new(),
            jobs: JoinSet::new(),
            send,
            receive,
        })
    }
    pub(crate) async fn next(&mut self) -> Option<Completion> {
        loop {
            tokio::select! { value = self.receive.recv()=>return value, _=self.jobs.join_next(), if !self.jobs.is_empty()=>() }
        }
    }
    pub(crate) fn protect(&mut self, secret: &str) -> Result<()> {
        let guard = Arc::new(Redactor::default());
        guard
            .register(
                &Secret::new(secret.into()).map_err(|_| Error::Refused("invalid Host secret"))?,
            )
            .map_err(|_| Error::Refused("invalid Host secret"))?;
        self.guards.push(guard);
        Ok(())
    }
    fn safe(&self, value: &Value) -> bool {
        let text = value.to_string();
        !Redactor::default().contains_secret(&text)
            && !self.guards.iter().any(|g| g.contains_secret(&text))
    }
    fn info(&self, snapshot: &Snapshot, profile: &MemoryProfile) -> Result<HostModelInfo> {
        let target = profile
            .model
            .provider
            .memory_target(&profile.model.model, profile.space.is_some())
            .map_err(|_| Error::Refused("Memory target unavailable"))?;
        let target = host_target(&snapshot.host_id, &profile.id, &target);
        let space = profile.space.clone().map(|mut s| {
            s.target = target.clone();
            s
        });
        Ok(HostModelInfo {
            profile: profile.id.clone(),
            purpose: profile.purpose,
            target,
            space,
        })
    }
    fn permitted(
        &self,
        snapshot: &Snapshot,
        client: &str,
        request: &InferenceRequest,
    ) -> Result<HostModelInfo> {
        request
            .validate()
            .map_err(|_| Error::Refused("invalid Memory request"))?;
        let profile = self
            .profiles
            .get(&request.profile)
            .ok_or(Error::Refused("Memory profile denied"))?;
        check_owner(&profile.model, &self.owner)?;
        let info = self.info(snapshot, profile)?;
        if request.target != info.target
            || request.purpose() != info.purpose
            || request.space != info.space
            || !self.grants.get(client).is_some_and(|g| {
                g.permissions
                    .iter()
                    .any(|p| p.permits(&info.target, info.purpose, &request.draft))
            })
            || !self.safe(&json!(request))
        {
            return Err(Error::Refused("Memory egress denied"));
        }
        Ok(info)
    }
    pub(crate) fn command(
        &mut self,
        snapshot: &Snapshot,
        client: &str,
        command: Command,
    ) -> Result<Value> {
        let host = match &command {
            Command::GrantEvidence { host_id, .. }
            | Command::Models { host_id }
            | Command::Submit { host_id, .. }
            | Command::Release { host_id, .. }
            | Command::Status { host_id, .. }
            | Command::Cancel { host_id, .. }
            | Command::Evidence { host_id, .. } => host_id,
        };
        if host != &snapshot.host_id {
            return Err(Error::Refused("Memory Host identity mismatch"));
        }
        match command {
            Command::GrantEvidence {
                client_id,
                scope,
                task_id,
                ..
            } => {
                if client != crate::access::OWNER
                    || !snapshot
                        .tasks
                        .get(&task_id)
                        .is_some_and(|t| t.status == "completed")
                {
                    return Err(Error::Refused(
                        "completed Memory evidence requires Host owner grant",
                    ));
                }
                let grant = self
                    .grants
                    .get_mut(&client_id)
                    .ok_or(Error::Refused("Memory client binding not configured"))?;
                if grant.evidence.len() >= 1024 {
                    return Err(Error::Refused("Memory evidence capacity reached"));
                }
                grant.evidence.insert(task_id, scope);
                Ok(json!({"granted":true}))
            }
            Command::Models { .. } => {
                let mut infos = vec![];
                for p in self.profiles.values() {
                    let info = self.info(snapshot, p)?;
                    if self.grants.get(client).is_some_and(|g| {
                        g.permissions
                            .iter()
                            .any(|p| p.target == info.target && p.purpose == info.purpose)
                    }) {
                        infos.push(info);
                    }
                }
                if !self.safe(&json!(infos)) || serde_json::to_vec(&infos)?.len() > 60 * 1024 {
                    return Err(Error::Refused("unsafe Memory model descriptor"));
                }
                Ok(json!(infos))
            }
            Command::Submit {
                operation_id,
                request,
                ..
            } => {
                self.permitted(snapshot, client, &request)?;
                if operation_id.len() != 36
                    || !operation_id
                        .bytes()
                        .all(|c| c.is_ascii_hexdigit() || c == b'-')
                {
                    return Err(Error::Refused("invalid Memory operation ID"));
                }
                let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&request)?));
                let existing: Option<(String, String)> = self
                    .db
                    .query_row(
                        "SELECT hash,state FROM operations WHERE client=?1 AND id=?2",
                        params![client, operation_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                if let Some((old, state)) = existing {
                    if old != hash {
                        return Err(Error::Refused("Memory operation payload mismatch"));
                    }
                    return Ok(json!({"operation_id":operation_id,"state":state}));
                }
                let count: i64 = self
                    .db
                    .query_row("SELECT count(*) FROM operations", [], |r| r.get(0))?;
                if count >= 4096 || self.running.len() >= 8 {
                    return Err(Error::Refused("Memory operation capacity reached"));
                }
                // Persist intent before spawning any model work. No payload or key is persisted.
                self.db.execute(
                    "INSERT INTO operations VALUES(?1,?2,?3,'running')",
                    params![client, operation_id, hash],
                )?;
                let model = self.profiles[&request.profile].model.clone();
                let cancel = CancellationToken::new();
                self.running.insert(
                    (client.into(), operation_id.clone()),
                    Running {
                        cancel: cancel.clone(),
                        request: (*request).clone(),
                        output: None,
                        finished: None,
                    },
                );
                let send = self.send.clone();
                let client = client.to_owned();
                let id = operation_id.clone();
                // Host's actual embedding target differs from its composite client-facing identity.
                let mut local = *request;
                local.target = model
                    .provider
                    .memory_target(&model.model, local.space.is_some())
                    .map_err(|_| Error::Refused("Memory target unavailable"))?;
                if let Some(space) = &mut local.space {
                    space.target = model
                        .provider
                        .memory_target(&model.model, true)
                        .map_err(|_| Error::Refused("embedding target unavailable"))?;
                }
                self.jobs.spawn(async move {
                    let result = tokio::select! { biased; _=cancel.cancelled()=>Err(caidex_chat::Error::Cancelled), _=tokio::time::sleep(Duration::from_secs(120))=>Err(caidex_chat::Error::HostUnknown), r=memory::infer(&model,&local,cancel.clone())=>r };
                    let _ = send.send(Completion { client, id, result }).await;
                });
                Ok(json!({"operation_id":operation_id,"state":"running"}))
            }
            Command::Release { operation_id, .. } => {
                let state: Option<String> = self
                    .db
                    .query_row(
                        "SELECT state FROM operations WHERE client=?1 AND id=?2",
                        params![client, operation_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                if state.as_deref() == Some("running") {
                    return Err(Error::Refused("Memory operation still running"));
                }
                if let Some(running) = self.running.remove(&(client.into(), operation_id)) {
                    running.cancel.cancel();
                }
                Ok(json!({"released":true}))
            }
            Command::Status { operation_id, .. } => {
                let state: Option<(String, String)> = self
                    .db
                    .query_row(
                        "SELECT state,hash FROM operations WHERE client=?1 AND id=?2",
                        params![client, operation_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                let (state, hash) = state.ok_or(Error::Refused("Memory operation unknown"))?;
                if let Some(running) = self.running.get(&(client.into(), operation_id.clone())) {
                    self.permitted(snapshot, client, &running.request)?;
                    if state == "done" {
                        return Ok(
                            json!({"operation_id":operation_id,"state":state,"payload_hash":hash,"output":running.output}),
                        );
                    }
                }
                Ok(
                    json!({"operation_id":operation_id,"payload_hash":hash,"state":if state == "done" { "unknown" } else { &state }}),
                )
            }
            Command::Cancel { operation_id, .. } => {
                if let Some(running) = self.running.remove(&(client.into(), operation_id.clone())) {
                    running.cancel.cancel();
                }
                self.db.execute("UPDATE operations SET state='cancelled' WHERE client=?1 AND id=?2 AND state IN('running','done')",params![client,operation_id])?;
                Ok(json!({"cancelled":true}))
            }
            Command::Evidence { scope, task_id, .. } => {
                if self
                    .grants
                    .get(client)
                    .and_then(|g| g.evidence.get(&task_id))
                    != Some(&scope)
                {
                    return Err(Error::Refused("Memory evidence denied"));
                }
                let task = snapshot
                    .tasks
                    .get(&task_id)
                    .filter(|t| t.status == "completed")
                    .ok_or(Error::Refused("Memory evidence unavailable"))?;
                let value = json!({"host_id":snapshot.host_id,"task_id":task.task_id,"status":task.status,"thread_id":task.thread_id,"turn_id":task.turn_id,"last_seq":task.last_seq,"codex_version":caidex_runtime::CODEX_VERSION});
                if !self.safe(&value) || serde_json::to_vec(&value)?.len() > 60 * 1024 {
                    return Err(Error::Refused("Memory evidence unsafe"));
                }
                Ok(value)
            }
        }
    }
    pub(crate) fn complete(&mut self, snapshot: &Snapshot, completion: Completion) -> Result<()> {
        let key = (completion.client.clone(), completion.id.clone());
        let Some(running) = self.running.get(&key) else {
            return Ok(());
        };
        let authorized = crate::access::permits(
            &snapshot.clients,
            &completion.client,
            Some(crate::Scope::Memory),
        ) && self
            .permitted(snapshot, &completion.client, &running.request)
            .is_ok()
            && !running.cancel.is_cancelled();
        let state = if !authorized {
            "cancelled"
        } else {
            match &completion.result {
                Ok(output)
                    if serde_json::to_vec(output).is_ok_and(|b| b.len() <= 60 * 1024)
                        && output.validate(&running.request).is_ok()
                        && self.safe(&json!(output)) =>
                {
                    "done"
                }
                Err(caidex_chat::Error::HostUnknown) => "unknown",
                _ => "failed",
            }
        };
        self.db.execute(
            "UPDATE operations SET state=?1 WHERE client=?2 AND id=?3 AND state='running'",
            params![state, completion.client, completion.id],
        )?;
        if state == "done" {
            let running = self.running.get_mut(&key).expect("present completion");
            running.output = completion.result.ok();
            running.finished = Some(std::time::Instant::now());
        } else if let Some(running) = self.running.remove(&key) {
            running.cancel.cancel();
        }
        Ok(())
    }
    pub(crate) fn expire(&mut self) -> Result<()> {
        let expired: Vec<_> = self
            .running
            .iter()
            .filter(|(_, r)| {
                r.finished
                    .is_some_and(|t| t.elapsed() >= Duration::from_secs(120))
            })
            .map(|(key, _)| key.clone())
            .collect();
        for (client, id) in expired {
            self.db.execute(
                "UPDATE operations SET state='unknown' WHERE client=?1 AND id=?2 AND state='done'",
                params![client, id],
            )?;
            self.running.remove(&(client, id));
        }
        Ok(())
    }
    pub(crate) fn revoke(&mut self, client: &str) -> Result<()> {
        self.running.retain(|(id, _), running| {
            if id == client {
                running.cancel.cancel();
                false
            } else {
                true
            }
        });
        self.db.execute("UPDATE operations SET state='cancelled' WHERE client=?1 AND state IN('running','done')",[client])?;
        Ok(())
    }
}
impl Drop for MemoryService {
    fn drop(&mut self) {
        for running in self.running.values() {
            running.cancel.cancel();
        }
        self.jobs.abort_all();
    }
}
fn check_owner(model: &LocalModel, owner: &Id) -> Result<()> {
    match model
        .provider
        .credential_requirements(&model.model)
        .map_err(|_| Error::Refused("Memory credentials unavailable"))?
    {
        CredentialRequirement::None => (),
        CredentialRequirement::Bearer { reference }
        | CredentialRequirement::ApiKey { reference } => {
            if &reference.owner != owner || model.provider.memory_guard(&model.model).is_none() {
                return Err(Error::Refused(
                    "Memory requires Host-owned guarded credentials",
                ));
            }
        }
    }
    Ok(())
}
