//! Data transactions use the ordinary RLS role while the authenticated user lock
//! orders uploads/downloads against control changes and session revocation.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use caidex_cloud_core::chat::{Control, Entity, PROTOCOL, Page, Pull, Push, Receipt, SetControl};

fn id(value: &caidex_cloud_core::Id) -> Uuid {
    Uuid::parse_str(&String::from(value.clone())).expect("validated canonical UUID")
}
fn wire(value: Uuid) -> caidex_cloud_core::Id {
    caidex_cloud_core::Id::try_from(value.to_string()).expect("UUID")
}
fn fingerprint<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(Sha256::digest(serde_json::to_vec(value).map_err(|_| Error::Invalid)?).to_vec())
}
async fn control(tx: &Transaction<'_>, user: Uuid) -> Result<Control> {
    let row = tx
        .query_one(
            "SELECT chat_sync_enabled,chat_settings_version FROM caidex.users WHERE user_id=$1",
            &[&user],
        )
        .await?;
    Ok(Control {
        enabled: row.get(0),
        version: row.get(1),
    })
}
impl<S: EmailSender> Service<S> {
    pub(super) async fn chat(
        &self,
        auth: &Transaction<'_>,
        action: &str,
        bearer: Option<&str>,
        body: Value,
    ) -> Result<Value> {
        let session = self.actor(auth, bearer).await?;
        let user: Uuid = session.get("user_id");
        let current = control(auth, user).await?;
        if action == "chat-status" {
            fields(&body, &["protocol"])?;
            if body["protocol"].as_u64() != Some(PROTOCOL as u64) {
                return Err(Error::Invalid);
            }
            return json_encode(current);
        }
        if action == "chat-control" {
            let request: SetControl = json_decode(body)?;
            if request.protocol != PROTOCOL || request.base_version < 1 {
                return Err(Error::Invalid);
            }
            let hash = fingerprint(&request)?;
            let operation = id(&request.operation);
            if let Some(row)=auth.query_opt("SELECT operation_hash,enabled,version FROM caidex.chat_controls WHERE user_id=$1 AND operation_id=$2",&[&user,&operation]).await? {
                if row.get::<_,Vec<u8>>(0)!=hash {return Err(Error::Conflict);}
                return json_encode(Control{enabled:row.get(1),version:row.get(2)});
            }
            if current.version != request.base_version {
                return Err(Error::Conflict);
            }
            let next = Control {
                enabled: request.enabled,
                version: current.version.checked_add(1).ok_or(Error::Unavailable)?,
            };
            auth.execute("UPDATE caidex.users SET chat_sync_enabled=$2,chat_settings_version=$3 WHERE user_id=$1",&[&user,&next.enabled,&next.version]).await?;
            auth.execute(
                "INSERT INTO caidex.chat_controls VALUES($1,$2,$3,$4,$5)",
                &[&user, &operation, &hash, &next.enabled, &next.version],
            )
            .await?;
            return json_encode(next);
        }
        if !current.enabled {
            return Err(Error::Disabled);
        }
        let mut data = self.data.lock().await;
        let tx = data.transaction().await?;
        tx.query_one(
            "SELECT caidex.bind_session($1)",
            &[&bearer.ok_or(Error::Denied)?],
        )
        .await?;
        let result = match action {
            "chat-push" => {
                let request: Push = json_decode(body)?;
                if request.protocol != PROTOCOL || !request.entity.valid() {
                    return Err(Error::Invalid);
                }
                let serialized =
                    serde_json::to_string(&request.entity).map_err(|_| Error::Invalid)?;
                if caidex_credentials::Redactor::default().contains_secret(&serialized)
                    || bearer.is_some_and(|token| serialized.contains(token))
                {
                    return Err(Error::Invalid);
                }
                if let Entity::Attachment { data, .. } = &request.entity {
                    let bytes = STANDARD.decode(data).map_err(|_| Error::Invalid)?;
                    let text = String::from_utf8_lossy(&bytes);
                    if caidex_credentials::Redactor::default().contains_secret(&text)
                        || bearer.is_some_and(|token| text.contains(token))
                    {
                        return Err(Error::Invalid);
                    }
                }
                if request.version != current.version {
                    return Err(Error::Conflict);
                }
                push(&tx, user, &request).await.and_then(json_encode)
            }
            "chat-pull" => {
                let request: Pull = json_decode(body)?;
                if request.protocol != PROTOCOL || request.after < 0 {
                    return Err(Error::Invalid);
                }
                if request.version != current.version {
                    return Err(Error::Conflict);
                }
                pull(&tx, user, session.get("session_id"), &request)
                    .await
                    .and_then(json_encode)
            }
            _ => Err(Error::Invalid),
        };
        match result {
            Ok(value) => {
                tx.commit().await?;
                Ok(value)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }
}
async fn conversation(
    tx: &Transaction<'_>,
    user: Uuid,
    c: Uuid,
    write: bool,
) -> Result<(Option<Uuid>, bool)> {
    let row=tx.query_opt("SELECT project_id,state,caidex.scope_access(user_id,project_id,$3) AS permitted FROM caidex.conversations WHERE user_id=$1 AND conversation_id=$2",&[&user,&c,&write]).await?.ok_or(Error::Denied)?;
    if !row.get::<_, bool>("permitted") {
        return Err(Error::Denied);
    }
    Ok((
        row.get("project_id"),
        row.get::<_, String>("state") == "deleted",
    ))
}
async fn push(tx: &Transaction<'_>, user: Uuid, request: &Push) -> Result<Receipt> {
    let entity = &request.entity;
    let operation = id(&request.operation);
    let entity_id = id(entity.id());
    let c = id(entity.conversation());
    let hash = fingerprint(&(&request.operation, &request.entity))?;
    if let Some(row)=tx.query_opt("SELECT operation_hash,seq,conversation_id FROM caidex.sync_events WHERE user_id=$1 AND operation_id=$2 AND channel='chat'",&[&user,&operation]).await? {
        if row.get::<_,Option<Vec<u8>>>(0)!=Some(hash.clone()) {return Err(Error::Conflict);}
        // A completed operation is never applied again; permissions are nevertheless fresh.
        conversation(tx,user,row.get::<_,Option<Uuid>>(2).ok_or(Error::Invalid)?,true).await?;
        return Ok(Receipt{operation:request.operation.clone(),hash:hex(&hash),sequence:row.get(1)});
    }
    let project = if let Entity::Conversation {
        project_id, title, ..
    } = entity
    {
        let project = project_id.as_ref().map(id);
        let allowed: bool = tx
            .query_one("SELECT caidex.scope_access($1,$2,true)", &[&user, &project])
            .await?
            .get(0);
        if !allowed {
            return Err(Error::Denied);
        }
        if let Some(row)=tx.query_opt("SELECT state,title,project_id FROM caidex.conversations WHERE conversation_id=$1 AND user_id=$2",&[&c,&user]).await? {
            if row.get::<_,String>(0)!="active" {return Err(Error::Conflict);}
            if row.get::<_,String>(1)!=*title || row.get::<_,Option<Uuid>>(2)!=project {return Err(Error::Conflict);}
        } else {
            if tx.query_opt("SELECT 1 FROM caidex.deletion_tombstones WHERE user_id=$1 AND entity_id=$2 AND entity_kind='conversation'",&[&user,&c]).await?.is_some() {return Err(Error::Conflict);}
            tx.execute("INSERT INTO caidex.conversations(conversation_id,user_id,project_id,title) VALUES($1,$2,$3,$4)",&[&c,&user,&project,&title]).await?;
        }
        project
    } else {
        if matches!(entity, Entity::Delete { .. })
            && tx
                .query_opt(
                    "SELECT 1 FROM caidex.conversations WHERE user_id=$1 AND conversation_id=$2",
                    &[&user, &c],
                )
                .await?
                .is_none()
        {
            tx.execute("INSERT INTO caidex.conversations(conversation_id,user_id,state,title) VALUES($1,$2,'deleted','')",&[&c,&user]).await?;
        }
        let (project, deleted) = conversation(tx, user, c, true).await?;
        if deleted && !matches!(entity, Entity::Delete { .. }) {
            return Err(Error::Conflict);
        }
        project
    };
    match entity {
        Entity::Attachment {
            name,
            media_type,
            data,
            ..
        } => {
            let bytes = STANDARD.decode(data).map_err(|_| Error::Invalid)?;
            if bytes.is_empty() || bytes.len() > 8388608 || STANDARD.encode(&bytes) != *data {
                return Err(Error::Invalid);
            }
            if let Some(existing) = attachment(tx, user, entity_id, c).await? {
                if existing != *entity {
                    return Err(Error::Conflict);
                }
            } else {
                let hash = Sha256::digest(&bytes).to_vec();
                tx.execute("INSERT INTO caidex.attachments(attachment_id,user_id,project_id,conversation_id,object_ref,content_hash,size_bytes,state,name,media_type,bytes) VALUES($1,$2,$3,$4,'inline',$5,$6,'ready',$7,$8,$9)",&[&entity_id,&user,&project,&c,&hash,&(bytes.len() as i64),&name,&media_type,&bytes]).await?;
            }
        }
        Entity::Message {
            parent,
            role,
            text,
            attachments,
            ending,
            created_at_millis,
            ..
        } => {
            if let Some(existing) = message(tx, user, entity_id, c).await? {
                if existing != *entity {
                    return Err(Error::Conflict);
                }
            } else {
                if let Some(parent) = parent {
                    let parent = id(parent);
                    if tx.query_opt("SELECT 1 FROM caidex.messages WHERE user_id=$1 AND conversation_id=$2 AND message_id=$3",&[&user,&c,&parent]).await?.is_none() {return Err(Error::Invalid);}
                }
                let mut ids = Vec::new();
                for a in attachments {
                    let a = id(a);
                    if attachment(tx, user, a, c).await?.is_none() {
                        return Err(Error::Invalid);
                    }
                    ids.push(a);
                }
                if serde_json::to_vec(&json!({"text":text,"attachments":attachments}))
                    .map_err(|_| Error::Invalid)?
                    .len()
                    > 1048576
                {
                    return Err(Error::Invalid);
                }
                let state = if ending == "complete" {
                    "complete"
                } else {
                    "interrupted"
                };
                tx.execute("INSERT INTO caidex.messages(message_id,user_id,project_id,conversation_id,base_message_id,author_user_id,role,body,state,ending,created_at_millis,attachment_ids) VALUES($1,$2,$3,$4,$5,$2,$6,$7,$8,$9,$10,$11)",&[&entity_id,&user,&project,&c,&parent.as_ref().map(id),&role,&text,&state,&ending,&created_at_millis,&ids]).await?;
            }
        }
        Entity::Delete { .. } => {
            tx.execute(
                "DELETE FROM caidex.attachments WHERE user_id=$1 AND conversation_id=$2",
                &[&user, &c],
            )
            .await?;
            tx.execute("UPDATE caidex.conversations SET state='deleted',title='',version=version+1 WHERE user_id=$1 AND conversation_id=$2 AND state='active'",&[&user,&c]).await?;
            tx.execute("SELECT caidex.erase_chat_body($1)", &[&c])
                .await?;
        }
        Entity::Conversation { .. } => (),
    }
    let seq:i64=tx.query_one("INSERT INTO caidex.sync_events(user_id,project_id,operation_id,entity_id,channel,kind,cloud_epoch,entity_kind,operation_hash,conversation_id) VALUES($1,$2,$3,$4,'chat',$5,1,$6,$7,$8) RETURNING seq",&[&user,&project,&operation,&entity_id,&if matches!(entity,Entity::Delete{..}) {"delete"} else {"upsert"},&entity.kind(),&hash,&c]).await?.get(0);
    if matches!(entity, Entity::Delete { .. }) {
        tx.execute("INSERT INTO caidex.deletion_tombstones(user_id,project_id,entity_id,entity_kind,seq,cloud_epoch,retain_until) VALUES($1,$2,$3,'conversation',$4,1,'infinity') ON CONFLICT(user_id,scope_key,entity_kind,entity_id) DO NOTHING",&[&user,&project,&c,&seq]).await?;
    }
    Ok(Receipt {
        operation: request.operation.clone(),
        hash: hex(&hash),
        sequence: seq,
    })
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
async fn attachment(tx: &Transaction<'_>, user: Uuid, a: Uuid, c: Uuid) -> Result<Option<Entity>> {
    tx.query_opt("SELECT name,media_type,bytes,content_hash,size_bytes FROM caidex.attachments WHERE user_id=$1 AND attachment_id=$2 AND conversation_id=$3 AND state='ready'",&[&user,&a,&c]).await?.map(|r| {
        let bytes:Vec<u8>=r.get(2);
        if Sha256::digest(&bytes).as_slice()!=r.get::<_,Vec<u8>>(3) || bytes.len() as i64!=r.get::<_,i64>(4) {return Err(Error::Unavailable);}
        Ok(Entity::Attachment{id:wire(a),conversation:wire(c),name:r.get(0),media_type:r.get(1),data:STANDARD.encode(bytes)})
    }).transpose()
}
async fn message(tx: &Transaction<'_>, user: Uuid, m: Uuid, c: Uuid) -> Result<Option<Entity>> {
    Ok(tx.query_opt("SELECT base_message_id,role,body,attachment_ids,ending,created_at_millis FROM caidex.messages WHERE user_id=$1 AND message_id=$2 AND conversation_id=$3",&[&user,&m,&c]).await?.map(|r|Entity::Message{id:wire(m),conversation:wire(c),parent:r.get::<_,Option<Uuid>>(0).map(wire),role:r.get(1),text:r.get(2),attachments:r.get::<_,Vec<Uuid>>(3).into_iter().map(wire).collect(),ending:r.get(4),created_at_millis:r.get(5)}))
}
async fn pull(tx: &Transaction<'_>, user: Uuid, session: Uuid, request: &Pull) -> Result<Page> {
    let latest:i64=tx.query_one("SELECT coalesce(max(seq),0) FROM caidex.sync_events WHERE user_id=$1 AND channel='chat' AND entity_kind IS NOT NULL",&[&user]).await?.get(0);
    if request.after > latest {
        return Err(Error::Invalid);
    }
    tx.execute("INSERT INTO caidex.sync_cursors(user_id,session_user_id,session_id,channel,cloud_epoch,acknowledged_seq,schema_version,protocol_version) VALUES($1,$1,$2,'chat',1,$3,3,1) ON CONFLICT(session_id,scope_key,channel) DO UPDATE SET acknowledged_seq=EXCLUDED.acknowledged_seq,schema_version=3,protocol_version=1",&[&user,&session,&request.after]).await?;
    let row=tx.query_opt("SELECT seq,entity_id,entity_kind,conversation_id FROM caidex.sync_events WHERE user_id=$1 AND channel='chat' AND entity_kind IS NOT NULL AND seq>$2 ORDER BY seq LIMIT 1",&[&user,&request.after]).await?;
    let Some(row) = row else {
        return Ok(Page {
            after: request.after,
            next: request.after,
            entity: None,
        });
    };
    let c: Uuid = row.get::<_, Option<Uuid>>(3).ok_or(Error::Unavailable)?;
    let (_, deleted) = conversation(tx, user, c, false).await?;
    let entity_id: Uuid = row.get(1);
    let entity = if deleted {
        Entity::Delete { id: wire(c) }
    } else {
        match row.get::<_, Option<String>>(2).as_deref() {
            Some("conversation") => {
                let r=tx.query_one("SELECT title,project_id FROM caidex.conversations WHERE user_id=$1 AND conversation_id=$2",&[&user,&c]).await?;
                Entity::Conversation {
                    id: wire(c),
                    title: r.get(0),
                    project_id: r.get::<_, Option<Uuid>>(1).map(wire),
                }
            }
            Some("attachment") => attachment(tx, user, entity_id, c)
                .await?
                .ok_or(Error::Unavailable)?,
            Some("message") => message(tx, user, entity_id, c)
                .await?
                .ok_or(Error::Unavailable)?,
            _ => return Err(Error::Unavailable),
        }
    };
    Ok(Page {
        after: request.after,
        next: row.get(0),
        entity: Some(entity),
    })
}
