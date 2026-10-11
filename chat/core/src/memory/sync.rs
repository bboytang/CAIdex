//! Durable Memory control and explicit per-memory upload consent; one bounded scope per round.
use super::{store, *};
use crate::{Namespace, client::State};
use caidex_auth::{ChatError, ConfirmedSession};
use caidex_cloud_core::memory::{
    Change, Control, Mode, PROTOCOL, Page, Permit, Pull, Push, Receipt, SetControl,
};
use rusqlite::{OptionalExtension, Transaction, params};

#[derive(Serialize)]
pub struct Status {
    pub control: Option<Control>,
    pub enabled: bool,
    pub pending_control: bool,
    pub control_conflict: bool,
    pub needs_epoch_choice: bool,
    pub queued: i64,
    pub blocked: i64,
}
#[derive(Serialize)]
pub struct ScopeStatus {
    pub scope: Scope,
    pub permit: Option<Permit>,
    pub cursor: i64,
    pub needs_epoch_choice: bool,
}
#[derive(Default, Serialize)]
pub struct Round {
    pub uploaded: bool,
    pub downloaded: bool,
    pub control: bool,
    pub needs_epoch_choice: bool,
    pub more: bool,
}
fn network(e: ChatError) -> Error {
    match e {
        ChatError::Offline => Error::SyncOffline,
        ChatError::Denied => Error::SyncDenied,
        ChatError::Conflict => Error::SyncConflict,
        ChatError::Invalid => Error::Invalid,
    }
}
fn value<T: Serialize>(v: &T) -> Result<serde_json::Value> {
    serde_json::to_value(v).map_err(|_| Error::Invalid)
}
fn reply<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Result<T> {
    serde_json::from_value(v).map_err(|_| Error::Invalid)
}
fn check(
    s: &State,
    session: &Arc<ConfirmedSession>,
    generation: u64,
    namespace: &Namespace,
) -> Result<()> {
    if s.generation != generation
        || s.cache.namespace() != namespace
        || !s
            .sync_session
            .as_ref()
            .is_some_and(|v| Arc::ptr_eq(v, session))
    {
        return Err(Error::Cancelled);
    }
    Ok(())
}
fn status(s: &State) -> Result<Status> {
    let (c, intent, stop, conflict, epoch): (
        Option<String>,
        Option<String>,
        Option<String>,
        bool,
        i64,
    ) = s.cache.db.query_row(
        "SELECT control,intent,stop,conflict,epoch FROM memory_sync",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let control: Option<Control> = c.map(|v| decode(&v)).transpose()?;
    let needs_epoch_choice = control.as_ref().is_some_and(|c| c.epoch != epoch);
    let queued = s.cache.db.query_row(
        "SELECT count(*) FROM memory_outbox o JOIN memory_sync_uploads u ON u.memory=o.entity",
        [],
        |r| r.get(0),
    )?;
    let blocked = s.cache.db.query_row(
        "SELECT count(*) FROM memory_outbox WHERE blocked=1",
        [],
        |r| r.get(0),
    )?;
    let enabled = control
        .as_ref()
        .is_some_and(|c| c.enabled && !c.delete_pending)
        && intent.is_none()
        && stop.is_none()
        && !needs_epoch_choice;
    Ok(Status {
        control,
        enabled,
        pending_control: intent.is_some() || stop.is_some(),
        control_conflict: conflict,
        needs_epoch_choice,
        queued,
        blocked,
    })
}
fn request(s: &State, mode: Mode, scopes: Vec<Scope>) -> Result<SetControl> {
    Ok(SetControl {
        protocol: PROTOCOL,
        operation: Uuid::new_v4(),
        base_version: status(s)?.control.map(|c| c.version).unwrap_or(0),
        mode,
        scopes,
    })
}
impl MemoryEngine<'_> {
    pub fn sync_status(&self) -> Result<Status> {
        status(&*self.chat.state()?)
    }
    /// Scope enables cloud access; exact local memory IDs separately authorize uploads.
    pub fn set_sync(&self, mode: Mode, scopes: &[Scope], memories: &[Uuid]) -> Result<()> {
        let mut s = self.chat.state()?;
        if s.cache.namespace().owner_user_id().is_none() {
            return Err(Error::SyncDenied);
        }
        if scopes.len() > 64
            || memories.len() > 4096
            || scopes
                .iter()
                .enumerate()
                .any(|(i, v)| scopes[..i].contains(v))
            || (mode == Mode::Enable && scopes.is_empty())
            || (mode != Mode::Enable && (!scopes.is_empty() || !memories.is_empty()))
        {
            return Err(Error::Invalid);
        }
        for scope in scopes {
            store::authorize(&s, scope)?;
        }
        let pending: Option<String> =
            s.cache
                .db
                .query_row("SELECT intent FROM memory_sync", [], |r| r.get(0))?;
        if pending.is_some() {
            if mode == Mode::Enable {
                return Err(Error::SyncConflict);
            }
            s.cache
                .db
                .execute("UPDATE memory_sync SET stop=?1", [encode(&mode)?])?;
            return Ok(());
        }
        if mode == Mode::Enable && status(&s)?.needs_epoch_choice {
            return Err(Error::SyncConflict);
        }
        let r = request(&s, mode.clone(), scopes.to_vec())?;
        let selected = memories
            .iter()
            .map(|id| {
                let m = store::load(&s, *id)?;
                if m.state == "deleted"
                    || m.revisions
                        .iter()
                        .any(|r| r.draft.privacy != Privacy::Shareable)
                    || !m.revisions.iter().all(|r| scopes.contains(&r.draft.scope))
                {
                    return Err(Error::SyncDenied);
                }
                Ok(m)
            })
            .collect::<Result<Vec<_>>>()?;
        let tx = s.cache.db.transaction()?;
        tx.execute(
            "UPDATE memory_sync SET intent=?1,stop=?2,conflict=0,idle=0",
            params![
                encode(&r)?,
                if mode == Mode::Enable {
                    None
                } else {
                    Some(encode(&mode)?)
                }
            ],
        )?;
        if mode == Mode::Enable {
            for scope in scopes {
                tx.execute(
                    "INSERT OR IGNORE INTO memory_sync_scopes(scope) VALUES(?1)",
                    [scope.key()],
                )?;
            }
            for m in selected {
                select_upload(&tx, &m)?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    /// Inherit Enabled without changing account settings or adopting unselected history.
    pub fn select_uploads(&self, memories: &[Uuid]) -> Result<()> {
        let mut s = self.chat.state()?;
        let current = status(&s)?;
        if !current.enabled || memories.len() > 4096 {
            return Err(Error::SyncDenied);
        }
        let scopes = current.control.ok_or(Error::SyncDenied)?.scopes;
        let selected = memories
            .iter()
            .map(|id| {
                let m = store::load(&s, *id)?;
                if m.state == "deleted"
                    || m.revisions.iter().any(|r| {
                        r.draft.privacy != Privacy::Shareable || !scopes.contains(&r.draft.scope)
                    })
                {
                    return Err(Error::SyncDenied);
                }
                Ok(m)
            })
            .collect::<Result<Vec<_>>>()?;
        let tx = s.cache.db.transaction()?;
        for m in selected {
            select_upload(&tx, &m)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn discard_control_conflict(&self) -> Result<()> {
        if self.chat.state()?.cache.db.execute(
            "UPDATE memory_sync SET intent=NULL,conflict=0 WHERE conflict=1",
            [],
        )? != 1
        {
            return Err(Error::SyncConflict);
        }
        Ok(())
    }
    /// Only a server-confirmed conflict may be discarded; unknown operations retain their IDs.
    pub fn discard_upload_conflict(&self, operation: Uuid) -> Result<()> {
        if self.chat.state()?.cache.db.execute(
            "DELETE FROM memory_outbox WHERE operation=?1 AND blocked=1",
            [operation.to_string()],
        )? != 1
        {
            return Err(Error::SyncConflict);
        }
        Ok(())
    }
    /// Apply the B instruction. Retained copies lose cloud links and upload consent.
    pub fn reconcile_deleted_epoch(&self, retain: bool) -> Result<()> {
        let mut s = self.chat.state()?;
        let current = status(&s)?.control.ok_or(Error::SyncConflict)?;
        let local: i64 = s
            .cache
            .db
            .query_row("SELECT epoch FROM memory_sync", [], |r| r.get(0))?;
        if local == current.epoch {
            return Err(Error::SyncConflict);
        }
        let owner = String::from(
            s.cache
                .namespace()
                .owner_user_id()
                .ok_or(Error::SyncDenied)?
                .clone(),
        );
        s.memory.cancel_all();
        let tx = s.cache.db.transaction()?;
        detach(&tx, &owner, None, retain)?;
        tx.execute("UPDATE memory_sync SET epoch=?1", [current.epoch])?;
        tx.commit()?;
        Ok(())
    }
    pub fn reconcile_project_epoch(&self, scope: &Scope, retain: bool) -> Result<()> {
        let mut s = self.chat.state()?;
        let key = scope.key();
        let (text, epoch): (String, i64) = s.cache.db.query_row(
            "SELECT permit,epoch FROM memory_sync_scopes WHERE scope=?1",
            [&key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let permit: Permit = decode(&text)?;
        if permit.epoch == epoch {
            return Err(Error::SyncConflict);
        }
        s.memory.cancel_all();
        let tx = s.cache.db.transaction()?;
        detach(&tx, &permit.owner.to_string(), Some(&key), retain)?;
        tx.execute(
            "UPDATE memory_sync_scopes SET epoch=?2,cursor=0 WHERE scope=?1",
            params![key, permit.epoch],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Explicit human recreation uses a new ID and a fresh upload operation, never an old job.
    pub fn recreate(&self, draft: Draft) -> Result<Uuid> {
        store::insert_mode(&mut *self.chat.state()?, draft, false, true)
    }
    pub fn sync_scopes(&self) -> Result<Vec<ScopeStatus>> {
        let s = self.chat.state()?;
        let keys = status(&s)?
            .control
            .map(|c| c.scopes.iter().map(Scope::key).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut stmt = s.cache.db.prepare("SELECT scope,permit,cursor,epoch FROM memory_sync_scopes WHERE scope IN(SELECT value FROM json_each(?1)) ORDER BY scope LIMIT 65")?;
        let rows = stmt
            .query_map([encode(&keys)?], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if rows.len() > 64 {
            return Err(Error::UnsafeStorage);
        }
        rows.into_iter()
            .map(|(scope, text, cursor, epoch)| {
                let permit: Option<Permit> = text.map(|v| decode(&v)).transpose()?;
                let pending = permit.as_ref().is_some_and(|p| p.epoch != epoch);
                Ok(ScopeStatus {
                    scope: store::scope_from_key(&scope)?,
                    permit,
                    cursor,
                    needs_epoch_choice: pending,
                })
            })
            .collect()
    }
    pub fn sync_operations(
        &self,
        after: i64,
        limit: usize,
    ) -> Result<Vec<(crate::OutboxEntry, bool)>> {
        if after < 0 || !(1..=100).contains(&limit) {
            return Err(Error::Invalid);
        }
        let s = self.chat.state()?;
        let mut stmt=s.cache.db.prepare("SELECT sequence,operation,entity,kind,blocked FROM memory_outbox WHERE sequence>?1 ORDER BY sequence LIMIT ?2")?;
        let rows = stmt
            .query_map(params![after, limit as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, bool>(4)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(sequence, operation, entity, kind, blocked)| {
                Ok((
                    crate::OutboxEntry {
                        sequence,
                        operation: Uuid::parse_str(&operation).map_err(|_| Error::UnsafeStorage)?,
                        entity: Uuid::parse_str(&entity).map_err(|_| Error::UnsafeStorage)?,
                        kind,
                    },
                    blocked,
                ))
            })
            .collect()
    }
    /// A proof is transient. It is never persisted with the intent or operation receipt.
    pub async fn sync_once(&self, deletion_proof: Option<&str>) -> Result<Round> {
        let (session, generation, namespace) = {
            let s = self.chat.state()?;
            (
                s.sync_session.clone().ok_or(Error::SyncOffline)?,
                s.generation,
                s.cache.namespace().clone(),
            )
        };
        let current: Control = reply(
            session
                .memory_request("status", serde_json::json!({"protocol":PROTOCOL}))
                .await
                .map_err(network)?,
        )?;
        if !current.valid() {
            return Err(Error::Invalid);
        }
        let pending = {
            let s = self.chat.state()?;
            check(&s, &session, generation, &namespace)?;
            if status(&s)?
                .control
                .is_some_and(|c| c.version > current.version)
            {
                return Err(Error::Cancelled);
            }
            s.cache
                .db
                .execute("UPDATE memory_sync SET control=?1", [encode(&current)?])?;
            let epoch: i64 = s
                .cache
                .db
                .query_row("SELECT epoch FROM memory_sync", [], |r| r.get(0))?;
            if epoch == 0 {
                s.cache
                    .db
                    .execute("UPDATE memory_sync SET epoch=?1", [current.epoch])?;
            }
            let stop: Option<String> =
                s.cache
                    .db
                    .query_row("SELECT stop FROM memory_sync", [], |r| r.get(0))?;
            let mut intent: Option<String> =
                s.cache
                    .db
                    .query_row("SELECT intent FROM memory_sync", [], |r| r.get(0))?;
            if intent.is_none()
                && let Some(stop) = stop
            {
                let r = request(&s, decode(&stop)?, vec![])?;
                let text = encode(&r)?;
                s.cache
                    .db
                    .execute("UPDATE memory_sync SET intent=?1", [&text])?;
                intent = Some(text);
            }
            if let Some(text) = intent {
                let mut r: SetControl = decode(&text)?;
                if r.base_version == 0 {
                    r.base_version = current.version;
                    s.cache
                        .db
                        .execute("UPDATE memory_sync SET intent=?1", [encode(&r)?])?;
                }
                Some(r)
            } else {
                None
            }
        };
        if let Some(r) = pending {
            let mut body = value(&r)?;
            if r.mode == Mode::Delete
                && let Some(proof) = deletion_proof
            {
                body["proof"] = proof.into();
            }
            let result = session.memory_request("control", body).await;
            if matches!(result, Err(ChatError::Conflict)) {
                let s = self.chat.state()?;
                check(&s, &session, generation, &namespace)?;
                s.cache
                    .db
                    .execute("UPDATE memory_sync SET conflict=1", [])?;
            }
            let accepted: Control = reply(result.map_err(network)?)?;
            if !accepted.valid()
                || accepted.version != r.base_version + 1
                || accepted.enabled != (r.mode == Mode::Enable)
            {
                return Err(Error::Invalid);
            }
            let observed = if current.version >= accepted.version {
                &current
            } else {
                &accepted
            };
            let mut s = self.chat.state()?;
            check(&s, &session, generation, &namespace)?;
            // A stop arriving during an unknown enable remains durable and wins locally.
            let stop: Option<String> =
                s.cache
                    .db
                    .query_row("SELECT stop FROM memory_sync", [], |r| r.get(0))?;
            let clear =
                r.mode != Mode::Enable && stop.as_deref() == Some(encode(&r.mode)?.as_str());
            let tx = s.cache.db.transaction()?;
            tx.execute(
                "UPDATE memory_sync SET control=?1,intent=NULL,conflict=0,stop=?2,idle=0",
                params![encode(observed)?, if clear { None } else { stop }],
            )?;
            for scope in &observed.scopes {
                tx.execute(
                    "INSERT OR IGNORE INTO memory_sync_scopes(scope) VALUES(?1)",
                    [scope.key()],
                )?;
            }
            tx.commit()?;
            return Ok(Round {
                control: true,
                needs_epoch_choice: status(&s)?.needs_epoch_choice,
                ..Round::default()
            });
        }
        if current.delete_pending {
            let done: Control = reply(
                session
                    .memory_request(
                        "cleanup",
                        serde_json::json!({"protocol":PROTOCOL,"epoch":current.epoch}),
                    )
                    .await
                    .map_err(network)?,
            )?;
            if done.epoch != current.epoch || done.enabled {
                return Err(Error::Invalid);
            }
            let s = self.chat.state()?;
            check(&s, &session, generation, &namespace)?;
            s.cache
                .db
                .execute("UPDATE memory_sync SET control=?1", [encode(&done)?])?;
            return Ok(Round {
                control: true,
                needs_epoch_choice: status(&s)?.needs_epoch_choice,
                ..Round::default()
            });
        }
        let scope = {
            let s = self.chat.state()?;
            check(&s, &session, generation, &namespace)?;
            let local = status(&s)?;
            if local.needs_epoch_choice {
                return Ok(Round {
                    needs_epoch_choice: true,
                    ..Round::default()
                });
            }
            if !local.enabled {
                return Ok(Round::default());
            }
            // Fresh devices inherit cloud scope access but receive no local upload consent.
            for scope in &current.scopes {
                if let Scope::Project { project_id } = scope {
                    s.cache.db.execute(
                        "INSERT OR IGNORE INTO memory_projects VALUES(?1)",
                        [String::from(project_id.clone())],
                    )?;
                }
                s.cache.db.execute(
                    "INSERT OR IGNORE INTO memory_sync_scopes(scope) VALUES(?1)",
                    [scope.key()],
                )?;
            }
            let key:Option<String>=s.cache.db.query_row("SELECT scope FROM memory_sync_scopes WHERE scope IN(SELECT value FROM json_each(?1)) ORDER BY (scope<=coalesce((SELECT last_scope FROM memory_sync),'')),scope LIMIT 1",[encode(&current.scopes.iter().map(Scope::key).collect::<Vec<_>>())?],|r|r.get(0)).optional()?;
            key.map(|key| store::scope_from_key(&key)).transpose()?
        };
        let Some(scope) = scope else {
            return Ok(Round::default());
        };
        let p = match &scope {
            Scope::Personal => None,
            Scope::Project { project_id } => Some(
                Uuid::parse_str(&String::from(project_id.clone())).map_err(|_| Error::Invalid)?,
            ),
        };
        let permit: Permit = reply(
            session
                .memory_request(
                    "scope",
                    serde_json::json!({"protocol":PROTOCOL,"project":p}),
                )
                .await
                .map_err(network)?,
        )?;
        if permit.project != p || permit.version < 1 || permit.epoch < 1 || permit.scope_version < 1
        {
            return Err(Error::Invalid);
        }
        let (queued, after) = {
            let s = self.chat.state()?;
            check(&s, &session, generation, &namespace)?;
            if !status(&s)?.enabled {
                return Err(Error::Cancelled);
            }
            let (old, ack): (Option<String>, i64) = s.cache.db.query_row(
                "SELECT permit,epoch FROM memory_sync_scopes WHERE scope=?1",
                [scope.key()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if old
                .as_ref()
                .is_some_and(|v| decode::<Permit>(v).is_ok_and(|old| old.owner != permit.owner))
            {
                return Err(Error::SyncDenied);
            }
            s.cache
                .db
                .execute("UPDATE memory_sync SET last_scope=?1", [scope.key()])?;
            s.cache.db.execute("UPDATE memory_sync_scopes SET permit=?2,epoch=CASE WHEN epoch=0 THEN ?3 ELSE epoch END WHERE scope=?1",params![scope.key(),encode(&permit)?,permit.epoch])?;
            if ack != 0 && ack != permit.epoch {
                return Ok(Round {
                    needs_epoch_choice: true,
                    ..Round::default()
                });
            }
            if !permit.enabled {
                return Ok(Round {
                    more: idle(&s, false)?,
                    ..Round::default()
                });
            }
            let row:Option<(String,String)>=s.cache.db.query_row("SELECT o.operation,o.payload FROM memory_outbox o JOIN memory_sync_uploads u ON u.memory=o.entity JOIN memories m ON m.id=o.entity WHERE m.scope=?1 AND o.blocked=0 AND o.payload IS NOT NULL ORDER BY o.sequence LIMIT 1",[scope.key()],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            let queued = if let Some((operation, text)) = row {
                let mut change: Change = decode(&text)?;
                if let Change::Select {
                    id,
                    base,
                    generation,
                    ..
                } = &mut change
                    && *generation == 0
                {
                    let link: Option<(Option<String>, i64)> = s
                        .cache
                        .db
                        .query_row(
                            "SELECT head,generation FROM memory_sync_links WHERE memory=?1",
                            [id.to_string()],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .optional()?;
                    if let Some((Some(head), g)) = link {
                        *base = Uuid::parse_str(&head).map_err(|_| Error::UnsafeStorage)?;
                        *generation = g;
                        s.cache.db.execute(
                            "UPDATE memory_outbox SET payload=?2 WHERE operation=?1",
                            params![operation, encode(&change)?],
                        )?;
                    }
                }
                if change.valid() {
                    Some((
                        Uuid::parse_str(&operation).map_err(|_| Error::UnsafeStorage)?,
                        change,
                    ))
                } else if matches!(change, Change::Select { generation: 0, .. }) {
                    None
                } else {
                    return Err(Error::Invalid);
                }
            } else {
                None
            };
            let after = s.cache.db.query_row(
                "SELECT cursor FROM memory_sync_scopes WHERE scope=?1",
                [scope.key()],
                |r| r.get(0),
            )?;
            (queued, after)
        };
        let mut round = Round::default();
        let mut conflict = false;
        if let Some((operation, change)) = queued {
            {
                let s = self.chat.state()?;
                if !s.memory.safe(&encode(&change)?) {
                    return Err(Error::Invalid);
                }
            }
            let expected = digest(
                &serde_json::to_vec(&(permit.owner, permit.project, operation, &change))
                    .map_err(|_| Error::Invalid)?,
            );
            let result = session
                .memory_request(
                    "push",
                    value(&Push {
                        protocol: PROTOCOL,
                        operation,
                        version: current.version,
                        permit: permit.clone(),
                        change,
                    })?,
                )
                .await;
            let s = self.chat.state()?;
            check(&s, &session, generation, &namespace)?;
            if !status(&s)?.enabled {
                return Err(Error::Cancelled);
            }
            match result {
                Err(ChatError::Conflict) => {
                    s.cache.db.execute(
                        "UPDATE memory_outbox SET blocked=1 WHERE operation=?1",
                        [operation.to_string()],
                    )?;
                    conflict = true;
                }
                result => {
                    let receipt: Receipt = reply(result.map_err(network)?)?;
                    if receipt.operation != operation
                        || receipt.hash != expected
                        || receipt.sequence < 1
                    {
                        return Err(Error::Invalid);
                    }
                    s.cache.db.execute(
                        "DELETE FROM memory_outbox WHERE operation=?1",
                        [operation.to_string()],
                    )?;
                    round.uploaded = true;
                }
            }
        }
        let page: Page = reply(
            session
                .memory_request(
                    "pull",
                    value(&Pull {
                        protocol: PROTOCOL,
                        version: current.version,
                        permit: permit.clone(),
                        after,
                    })?,
                )
                .await
                .map_err(network)?,
        )?;
        let mut s = self.chat.state()?;
        check(&s, &session, generation, &namespace)?;
        if !status(&s)?.enabled {
            return Err(Error::Cancelled);
        }
        apply(&mut s, &scope, &permit, &page, after)?;
        round.downloaded = page.change.is_some();
        round.more = idle(&s, round.uploaded || round.downloaded)?;
        if conflict {
            return Err(Error::SyncConflict);
        }
        Ok(round)
    }
}
fn idle(s: &State, activity: bool) -> Result<bool> {
    s.cache.db.execute(
        "UPDATE memory_sync SET idle=CASE WHEN ?1 THEN 0 ELSE idle+1 END",
        [activity],
    )?;
    Ok(s.cache.db.query_row(
        "SELECT idle<coalesce(json_array_length(control,'$.scopes'),0) FROM memory_sync",
        [],
        |r| r.get(0),
    )?)
}
fn select_upload(tx: &Transaction<'_>, m: &Memory) -> Result<()> {
    if tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM memory_sync_retained WHERE memory=?1)",
        [m.id.to_string()],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(Error::SyncDenied);
    }
    let selected: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM memory_sync_uploads WHERE memory=?1)",
        [m.id.to_string()],
        |r| r.get(0),
    )?;
    if selected {
        return Ok(());
    }
    tx.execute(
        "INSERT INTO memory_sync_uploads VALUES(?1)",
        [m.id.to_string()],
    )?;
    // Legacy references were never sent by I-4; consent materializes their immutable history.
    let pending: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM memory_outbox WHERE entity=?1 AND payload IS NOT NULL)",
        [m.id.to_string()],
        |r| r.get(0),
    )?;
    if !pending {
        for r in &m.revisions {
            enqueue(
                tx,
                m.id,
                "revision",
                &Change::Upsert {
                    id: m.id,
                    revision: r.id,
                    base: r.parent,
                    created: r.created_millis,
                    draft: r.draft.clone(),
                    recreate: false,
                    candidate: m.state == "candidate",
                },
            )?;
        }
    }
    tx.execute(
        "DELETE FROM memory_outbox WHERE entity=?1 AND payload IS NULL",
        [m.id.to_string()],
    )?;
    Ok(())
}
fn enqueue(tx: &Transaction<'_>, id: Uuid, kind: &str, change: &Change) -> Result<()> {
    tx.execute("UPDATE memory_sync SET idle=0", [])?;
    tx.execute(
        "INSERT INTO memory_outbox(operation,entity,kind,payload) VALUES(?1,?2,?3,?4)",
        params![
            Uuid::new_v4().to_string(),
            id.to_string(),
            kind,
            encode(change)?
        ],
    )?;
    Ok(())
}
pub(crate) fn queue(tx: &Transaction<'_>, id: Uuid, kind: &str) -> Result<()> {
    let (head, state): (String, String) = tx.query_row(
        "SELECT head,state FROM memories WHERE id=?1",
        [id.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let change = if kind == "delete" {
        tx.execute(
            "DELETE FROM memory_outbox WHERE entity=?1",
            [id.to_string()],
        )?;
        Change::Delete {
            id,
            sources: vec![],
        }
    } else if kind == "approve" {
        let link: Option<(Option<String>, i64)> = tx
            .query_row(
                "SELECT head,generation FROM memory_sync_links WHERE memory=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (base, generation) = link.unwrap_or((Some(head.clone()), 0));
        Change::Select {
            id,
            revision: Uuid::parse_str(&head).map_err(|_| Error::UnsafeStorage)?,
            base: Uuid::parse_str(&base.unwrap_or(head)).map_err(|_| Error::UnsafeStorage)?,
            generation,
        }
    } else {
        let (rid,parent,created,text):(String,Option<String>,i64,String)=tx.query_row("SELECT r.id,r.parent,r.created,p.value FROM memory_revisions r JOIN memory_payloads p ON p.revision=r.id WHERE r.memory=?1 ORDER BY r.rowid DESC LIMIT 1",[id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        Change::Upsert {
            id,
            revision: Uuid::parse_str(&rid).map_err(|_| Error::UnsafeStorage)?,
            base: parent
                .map(|p| Uuid::parse_str(&p).map_err(|_| Error::UnsafeStorage))
                .transpose()?,
            created,
            draft: decode(&text)?,
            recreate: kind == "recreate",
            candidate: state == "candidate",
        }
    };
    enqueue(tx, id, kind, &change)
}
fn erase(tx: &Transaction<'_>, id: Uuid, scope: &str, hashes: &[String]) -> Result<()> {
    let mut statement=tx.prepare("SELECT value FROM memory_payloads p JOIN memory_revisions r ON r.id=p.revision WHERE r.memory=?1")?;
    let values = statement
        .query_map([id.to_string()], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(statement);
    for text in values {
        let draft: Draft = decode(&text)?;
        for source in draft.sources {
            tx.execute(
                "INSERT OR IGNORE INTO memory_deleted_sources VALUES(?1,?2)",
                params![scope, source.fingerprint()?],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO memory_sync_deleted_sources VALUES(?1,?2)",
                params![scope, source.sha256],
            )?;
        }
    }
    for hash in hashes {
        tx.execute(
            "INSERT OR IGNORE INTO memory_sync_deleted_sources VALUES(?1,?2)",
            params![scope, hash],
        )?;
    }
    tx.execute("UPDATE memory_jobs SET state='cancelled',reference='' WHERE CASE WHEN json_valid(reference) THEN json_extract(reference,'$.memory')=?1 OR (scope=?2 AND json_extract(reference,'$.source.sha256') IN(SELECT hash FROM memory_sync_deleted_sources WHERE scope=?2)) ELSE 0 END",params![id.to_string(),scope])?;
    tx.execute("DELETE FROM memory_vectors WHERE revision IN(SELECT id FROM memory_revisions WHERE memory=?1)",[id.to_string()])?;
    tx.execute("DELETE FROM memory_payloads WHERE revision IN(SELECT id FROM memory_revisions WHERE memory=?1)",[id.to_string()])?;
    tx.execute(
        "UPDATE memories SET state='deleted' WHERE id=?1",
        [id.to_string()],
    )?;
    tx.execute(
        "DELETE FROM memory_outbox WHERE entity=?1",
        [id.to_string()],
    )?;
    Ok(())
}
fn detach(tx: &Transaction<'_>, owner: &str, scope: Option<&str>, retain: bool) -> Result<()> {
    let mut stmt=tx.prepare("SELECT m.id,m.scope,json_extract(sc.permit,'$.owner') FROM memories m LEFT JOIN memory_sync_links l ON l.memory=m.id LEFT JOIN memory_sync_scopes sc ON sc.scope=m.scope WHERE (?2 IS NULL OR m.scope=?2) AND (l.owner=?1 OR (l.owner IS NULL AND EXISTS(SELECT 1 FROM memory_sync_uploads u WHERE u.memory=m.id) AND ?2 IS NULL))")?;
    let rows = stmt
        .query_map(params![owner, scope], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);
    for (id, scope, known_owner) in rows {
        let preserve = retain || (scope != "personal" && known_owner.as_deref() != Some(owner));
        let id = Uuid::parse_str(&id).map_err(|_| Error::UnsafeStorage)?;
        // Freeze derived jobs even when a local copy is retained.
        tx.execute(
            "UPDATE memory_jobs SET state='cancelled',reference='' WHERE scope=?1",
            [&scope],
        )?;
        if !preserve {
            erase(tx, id, &scope, &[])?;
        } else {
            tx.execute(
                "INSERT OR IGNORE INTO memory_sync_retained VALUES(?1)",
                [id.to_string()],
            )?;
            let mut stmt=tx.prepare("SELECT value FROM memory_payloads p JOIN memory_revisions r ON r.id=p.revision WHERE r.memory=?1")?;
            let values = stmt
                .query_map([id.to_string()], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            drop(stmt);
            for text in values {
                let draft: Draft = decode(&text)?;
                for source in draft.sources {
                    tx.execute(
                        "INSERT OR IGNORE INTO memory_sync_deleted_sources VALUES(?1,?2)",
                        params![scope, source.sha256],
                    )?;
                }
            }
        }
        tx.execute(
            "DELETE FROM memory_outbox WHERE entity=?1",
            [id.to_string()],
        )?;
        tx.execute(
            "DELETE FROM memory_sync_uploads WHERE memory=?1",
            [id.to_string()],
        )?;
        tx.execute(
            "DELETE FROM memory_sync_links WHERE memory=?1",
            [id.to_string()],
        )?;
    }
    tx.execute("UPDATE memory_sync_scopes SET cursor=0,epoch=0 WHERE (?1 IS NULL OR scope=?1) AND json_extract(permit,'$.owner')=?2",params![scope,owner])?;
    Ok(())
}
fn apply(s: &mut State, scope: &Scope, permit: &Permit, page: &Page, after: i64) -> Result<()> {
    if page.after != after || page.next < after || page.change.is_some() != (page.next > after) {
        return Err(Error::Invalid);
    }
    if let Some(change) = &page.change {
        if !change.valid() || !s.memory.safe(&encode(change)?) {
            return Err(Error::Invalid);
        }
        store::authorize(s, scope)?;
    }
    let tx = s.cache.db.transaction()?;
    let actual: i64 = tx.query_row(
        "SELECT cursor FROM memory_sync_scopes WHERE scope=?1",
        [scope.key()],
        |r| r.get(0),
    )?;
    if actual != after {
        return Err(Error::Cancelled);
    }
    if let Some(change) = &page.change {
        let id = change.id();
        let old: Option<(String, String, String)> = tx
            .query_row(
                "SELECT scope,head,state FROM memories WHERE id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if old.as_ref().is_some_and(|o| o.0 != scope.key()) {
            return Err(Error::SyncDenied);
        }
        let old_owner: Option<String> = tx
            .query_row(
                "SELECT owner FROM memory_sync_links WHERE memory=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if old_owner
            .as_ref()
            .is_some_and(|o| o != &permit.owner.to_string())
        {
            return Err(Error::SyncDenied);
        }
        match change {
            Change::Upsert {
                revision,
                base,
                created,
                draft,
                ..
            } => {
                if &draft.scope != scope {
                    return Err(Error::Invalid);
                }
                if !old.as_ref().is_some_and(|o| o.2 == "deleted") {
                    let head = page.head.ok_or(Error::Invalid)?;
                    let cloud_state = page.state.as_deref().ok_or(Error::Invalid)?;
                    if !matches!(cloud_state, "active" | "candidate" | "disputed")
                        || page.generation < 1
                    {
                        return Err(Error::Invalid);
                    }
                    if old.is_none() {
                        let count: i64 = tx.query_row(
                            "SELECT count(*) FROM memories WHERE scope=?1 AND state!='deleted'",
                            [scope.key()],
                            |r| r.get(0),
                        )?;
                        if count >= 4096 {
                            return Err(Error::Invalid);
                        }
                        tx.execute(
                            "INSERT INTO memories VALUES(?1,?2,?3,'disputed')",
                            params![id.to_string(), scope.key(), revision.to_string()],
                        )?;
                    }
                    let existing:Option<(Option<String>,i64,String)>=tx.query_row("SELECT r.parent,r.created,p.value FROM memory_revisions r JOIN memory_payloads p ON p.revision=r.id WHERE r.memory=?1 AND r.id=?2",params![id.to_string(),revision.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
                    let serialized = encode(draft)?;
                    if let Some((parent, time, text)) = existing {
                        if parent != base.map(|id| id.to_string())
                            || time != *created
                            || text != serialized
                        {
                            return Err(Error::SyncConflict);
                        }
                    } else {
                        if let Some(base) = base
                            && !tx.query_row("SELECT EXISTS(SELECT 1 FROM memory_revisions WHERE memory=?1 AND id=?2)",params![id.to_string(),base.to_string()],|r|r.get::<_,bool>(0))?{return Err(Error::SyncConflict);}

                        let count: i64 = tx.query_row(
                            "SELECT count(*) FROM memory_revisions WHERE memory=?1",
                            [id.to_string()],
                            |r| r.get(0),
                        )?;
                        if count >= 1024 {
                            return Err(Error::Invalid);
                        }
                        tx.execute(
                            "INSERT INTO memory_revisions VALUES(?1,?2,?3,?4,?5)",
                            params![
                                revision.to_string(),
                                id.to_string(),
                                base.map(|id| id.to_string()),
                                created,
                                cloud_state == "disputed"
                            ],
                        )?;
                        tx.execute(
                            "INSERT INTO memory_payloads VALUES(?1,?2)",
                            params![revision.to_string(), serialized],
                        )?;
                    }
                    let dirty: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM memory_outbox WHERE entity=?1)",
                        [id.to_string()],
                        |r| r.get(0),
                    )?;
                    let known: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM memory_revisions WHERE memory=?1 AND id=?2)",
                        params![id.to_string(), head.to_string()],
                        |r| r.get(0),
                    )?;
                    if old
                        .as_ref()
                        .is_some_and(|o| o.1 != head.to_string() || o.2 != cloud_state)
                    {
                        tx.execute("DELETE FROM memory_vectors WHERE revision IN(SELECT id FROM memory_revisions WHERE memory=?1)",[id.to_string()])?;
                    }
                    if !dirty && known {
                        tx.execute(
                            "UPDATE memories SET head=?2,state=?3 WHERE id=?1",
                            params![id.to_string(), head.to_string(), cloud_state],
                        )?;
                    } else if !known || old.as_ref().is_some_and(|o| o.1 != head.to_string()) {
                        tx.execute(
                            "UPDATE memories SET state='disputed' WHERE id=?1",
                            [id.to_string()],
                        )?;
                    }
                    tx.execute("INSERT INTO memory_sync_links VALUES(?1,?2,?3,?4,?5) ON CONFLICT(memory) DO UPDATE SET head=excluded.head,generation=excluded.generation,epoch=excluded.epoch",params![id.to_string(),permit.owner.to_string(),permit.epoch,head.to_string(),page.generation])?;
                }
            }
            Change::Delete { sources, .. } => {
                if old.is_none() {
                    tx.execute(
                        "INSERT INTO memories VALUES(?1,?2,?1,'deleted')",
                        params![id.to_string(), scope.key()],
                    )?;
                }
                let retained: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM memory_sync_retained WHERE memory=?1)",
                    [id.to_string()],
                    |r| r.get(0),
                )?;
                if retained {
                    for hash in sources {
                        tx.execute(
                            "INSERT OR IGNORE INTO memory_sync_deleted_sources VALUES(?1,?2)",
                            params![scope.key(), hash],
                        )?;
                    }
                } else {
                    erase(&tx, id, &scope.key(), sources)?;
                }
                tx.execute(
                    "DELETE FROM memory_sync_uploads WHERE memory=?1",
                    [id.to_string()],
                )?;
            }
            Change::Select { .. } => return Err(Error::Invalid),
        }
    }
    tx.execute(
        "UPDATE memory_sync_scopes SET cursor=?2 WHERE scope=?1",
        params![scope.key(), page.next],
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(all(test, feature = "sync-test"))]
#[path = "../memory_http_tests.rs"]
mod http_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn v4_memory_migration_preserves_legacy_and_failed_import_keeps_cursor() {
        let root = std::env::temp_dir().join(format!("caidex-i6-migration-{}", Uuid::new_v4()));
        caidex_credentials::private_directory(&root).unwrap();
        let ns = Namespace::guest();
        let dir = root.join(&ns.0);
        caidex_credentials::private_directory(&dir).unwrap();
        let mut db = rusqlite::Connection::open(dir.join("cache.sqlite3")).unwrap();
        db.execute_batch(include_str!("../schema.sql")).unwrap();
        db.execute_batch(include_str!("schema2.sql")).unwrap();
        db.execute_batch(include_str!("../sync_schema4.sql"))
            .unwrap();
        db.execute("INSERT INTO identity VALUES(?1)", [&ns.0])
            .unwrap();
        db.pragma_update(None, "application_id", 0x43414943)
            .unwrap();
        db.pragma_update(None, "user_version", 4).unwrap();
        let legacy = Uuid::new_v4();
        let operation = Uuid::new_v4();
        db.execute(
            "INSERT INTO memory_outbox(operation,entity,kind) VALUES(?1,?2,'create')",
            params![operation.to_string(), legacy.to_string()],
        )
        .unwrap();
        {
            let tx = db.transaction().unwrap();
            assert!(
                tx.execute_batch(
                    &(include_str!("schema5.sql").to_owned() + "SELECT missing_column;")
                )
                .is_err()
            );
        }
        assert_eq!(
            db.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            4
        );
        assert!(
            db.query_row(
                "SELECT name FROM sqlite_master WHERE name='memory_sync'",
                [],
                |r| r.get::<_, String>(0)
            )
            .optional()
            .unwrap()
            .is_none()
        );
        drop(db);
        let chat = Chat::open(&root, ns).unwrap();
        let mut s = chat.state().unwrap();
        assert_eq!(
            s.cache
                .db
                .pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            5
        );
        let saved: String = s
            .cache
            .db
            .query_row(
                "SELECT operation FROM memory_outbox WHERE entity=?1",
                [legacy.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(saved, operation.to_string());
        s.cache
            .db
            .execute(
                "INSERT INTO memory_sync_scopes(scope) VALUES('personal')",
                [],
            )
            .unwrap();
        let text = "x".repeat(16000);
        let draft = Draft {
            scope: Scope::Personal,
            text: text.clone(),
            kind: Kind::Preference,
            privacy: Privacy::Shareable,
            tags: vec![],
            sources: vec![Source::user(Scope::Personal, text)],
            valid_until_millis: None,
        };
        let mid = Uuid::new_v4();
        let rid = Uuid::new_v4();
        let permit = Permit {
            enabled: true,
            delete_pending: false,
            owner: Uuid::new_v4(),
            project: None,
            version: 2,
            epoch: 1,
            scope_version: 2,
        };
        let page = Page {
            after: 0,
            next: 1,
            change: Some(Change::Upsert {
                id: mid,
                revision: rid,
                base: None,
                created: 1,
                draft,
                recreate: false,
                candidate: false,
            }),
            head: Some(rid),
            state: Some("active".into()),
            generation: 1,
        };
        let pages: i64 = s
            .cache
            .db
            .pragma_query_value(None, "page_count", |r| r.get(0))
            .unwrap();
        s.cache
            .db
            .pragma_update(None, "max_page_count", pages)
            .unwrap();
        assert!(matches!(
            apply(&mut s, &Scope::Personal, &permit, &page, 0),
            Err(Error::Storage)
        ));
        assert_eq!(
            s.cache
                .db
                .query_row("SELECT cursor FROM memory_sync_scopes", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            s.cache
                .db
                .query_row("SELECT count(*) FROM memories", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        s.cache
            .db
            .pragma_update(None, "max_page_count", 65536)
            .unwrap();
        apply(&mut s, &Scope::Personal, &permit, &page, 0).unwrap();
        assert_eq!(
            s.cache
                .db
                .query_row("SELECT cursor FROM memory_sync_scopes", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            s.cache
                .db
                .query_row("SELECT count(*) FROM memory_outbox", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        drop(s);
        drop(chat);
        std::fs::remove_dir_all(root).unwrap();
    }
}
