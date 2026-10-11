//! The actor and owner locks order every page/commit against controls and revocation.
use super::*;
use caidex_cloud_core::memory::{
    Change, Control, Draft, Mode, PROTOCOL, Page, Permit, Pull, Push, Receipt, Scope, SetControl,
};

fn hash<T: serde::Serialize>(v: &T) -> Result<Vec<u8>> {
    Ok(Sha256::digest(serde_json::to_vec(v).map_err(|_| Error::Invalid)?).to_vec())
}
fn hex(v: &[u8]) -> String {
    v.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(s: &str) -> Result<Vec<u8>> {
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(s.get(i..i + 2).ok_or(Error::Invalid)?, 16)
                .map_err(|_| Error::Invalid)
        })
        .collect()
}
fn project(scope: &Scope) -> Option<Uuid> {
    match scope {
        Scope::Personal => None,
        Scope::Project { project_id } => {
            Some(Uuid::parse_str(&String::from(project_id.clone())).expect("canonical UUID"))
        }
    }
}
async fn control(tx: &Transaction<'_>, user: Uuid) -> Result<Control> {
    let r=tx.query_one("SELECT memory_enabled,settings_version,cloud_epoch,scope_version,allowed_scopes,memory_delete_pending FROM caidex.users WHERE user_id=$1",&[&user]).await?;
    Ok(Control {
        enabled: r.get(0),
        version: r.get(1),
        epoch: r.get(2),
        scope_version: r.get(3),
        scopes: json_decode(r.get(4))?,
        delete_pending: r.get(5),
    })
}
async fn bind(tx: &Transaction<'_>, bearer: Option<&str>) -> Result<()> {
    tx.query_one(
        "SELECT caidex.bind_session($1)",
        &[&bearer.ok_or(Error::Denied)?],
    )
    .await?;
    Ok(())
}
async fn permit(tx: &Transaction<'_>, owner: Uuid, p: Option<Uuid>) -> Result<Permit> {
    tx.execute("SELECT caidex.lock_memory_acl($1,$2)", &[&owner, &p])
        .await?;
    let r = tx
        .query_opt("SELECT * FROM caidex.memory_permit($1,$2)", &[&owner, &p])
        .await?
        .ok_or(Error::Denied)?;
    Ok(Permit {
        enabled: r.get(3),
        delete_pending: r.get(4),
        owner,
        project: p,
        version: r.get(0),
        epoch: r.get(1),
        scope_version: r.get(2),
    })
}
impl<S: EmailSender> Service<S> {
    pub(super) async fn memory(
        &self,
        auth: &Transaction<'_>,
        action: &str,
        bearer: Option<&str>,
        mut body: Value,
    ) -> Result<Value> {
        let session = self.actor(auth, bearer).await?;
        let user: Uuid = session.get("user_id");
        let current = control(auth, user).await?;
        if body["protocol"].as_u64() != Some(PROTOCOL as u64) {
            return Err(Error::Invalid);
        }
        if action == "memory-status" {
            fields(&body, &["protocol"])?;
            return json_encode(current);
        }
        if action == "memory-control" {
            let proof = body.as_object_mut().ok_or(Error::Invalid)?.remove("proof");
            let r: SetControl = json_decode(body)?;
            if r.base_version < 1
                || r.scopes.len() > 64
                || r.scopes
                    .iter()
                    .enumerate()
                    .any(|(i, s)| r.scopes[..i].contains(s))
                || (r.mode == Mode::Enable && r.scopes.is_empty())
                || (r.mode != Mode::Enable && !r.scopes.is_empty())
            {
                return Err(Error::Invalid);
            }
            let h = hash(&r)?;
            if let Some(old)=auth.query_opt("SELECT operation_hash,result FROM caidex.memory_controls WHERE user_id=$1 AND operation_id=$2",&[&user,&r.operation]).await? {
    if old.get::<_,Vec<u8>>(0)!=h {return Err(Error::Conflict);}return Ok(old.get(1));
   }
            if current.version != r.base_version || current.delete_pending {
                return Err(Error::Conflict);
            }
            if r.mode == Mode::Delete {
                self.proof(
                    auth,
                    &session,
                    &json!({"proof":proof.ok_or(Error::Denied)?}),
                )
                .await?;
            }
            if r.mode == Mode::Enable {
                let mut data = self.data.lock().await;
                let tx = data.transaction().await?;
                bind(&tx, bearer).await?;
                for s in &r.scopes {
                    if let Some(p) = project(s) {
                        let allowed: bool = tx
                            .query_one("SELECT caidex.project_access($1,false)", &[&p])
                            .await?
                            .get(0);
                        if !allowed {
                            return Err(Error::Denied);
                        }
                    }
                }
                tx.commit().await?;
            }
            let enabled = r.mode == Mode::Enable;
            let deletion = r.mode == Mode::Delete;
            let next = Control {
                enabled,
                version: current.version.checked_add(1).ok_or(Error::Unavailable)?,
                epoch: current
                    .epoch
                    .checked_add(i64::from(deletion))
                    .ok_or(Error::Unavailable)?,
                scope_version: current
                    .scope_version
                    .checked_add(i64::from(enabled || deletion))
                    .ok_or(Error::Unavailable)?,
                scopes: if enabled {
                    r.scopes
                } else if deletion {
                    vec![]
                } else {
                    current.scopes
                },
                delete_pending: deletion,
            };
            let scopes = json_encode(&next.scopes)?;
            let value = json_encode(&next)?;
            auth.execute("UPDATE caidex.users SET memory_enabled=$2,settings_version=$3,cloud_epoch=$4,scope_version=$5,allowed_scopes=$6,memory_delete_pending=$7 WHERE user_id=$1",&[&user,&next.enabled,&next.version,&next.epoch,&next.scope_version,&scopes,&next.delete_pending]).await?;
            auth.execute(
                "INSERT INTO caidex.memory_controls VALUES($1,$2,$3,$4)",
                &[&user, &r.operation, &h, &value],
            )
            .await?;
            return Ok(value);
        }
        if action == "memory-cleanup" {
            fields(&body, &["protocol", "epoch"])?;
            if body["epoch"].as_i64() != Some(current.epoch) || current.enabled {
                return Err(Error::Conflict);
            }
            let mut result = current;
            if result.delete_pending {
                let mut data = self.data.lock().await;
                let tx = data.transaction().await?;
                bind(&tx, bearer).await?;
                let complete: bool = tx
                    .query_one("SELECT caidex.erase_memory(NULL::uuid)", &[])
                    .await?
                    .get(0);
                tx.commit().await?;
                if complete {
                    auth.execute(
                        "UPDATE caidex.users SET memory_delete_pending=false WHERE user_id=$1",
                        &[&user],
                    )
                    .await?;
                }
                result.delete_pending = !complete;
            }
            return json_encode(result);
        }
        if action == "memory-scope" {
            fields(&body, &["protocol", "project"])?;
            let p: Option<Uuid> = json_decode(body["project"].clone())?;
            let mut data = self.data.lock().await;
            let tx = data.transaction().await?;
            bind(&tx, bearer).await?;
            let owner = if let Some(p) = p {
                tx.query_opt(
                    "SELECT owner_user_id FROM caidex.projects WHERE project_id=$1",
                    &[&p],
                )
                .await?
                .ok_or(Error::Denied)?
                .get(0)
            } else {
                user
            };
            if owner != user {
                lock_user(auth, owner).await?;
            }
            let result = permit(&tx, owner, p).await?;
            tx.commit().await?;
            return json_encode(result);
        }
        if !current.enabled || current.delete_pending {
            return Err(Error::Disabled);
        }
        let requested: Permit = json_decode(body["permit"].clone())?;
        if requested.owner != user {
            lock_user(auth, requested.owner).await?;
        }
        let mut data = self.data.lock().await;
        let tx = data.transaction().await?;
        bind(&tx, bearer).await?;
        let actual = permit(&tx, requested.owner, requested.project).await?;
        if !actual.enabled {
            return Err(Error::Disabled);
        }
        if actual != requested || body["version"].as_i64() != Some(current.version) {
            return Err(Error::Conflict);
        }
        let result = match action {
            "memory-push" => {
                let r: Push = json_decode(body)?;
                push(&tx, user, &r, bearer).await.and_then(json_encode)
            }
            "memory-pull" => {
                let r: Pull = json_decode(body)?;
                pull(&tx, user, session.get("session_id"), &r)
                    .await
                    .and_then(json_encode)
            }
            _ => Err(Error::Invalid),
        };
        match result {
            Ok(v) => {
                tx.commit().await?;
                Ok(v)
            }
            Err(e) => {
                tx.rollback().await?;
                Err(e)
            }
        }
    }
}
async fn push(
    tx: &Transaction<'_>,
    actor: Uuid,
    r: &Push,
    bearer: Option<&str>,
) -> Result<Receipt> {
    if r.protocol != PROTOCOL || !r.change.valid() {
        return Err(Error::Invalid);
    }
    let owner = r.permit.owner;
    let p = r.permit.project;
    let mid = r.change.id();
    if !tx
        .query_one("SELECT caidex.scope_access($1,$2,true)", &[&owner, &p])
        .await?
        .get::<_, bool>(0)
    {
        return Err(Error::Denied);
    }
    let h = hash(&(owner, p, r.operation, &r.change))?;
    if let Some(old)=tx.query_opt("SELECT operation_hash,seq,submitted_by,cloud_epoch FROM caidex.sync_events WHERE user_id=$1 AND operation_id=$2 AND channel='memory'",&[&owner,&r.operation]).await? {
  if old.get::<_,Option<Vec<u8>>>(0)!=Some(h.clone())||old.get::<_,Option<Uuid>>(2)!=Some(actor)||old.get::<_,i64>(3)!=r.permit.epoch{return Err(Error::Conflict);}
  return Ok(Receipt{operation:r.operation,hash:hex(&h),sequence:old.get(1)});
 }
    let old=tx.query_opt("SELECT current_revision_id,state,sync_generation,project_id FROM caidex.memories WHERE user_id=$1 AND memory_id=$2 FOR UPDATE",&[&owner,&mid]).await?;
    if old
        .as_ref()
        .is_some_and(|m| m.get::<_, Option<Uuid>>(3) != p)
    {
        return Err(Error::Denied);
    }
    let tomb=tx.query_opt("SELECT 1 FROM caidex.deletion_tombstones WHERE user_id=$1 AND scope_key=coalesce($2::uuid::text,'personal') AND entity_id=$3 AND entity_kind='memory'",&[&owner,&p,&mid]).await?.is_some();
    let generation = old
        .as_ref()
        .map(|m| m.get::<_, i64>(2))
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(Error::Unavailable)?;
    let mut revision_event = None;
    match &r.change {
        Change::Upsert {
            id: _,
            revision,
            base,
            created,
            draft,
            recreate,
            candidate,
        } => {
            if tomb
                || project(&draft.scope) != p
                || old
                    .as_ref()
                    .is_some_and(|m| m.get::<_, String>(1) == "deleted")
            {
                return Err(Error::Conflict);
            }
            let text = serde_json::to_string(draft).map_err(|_| Error::Invalid)?;
            if caidex_credentials::Redactor::default().contains_secret(&text)
                || bearer.is_some_and(|v| text.contains(v))
            {
                return Err(Error::Invalid);
            }
            if let Some(existing)=tx.query_opt("SELECT payload,base_revision_id,created_millis FROM caidex.memory_revisions WHERE memory_id=$1 AND revision_id=$2 AND user_id=$3",&[&mid,&revision,&owner]).await? {
                if existing.get::<_,Option<Value>>(0)!=Some(json_encode(draft)?) || existing.get::<_,Option<Uuid>>(1)!=*base || existing.get::<_,i64>(2)!=*created {return Err(Error::Conflict);}
                let g=old.as_ref().ok_or(Error::Conflict)?.get::<_,i64>(2);
                let seq:i64=tx.query_one("INSERT INTO caidex.sync_events(user_id,project_id,operation_id,entity_id,revision_id,channel,kind,cloud_epoch,operation_hash,memory_generation,submitted_by) VALUES($1,$2,$3,$4,$5,'memory','upsert',$6,$7,$8,$9) RETURNING seq",&[&owner,&p,&r.operation,&mid,&revision,&r.permit.epoch,&h,&g,&actor]).await?.get(0);
                return Ok(Receipt{operation:r.operation,hash:hex(&h),sequence:seq});
            }
            let count: i64 = tx
                .query_one(
                    "SELECT count(*) FROM caidex.memory_revisions WHERE memory_id=$1",
                    &[&mid],
                )
                .await?
                .get(0);
            if count >= 1024 {
                return Err(Error::Invalid);
            }
            if let Some(base) = base
                && tx.query_opt("SELECT 1 FROM caidex.memory_revisions WHERE memory_id=$1 AND revision_id=$2 AND user_id=$3",&[&mid,&base,&owner]).await?.is_none(){return Err(Error::Conflict);}

            if old.is_none() {
                if base.is_some() {
                    return Err(Error::Conflict);
                }
                let count:i64=tx.query_one("SELECT count(*) FROM caidex.memories WHERE user_id=$1 AND scope_key=coalesce($2::uuid::text,'personal')",&[&owner,&p]).await?.get(0);
                if count >= 4096 {
                    return Err(Error::Invalid);
                }
                tx.execute("INSERT INTO caidex.memories(memory_id,user_id,project_id,state,privacy) VALUES($1,$2,$3,$4,'sync_allowed')",&[&mid,&owner,&p,&if *candidate {"candidate"}else {"active"}]).await?;
            }
            for source in &draft.sources {
                let sha = unhex(&source.sha256)?;
                if !recreate&&tx.query_opt("SELECT 1 FROM caidex.deletion_tombstones WHERE user_id=$1 AND scope_key=coalesce($2::uuid::text,'personal') AND entity_kind='source' AND source_hash=$3",&[&owner,&p,&sha]).await?.is_some(){return Err(Error::Conflict);}
            }
            let payload = json_encode(draft)?;
            let kind = json_encode(draft.kind)?
                .as_str()
                .ok_or(Error::Invalid)?
                .to_owned();
            tx.execute("INSERT INTO caidex.memory_revisions(revision_id,memory_id,user_id,project_id,base_revision_id,author_user_id,content,tags,kind,payload,created_millis,valid_from,valid_until) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'-infinity',to_timestamp($12::bigint::double precision/1000))",&[&revision,&mid,&owner,&p,&base,&actor,&draft.text,&draft.tags,&kind,&payload,&created,&draft.valid_until_millis]).await?;
            for source in &draft.sources {
                let source_kind = json_encode(source.origin.source_type())?
                    .as_str()
                    .ok_or(Error::Invalid)?
                    .to_owned();
                let source_ref =
                    serde_json::to_string(&source.origin).map_err(|_| Error::Invalid)?;
                let sha = unhex(&source.sha256)?;
                let message = if let caidex_cloud_core::memory::Origin::Chat {
                    conversation,
                    message,
                } = &source.origin
                {
                    tx.query_opt("SELECT message_id FROM caidex.messages WHERE user_id=$1 AND scope_key=coalesce($2::uuid::text,'personal') AND conversation_id=$3 AND message_id=$4",&[&owner,&p,&conversation,&message]).await?.map(|m|m.get::<_,Uuid>(0))
                } else {
                    None
                };
                tx.execute("INSERT INTO caidex.memory_sources(user_id,project_id,revision_id,message_id,source_kind,source_ref,content_hash,excerpt) VALUES($1,$2,$3,$4,$5,$6,$7,$8)",&[&owner,&p,&revision,&message,&source_kind,&source_ref,&sha,&source.excerpt]).await?;
            }
            let conflict = old.as_ref().is_some_and(|m| {
                m.get::<_, Option<Uuid>>(0) != *base || m.get::<_, String>(1) == "disputed"
            });
            if conflict {
                tx.execute("UPDATE caidex.memories SET state='disputed',sync_generation=$2 WHERE memory_id=$1",&[&mid,&generation]).await?;
            } else {
                tx.execute("UPDATE caidex.memories SET current_revision_id=$2,sync_generation=$3 WHERE memory_id=$1",&[&mid,&revision,&generation]).await?;
            }
            revision_event = Some(*revision);
        }
        Change::Select {
            revision,
            base,
            generation: expected,
            ..
        } => {
            let old = old.ok_or(Error::Conflict)?;
            if tomb
                || old.get::<_, Option<Uuid>>(0) != Some(*base)
                || old.get::<_, i64>(2) != *expected
            {
                return Err(Error::Conflict);
            }
            if tx.query_opt("SELECT 1 FROM caidex.memory_revisions WHERE memory_id=$1 AND revision_id=$2 AND user_id=$3",&[&mid,&revision,&owner]).await?.is_none(){return Err(Error::Conflict);}
            tx.execute("UPDATE caidex.memories SET current_revision_id=$2,state='active',sync_generation=$3 WHERE memory_id=$1",&[&mid,&revision,&generation]).await?;
            revision_event = Some(*revision);
        }
        Change::Delete { .. } => {
            if let Some(old) = old {
                if old.get::<_, String>(1) != "deleted" {
                    tx.execute("UPDATE caidex.memories SET state='deleted',sync_generation=$2 WHERE memory_id=$1",&[&mid,&generation]).await?;
                }
            } else {
                tx.execute("INSERT INTO caidex.memories(memory_id,user_id,project_id,state) VALUES($1,$2,$3,'deleted')",&[&mid,&owner,&p]).await?;
            }
        }
    }
    let seq:i64=tx.query_one("INSERT INTO caidex.sync_events(user_id,project_id,operation_id,entity_id,revision_id,channel,kind,cloud_epoch,operation_hash,memory_generation,submitted_by) VALUES($1,$2,$3,$4,$5,'memory',$6,$7,$8,$9,$10) RETURNING seq",&[&owner,&p,&r.operation,&mid,&revision_event,&if matches!(r.change,Change::Delete{..}){"delete"}else{"upsert"},&r.permit.epoch,&h,&generation,&actor]).await?.get(0);
    if matches!(r.change, Change::Delete { .. }) {
        tx.execute("SELECT caidex.erase_memory($1)", &[&mid])
            .await?;
    }
    Ok(Receipt {
        operation: r.operation,
        hash: hex(&h),
        sequence: seq,
    })
}
async fn pull(tx: &Transaction<'_>, actor: Uuid, session: Uuid, r: &Pull) -> Result<Page> {
    if r.protocol != PROTOCOL || r.after < 0 {
        return Err(Error::Invalid);
    }
    let owner = r.permit.owner;
    let p = r.permit.project;
    let latest:i64=tx.query_one("SELECT coalesce(max(seq),0) FROM caidex.sync_events WHERE user_id=$1 AND scope_key=coalesce($2::uuid::text,'personal') AND channel='memory' AND memory_generation IS NOT NULL",&[&owner,&p]).await?.get(0);
    if r.after > latest {
        return Err(Error::Conflict);
    }
    tx.execute("INSERT INTO caidex.sync_cursors(user_id,project_id,session_user_id,session_id,channel,cloud_epoch,acknowledged_seq,schema_version,protocol_version) VALUES($1,$2,$3,$4,'memory',$5,$6,4,1) ON CONFLICT(session_id,scope_key,channel) DO UPDATE SET acknowledged_seq=EXCLUDED.acknowledged_seq,cloud_epoch=EXCLUDED.cloud_epoch,schema_version=4,protocol_version=1",&[&owner,&p,&actor,&session,&r.permit.epoch,&r.after]).await?;
    let event=tx.query_opt("SELECT seq,entity_id,revision_id,kind FROM caidex.sync_events WHERE user_id=$1 AND scope_key=coalesce($2::uuid::text,'personal') AND channel='memory' AND memory_generation IS NOT NULL AND seq>$3 ORDER BY seq LIMIT 1",&[&owner,&p,&r.after]).await?;
    let Some(event) = event else {
        return Ok(Page {
            after: r.after,
            next: r.after,
            change: None,
            head: None,
            state: None,
            generation: 0,
        });
    };
    let mid: Uuid = event.get(1);
    let memory=tx.query_opt("SELECT current_revision_id,state,sync_generation FROM caidex.memories WHERE user_id=$1 AND memory_id=$2",&[&owner,&mid]).await?;
    let (change, head, state, generation) = if let Some(m) = memory {
        let revision: Option<Uuid> = event.get(2);
        let revision = revision.ok_or(Error::Unavailable)?;
        let rr=tx.query_opt("SELECT payload,base_revision_id,created_millis FROM caidex.memory_revisions WHERE user_id=$1 AND memory_id=$2 AND revision_id=$3",&[&owner,&mid,&revision]).await?.ok_or(Error::Unavailable)?;
        let draft: Draft = json_decode(rr.get::<_, Option<Value>>(0).ok_or(Error::Unavailable)?)?;
        let state: String = m.get(1);
        (
            Change::Upsert {
                id: mid,
                revision,
                base: rr.get(1),
                created: rr.get(2),
                draft,
                recreate: false,
                candidate: state == "candidate",
            },
            m.get(0),
            Some(state),
            m.get(2),
        )
    } else {
        let rows=tx.query("SELECT DISTINCT encode(source_hash,'hex') FROM caidex.deletion_tombstones WHERE user_id=$1 AND scope_key=coalesce($2::uuid::text,'personal') AND entity_kind='source' AND memory_id=$3 AND source_hash IS NOT NULL LIMIT 8193",&[&owner,&p,&mid]).await?;
        if rows.len() > 8192 {
            return Err(Error::Unavailable);
        }
        (
            Change::Delete {
                id: mid,
                sources: rows.into_iter().map(|r| r.get(0)).collect(),
            },
            None,
            Some("deleted".into()),
            0,
        )
    };
    Ok(Page {
        after: r.after,
        next: event.get(0),
        change: Some(change),
        head,
        state,
        generation,
    })
}
