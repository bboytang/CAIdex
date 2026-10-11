//! Explicit, bounded synchronization. A round sends at most one outbox item and receives one event.
use crate::{Cache, Chat, Content, End, Error, Namespace, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use caidex_auth::{ChatError, ConfirmedSession};
use caidex_cloud_core::{
    Id,
    chat::{Control, Entity, PROTOCOL, Page, Pull, Push, Receipt, SetControl},
};
use rusqlite::{OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

fn id(value: Uuid) -> Id {
    Id::try_from(value.to_string()).expect("UUID")
}
fn uuid(value: &Id) -> Uuid {
    Uuid::parse_str(&String::from(value.clone())).expect("canonical UUID")
}
fn encode<T: serde::Serialize>(value: &T) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(|_| Error::Invalid)
}
fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| Error::Invalid)
}
fn network(error: ChatError) -> Error {
    match error {
        ChatError::Offline => Error::SyncOffline,
        ChatError::Denied => Error::SyncDenied,
        ChatError::Conflict => Error::SyncConflict,
        ChatError::Invalid => Error::Invalid,
    }
}
#[derive(serde::Serialize)]
pub struct Status {
    pub enabled: bool,
    pub version: i64,
    pub cursor: i64,
    pub pending_control: bool,
    pub queued: i64,
}
#[derive(Default, serde::Serialize)]
pub struct Round {
    pub uploaded: bool,
    pub downloaded: bool,
}
impl Chat {
    pub fn connect_sync(&self, session: Arc<ConfirmedSession>) -> Result<()> {
        let mut s = self.state()?;
        if s.cache.namespace() != &Namespace::account(&session) {
            return Err(Error::SyncDenied);
        }
        s.memory.guards.push(session.credential_guard());
        s.sync_session = Some(session);
        Ok(())
    }
    pub fn sync_status(&self) -> Result<Status> {
        self.state()?.cache.sync_status()
    }
    /// Persist the intent before any network work. Enabling includes explicit local conversations.
    /// Offline accounts can stop immediately; guests can never enqueue an account control.
    pub fn set_chat_sync(&self, enabled: bool, conversations: &[Uuid]) -> Result<()> {
        let mut s = self.state()?;
        if s.cache.namespace().owner_user_id().is_none() || conversations.len() > 1000 {
            return Err(Error::SyncDenied);
        }
        let version = s.cache.sync_status()?.version;
        let tx = s.cache.db.transaction()?;
        for c in conversations {
            if !enabled
                || tx
                    .query_row(
                        "SELECT 1 FROM conversations WHERE id=?1",
                        [c.to_string()],
                        |r| r.get::<_, i32>(0),
                    )
                    .optional()?
                    .is_none()
            {
                return Err(Error::Invalid);
            }
            tx.execute(
                "INSERT OR IGNORE INTO chat_uploads VALUES(?1)",
                [c.to_string()],
            )?;
        }
        // Do not replace an unresolved operation: its result may already have committed remotely.
        let pending: Option<String> =
            tx.query_row("SELECT intent FROM chat_sync", [], |r| r.get(0))?;
        if pending.is_some() {
            if enabled {
                return Err(Error::SyncConflict);
            }
            tx.execute("UPDATE chat_sync SET enabled=0,stop_requested=1", [])?;
            tx.commit()?;
            return Ok(());
        }
        let request = SetControl {
            protocol: PROTOCOL,
            operation: id(Uuid::new_v4()),
            base_version: version,
            enabled,
        };
        tx.execute(
            "UPDATE chat_sync SET enabled=0,intent=?1,stop_requested=?2,conflict=0",
            params![
                serde_json::to_string(&request).map_err(|_| Error::Invalid)?,
                !enabled
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// A server-confirmed conflict requires an explicit decision; an unknown timeout keeps the ID.
    pub fn discard_chat_control_conflict(&self) -> Result<()> {
        let s = self.state()?;
        if s.cache.db.execute(
            "UPDATE chat_sync SET intent=NULL,enabled=0,conflict=0 WHERE conflict=1",
            [],
        )? != 1
        {
            return Err(Error::SyncConflict);
        }
        Ok(())
    }
    pub fn delete_conversation(&self, conversation: Uuid) -> Result<()> {
        let mut s = self.state()?;
        if s.cache
            .db
            .query_row(
                "SELECT 1 FROM turns WHERE conversation=?1 AND status='pending'",
                [conversation.to_string()],
                |r| r.get::<_, i32>(0),
            )
            .optional()?
            .is_some()
        {
            return Err(Error::Invalid);
        }
        let tx = s.cache.db.transaction()?;
        if tx
            .query_row(
                "SELECT 1 FROM conversations WHERE id=?1",
                [conversation.to_string()],
                |r| r.get::<_, i32>(0),
            )
            .optional()?
            .is_none()
        {
            return Err(Error::NotFound);
        }
        erase(&tx, conversation)?;
        tx.execute(
            "INSERT INTO outbox(operation,entity,kind) VALUES(?1,?2,'delete')",
            params![Uuid::new_v4().to_string(), conversation.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub async fn sync_once(&self) -> Result<Round> {
        let (session, generation, namespace) = {
            let s = self.state()?;
            (
                s.sync_session.clone().ok_or(Error::SyncOffline)?,
                s.generation,
                s.cache.namespace().clone(),
            )
        };
        let current: Control = decode(
            session
                .chat_request("status", serde_json::json!({"protocol":PROTOCOL}))
                .await
                .map_err(network)?,
        )?;
        if current.version < 1 {
            return Err(Error::Invalid);
        }
        let pending = {
            let s = self.state()?;
            check(&s, &session, generation, &namespace)?;
            let stop: bool =
                s.cache
                    .db
                    .query_row("SELECT stop_requested FROM chat_sync", [], |r| r.get(0))?;
            let mut text: Option<String> =
                s.cache
                    .db
                    .query_row("SELECT intent FROM chat_sync", [], |r| r.get(0))?;
            if text.is_none() && stop {
                let request = SetControl {
                    protocol: PROTOCOL,
                    operation: id(Uuid::new_v4()),
                    base_version: current.version,
                    enabled: false,
                };
                let serialized = serde_json::to_string(&request).map_err(|_| Error::Invalid)?;
                s.cache
                    .db
                    .execute("UPDATE chat_sync SET intent=?1", [&serialized])?;
                text = Some(serialized);
            }
            if let Some(text) = text {
                let mut request: SetControl =
                    serde_json::from_str(&text).map_err(|_| Error::UnsafeStorage)?;
                if request.base_version == 0 {
                    request.base_version = current.version;
                    s.cache.db.execute(
                        "UPDATE chat_sync SET intent=?1",
                        [serde_json::to_string(&request).map_err(|_| Error::Invalid)?],
                    )?;
                }
                Some(request)
            } else {
                s.cache.db.execute(
                    "UPDATE chat_sync SET enabled=?1,version=?2",
                    params![current.enabled, current.version],
                )?;
                None
            }
        };
        if let Some(request) = pending {
            let outcome = session.chat_request("control", encode(&request)?).await;
            if matches!(outcome, Err(ChatError::Conflict)) {
                let s = self.state()?;
                check(&s, &session, generation, &namespace)?;
                s.cache.db.execute("UPDATE chat_sync SET conflict=1", [])?;
            }
            let reply: Control = decode(outcome.map_err(network)?)?;
            if reply.version != request.base_version + 1 || reply.enabled != request.enabled {
                return Err(Error::Invalid);
            }
            let s = self.state()?;
            check(&s, &session, generation, &namespace)?;
            let stored: Option<String> =
                s.cache
                    .db
                    .query_row("SELECT intent FROM chat_sync", [], |r| r.get(0))?;
            if stored.as_deref()
                != Some(&serde_json::to_string(&request).map_err(|_| Error::Invalid)?)
            {
                return Err(Error::Cancelled);
            }
            let stop: bool =
                s.cache
                    .db
                    .query_row("SELECT stop_requested FROM chat_sync", [], |r| r.get(0))?;
            s.cache.db.execute(
                "UPDATE chat_sync SET enabled=?1,version=?2,intent=NULL,stop_requested=?3,conflict=0",
                params![reply.enabled && !stop,reply.version,stop && reply.enabled],
            )?;
            return Ok(Round::default());
        }
        if !current.enabled {
            return Ok(Round::default());
        }
        let upload = {
            let s = self.state()?;
            check(&s, &session, generation, &namespace)?;
            let status = s.cache.sync_status()?;
            if !status.enabled || status.pending_control {
                return Err(Error::Cancelled);
            }
            s.cache.next_upload(current.version, &s.memory)?
        };
        let mut round = Round::default();
        if let Some(request) = upload {
            let reply: Receipt = decode(
                session
                    .chat_request("push", encode(&request)?)
                    .await
                    .map_err(network)?,
            )?;
            let hash = format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(&(&request.operation, &request.entity))
                        .map_err(|_| Error::Invalid)?
                )
            );
            if reply.operation != request.operation || reply.hash != hash || reply.sequence < 1 {
                return Err(Error::Invalid);
            }
            let s = self.state()?;
            check(&s, &session, generation, &namespace)?;
            let status = s.cache.sync_status()?;
            if !status.enabled || status.pending_control {
                return Err(Error::Cancelled);
            }
            s.cache.db.execute(
                "DELETE FROM outbox WHERE operation=?1",
                [String::from(request.operation)],
            )?;
            round.uploaded = true;
        }
        let request = {
            let s = self.state()?;
            check(&s, &session, generation, &namespace)?;
            let status = s.cache.sync_status()?;
            if !status.enabled || status.pending_control {
                return Err(Error::Cancelled);
            }
            Pull {
                protocol: PROTOCOL,
                version: current.version,
                after: status.cursor,
            }
        };
        let page: Page = decode(
            session
                .chat_request("pull", encode(&request)?)
                .await
                .map_err(network)?,
        )?;
        let mut s = self.state()?;
        check(&s, &session, generation, &namespace)?;
        let status = s.cache.sync_status()?;
        if !status.enabled || status.pending_control || status.cursor != request.after {
            return Err(Error::Cancelled);
        }
        let crate::client::State { cache, memory, .. } = &mut *s;
        cache.apply_page(&page, memory)?;
        round.downloaded = page.entity.is_some();
        Ok(round)
    }
}
fn check(
    s: &crate::client::State,
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
impl Cache {
    pub fn sync_status(&self) -> Result<Status> {
        Ok(self.db.query_row("SELECT enabled,version,cursor,(intent IS NOT NULL OR stop_requested=1),(SELECT count(*) FROM outbox) FROM chat_sync",[],|r|Ok(Status{enabled:r.get(0)?,version:r.get(1)?,cursor:r.get(2)?,pending_control:r.get(3)?,queued:r.get(4)?}))?)
    }
    fn next_upload(&self, version: i64, privacy: &crate::memory::Session) -> Result<Option<Push>> {
        let row=self.db.query_row("SELECT operation,entity,kind FROM outbox o WHERE EXISTS(SELECT 1 FROM chat_uploads u WHERE u.conversation=CASE o.kind WHEN 'conversation' THEN o.entity WHEN 'delete' THEN o.entity WHEN 'message' THEN (SELECT conversation FROM messages WHERE id=o.entity) WHEN 'attachment' THEN (SELECT conversation FROM attachments WHERE id=o.entity) END) ORDER BY sequence LIMIT 1",[],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).optional()?;
        let Some((operation, key, kind)) = row else {
            return Ok(None);
        };
        let entity = self.sync_entity(Uuid::parse_str(&key).map_err(|_| Error::Invalid)?, &kind)?;
        if !safe(&entity, privacy) {
            return Err(Error::SyncDenied);
        }
        Ok(Some(Push {
            protocol: PROTOCOL,
            operation: Id::try_from(operation).map_err(|_| Error::Invalid)?,
            version,
            entity,
        }))
    }
    fn sync_entity(&self, key: Uuid, kind: &str) -> Result<Entity> {
        match kind {
            "conversation" => {
                let (project, title) = self.db.query_row(
                    "SELECT project_id,title FROM conversations WHERE id=?1",
                    [key.to_string()],
                    |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?)),
                )?;
                Ok(Entity::Conversation {
                    id: id(key),
                    project_id: project
                        .map(Id::try_from)
                        .transpose()
                        .map_err(|_| Error::Invalid)?,
                    title,
                })
            }
            "message" => {
                let c: String = self.db.query_row(
                    "SELECT conversation FROM messages WHERE id=?1",
                    [key.to_string()],
                    |r| r.get(0),
                )?;
                let m = self.message(Uuid::parse_str(&c).map_err(|_| Error::Invalid)?, key)?;
                Ok(Entity::Message {
                    id: id(key),
                    conversation: id(m.conversation),
                    parent: m.parent.map(id),
                    role: m.role,
                    text: m.content.text,
                    attachments: m.content.attachments.into_iter().map(id).collect(),
                    ending: m.end.as_str().into(),
                    created_at_millis: m.created_at_millis,
                })
            }
            "attachment" => {
                let c: String = self.db.query_row(
                    "SELECT conversation FROM attachments WHERE id=?1",
                    [key.to_string()],
                    |r| r.get(0),
                )?;
                let c = Uuid::parse_str(&c).map_err(|_| Error::Invalid)?;
                let a = self.attachment(c, key)?;
                Ok(Entity::Attachment {
                    id: id(key),
                    conversation: id(c),
                    name: a.name,
                    media_type: a.media_type,
                    data: STANDARD.encode(a.bytes),
                })
            }
            "delete" => Ok(Entity::Delete { id: id(key) }),
            _ => Err(Error::Invalid),
        }
    }
    fn apply_page(&mut self, page: &Page, privacy: &crate::memory::Session) -> Result<()> {
        let current = self.sync_status()?.cursor;
        if page.after != current
            || page.next < current
            || page.entity.is_some() != (page.next > current)
        {
            return Err(Error::Invalid);
        }
        if let Some(entity) = &page.entity
            && (!entity.valid() || !safe(entity, privacy))
        {
            return Err(Error::SyncDenied);
        }
        let tx = self.db.transaction()?;
        if let Some(entity) = &page.entity {
            apply(&tx, entity)?;
        }
        tx.execute("UPDATE chat_sync SET cursor=?1", [page.next])?;
        tx.commit()?;
        Ok(())
    }
}
fn safe(entity: &Entity, privacy: &crate::memory::Session) -> bool {
    if let Entity::Attachment { data, .. } = entity {
        let Ok(bytes) = STANDARD.decode(data) else {
            return false;
        };
        if bytes.is_empty() || bytes.len() > crate::MAX_ATTACHMENT {
            return false;
        }
        let text = String::from_utf8_lossy(&bytes);
        if !privacy.safe(&text) {
            return false;
        }
    }
    serde_json::to_string(entity).is_ok_and(|text| privacy.safe(&text))
}
fn erase(tx: &Transaction<'_>, c: Uuid) -> Result<()> {
    let c = c.to_string();
    tx.execute("INSERT OR IGNORE INTO chat_deleted VALUES(?1)", [&c])?;
    tx.execute("DELETE FROM outbox WHERE entity=?1 OR entity IN(SELECT id FROM messages WHERE conversation=?1) OR entity IN(SELECT id FROM attachments WHERE conversation=?1)",[&c])?;
    tx.execute("DELETE FROM turns WHERE conversation=?1", [&c])?;
    tx.execute(
        "DELETE FROM message_attachments WHERE conversation=?1",
        [&c],
    )?;
    tx.execute("DELETE FROM messages WHERE conversation=?1", [&c])?;
    tx.execute("DELETE FROM attachments WHERE conversation=?1", [&c])?;
    // Keep the consent row until the server acknowledges this deletion, but no local body.
    tx.execute("UPDATE conversations SET title='' WHERE id=?1", [&c])?;
    Ok(())
}
fn apply(tx: &Transaction<'_>, entity: &Entity) -> Result<()> {
    let c = String::from(entity.conversation().clone());
    if matches!(entity, Entity::Delete { .. }) {
        return erase(tx, uuid(entity.conversation()));
    }
    if tx
        .query_row("SELECT 1 FROM chat_deleted WHERE id=?1", [&c], |r| {
            r.get::<_, i32>(0)
        })
        .optional()?
        .is_some()
    {
        return Ok(());
    }
    match entity {
        Entity::Conversation {
            id,
            project_id,
            title,
        } => {
            let key = String::from(id.clone());
            let existing = tx
                .query_row(
                    "SELECT project_id,title FROM conversations WHERE id=?1",
                    [&key],
                    |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()?;
            let project = project_id.clone().map(String::from);
            if let Some(existing) = existing {
                if existing != (project, title.clone()) {
                    return Err(Error::SyncConflict);
                }
            } else {
                tx.execute(
                    "INSERT INTO conversations VALUES(?1,?2,?3)",
                    params![key, project, title],
                )?;
            }
            // A restored conversation's future edits are already within its confirmed cloud range.
            tx.execute("INSERT OR IGNORE INTO chat_uploads VALUES(?1)", [&key])?;
        }
        Entity::Attachment {
            id,
            name,
            media_type,
            data,
            ..
        } => {
            let bytes = STANDARD.decode(data).map_err(|_| Error::Invalid)?;
            let key = String::from(id.clone());
            let existing = tx
                .query_row(
                    "SELECT conversation,name,media_type,bytes FROM attachments WHERE id=?1",
                    [&key],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, Vec<u8>>(3)?,
                        ))
                    },
                )
                .optional()?;
            if let Some(existing) = existing {
                if existing != (c, name.clone(), media_type.clone(), bytes) {
                    return Err(Error::SyncConflict);
                }
            } else {
                tx.execute(
                    "INSERT INTO attachments VALUES(?1,?2,?3,?4,?5)",
                    params![key, c, name, media_type, bytes],
                )?;
            }
        }
        Entity::Message {
            id,
            parent,
            role,
            text,
            attachments,
            ending,
            created_at_millis,
            ..
        } => {
            let content = Content {
                text: text.clone(),
                attachments: attachments.iter().map(uuid).collect(),
            };
            content.validate()?;
            let content = serde_json::to_string(&content).map_err(|_| Error::Invalid)?;
            let key = String::from(id.clone());
            let parent = parent.clone().map(String::from);
            let existing=tx.query_row("SELECT conversation,parent,role,content,ending,created_at_millis FROM messages WHERE id=?1",[&key],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,i64>(5)?))).optional()?;
            if let Some(existing) = existing {
                if existing
                    != (
                        c,
                        parent,
                        role.clone(),
                        content,
                        ending.clone(),
                        *created_at_millis,
                    )
                {
                    return Err(Error::SyncConflict);
                }
            } else {
                let end = End::parse(ending)?;
                tx.execute("INSERT INTO messages(id,conversation,parent,role,content,state,ending,created_at_millis) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![key,c,parent,role,content,end.state(),ending,created_at_millis])?;
                for a in attachments {
                    tx.execute(
                        "INSERT INTO message_attachments VALUES(?1,?2,?3)",
                        params![key, String::from(a.clone()), c],
                    )?;
                }
            }
        }
        Entity::Delete { .. } => unreachable!(),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("caidex-i5-{}", Uuid::new_v4()))
    }
    fn account() -> Namespace {
        Namespace::offline_account(
            &caidex_auth::Trust::local_test("http://localhost:9/").unwrap(),
            &Id::try_from("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_owned()).unwrap(),
        )
    }
    #[test]
    fn consent_deletion_and_cursor_are_atomic_and_memory_independent() {
        let root = test_root();
        let chat = Chat::open(&root, account()).unwrap();
        let c = chat.create_conversation(None, "selected".into()).unwrap();
        let op = Uuid::new_v4();
        let message = {
            let mut s = chat.state().unwrap();
            s.cache
                .begin(c, None, Content::text("offline user"), op)
                .unwrap();
            s.cache.finish(op, End::Interrupted).unwrap()
        };
        chat.set_chat_sync(true, &[c]).unwrap();
        chat.set_chat_sync(false, &[]).unwrap();
        assert!(!chat.sync_status().unwrap().enabled);
        assert!(chat.sync_status().unwrap().pending_control);
        assert!(chat.discard_chat_control_conflict().is_err());
        let privacy = crate::memory::Session::for_namespace(&account());
        let before = chat.sync_status().unwrap().cursor;
        {
            let mut s = chat.state().unwrap();
            let invalid = Page {
                after: before,
                next: before + 1,
                entity: Some(Entity::Message {
                    id: id(Uuid::new_v4()),
                    conversation: id(c),
                    parent: Some(id(Uuid::new_v4())),
                    role: "user".into(),
                    text: "missing parent".into(),
                    attachments: vec![],
                    ending: "complete".into(),
                    created_at_millis: 0,
                }),
            };
            assert!(s.cache.apply_page(&invalid, &privacy).is_err());
            assert_eq!(s.cache.sync_status().unwrap().cursor, before);
        }
        chat.delete_conversation(c).unwrap();
        assert!(chat.conversations(None, 100).unwrap().is_empty());
        assert!(chat.messages(c, 0, 100).is_err());
        assert!(
            chat.attach(c, "a.txt".into(), "text/plain".into(), vec![1])
                .is_err()
        );
        assert_eq!(chat.outbox(0, 100).unwrap().len(), 1);
        let mut s = chat.state().unwrap();
        let stale = Page {
            after: before,
            next: before + 1,
            entity: Some(Entity::Message {
                id: id(message.id),
                conversation: id(c),
                parent: None,
                role: "assistant".into(),
                text: "late deleted body".into(),
                attachments: vec![],
                ending: "interrupted".into(),
                created_at_millis: 0,
            }),
        };
        s.cache.apply_page(&stale, &privacy).unwrap();
        assert_eq!(
            s.cache
                .db
                .query_row("SELECT count(*) FROM messages", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(
            !s.cache
                .db
                .query_row("SELECT automatic FROM memory_settings", [], |r| r
                    .get::<_, bool>(0))
                .unwrap()
        );
        drop(s);
        drop(chat);
        let restored = Chat::open(&root, account()).unwrap();
        assert!(restored.sync_status().unwrap().pending_control);
        assert!(restored.conversations(None, 100).unwrap().is_empty());
        drop(restored);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unselected_queue_does_not_starve_selected_and_known_secrets_are_refused() {
        let root = test_root();
        let chat = Chat::open(&root, account()).unwrap();
        for _ in 0..1001 {
            chat.create_conversation(None, "unselected".into()).unwrap();
        }
        let c = chat.create_conversation(None, "selected".into()).unwrap();
        chat.set_chat_sync(true, &[c]).unwrap();
        let s = chat.state().unwrap();
        assert_eq!(
            s.cache
                .next_upload(2, &s.memory)
                .unwrap()
                .unwrap()
                .entity
                .id(),
            &id(c)
        );
        drop(s);
        let guard = Arc::new(caidex_credentials::Redactor::default());
        guard
            .register(
                &caidex_credentials::Secret::new("synthetic-private-model-value".into()).unwrap(),
            )
            .unwrap();
        chat.memory().protect_credentials(guard).unwrap();
        let a = chat
            .attach(
                c,
                "secret.txt".into(),
                "text/plain".into(),
                b"synthetic-private-model-value".to_vec(),
            )
            .unwrap();
        let s = chat.state().unwrap();
        assert!(!safe(
            &s.cache.sync_entity(a, "attachment").unwrap(),
            &s.memory
        ));
        drop(s);
        drop(chat);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn full_database_download_preserves_cursor_queue_and_existing_content() {
        let root = test_root();
        let mut cache = Cache::open(&root, account()).unwrap();
        let c = cache.create_conversation(None, "existing".into()).unwrap();
        let privacy = crate::memory::Session::for_namespace(&account());
        let pages: i64 = cache
            .db
            .pragma_query_value(None, "page_count", |r| r.get(0))
            .unwrap();
        cache
            .db
            .pragma_update(None, "max_page_count", pages)
            .unwrap();
        let a = Uuid::new_v4();
        let page = Page {
            after: 0,
            next: 1,
            entity: Some(Entity::Attachment {
                id: id(a),
                conversation: id(c),
                name: "bounded.txt".into(),
                media_type: "text/plain".into(),
                data: STANDARD.encode(vec![b'a'; 65536]),
            }),
        };
        assert!(matches!(
            cache.apply_page(&page, &privacy),
            Err(Error::Storage)
        ));
        assert_eq!(cache.sync_status().unwrap().cursor, 0);
        assert_eq!(cache.outbox(0, 100).unwrap().len(), 1);
        assert!(cache.attachment(c, a).is_err());
        assert_eq!(cache.conversations(None, 100).unwrap()[0].title, "existing");
        drop(cache);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn guest_and_wrong_identity_never_gain_upload_and_v3_queue_survives_upgrade() {
        let root = test_root();
        let chat = Chat::open(&root, Namespace::guest()).unwrap();
        assert!(matches!(
            chat.set_chat_sync(true, &[]),
            Err(Error::SyncDenied)
        ));
        drop(chat);
        std::fs::remove_dir_all(root).unwrap();
        let root = test_root();
        let namespace = account();
        caidex_credentials::private_directory(&root).unwrap();
        let dir = root.join(&namespace.0);
        caidex_credentials::private_directory(&dir).unwrap();
        let db = rusqlite::Connection::open(dir.join("cache.sqlite3")).unwrap();
        db.execute_batch(include_str!("schema.sql")).unwrap();
        db.execute_batch(include_str!("memory/schema2.sql"))
            .unwrap();
        db.execute("INSERT INTO identity VALUES(?1)", [&namespace.0])
            .unwrap();
        db.pragma_update(None, "application_id", 0x43414943_i64)
            .unwrap();
        db.pragma_update(None, "user_version", 3).unwrap();
        let c = Uuid::new_v4();
        let operation = Uuid::new_v4();
        db.execute(
            "INSERT INTO conversations VALUES(?1,NULL,'old')",
            [c.to_string()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO outbox(operation,entity,kind) VALUES(?1,?2,'conversation')",
            params![operation.to_string(), c.to_string()],
        )
        .unwrap();
        drop(db);
        let chat = Chat::open(&root, namespace).unwrap();
        assert_eq!(chat.outbox(0, 100).unwrap()[0].operation, operation);
        assert_eq!(chat.conversations(None, 100).unwrap()[0].title, "old");
        assert!(!chat.sync_status().unwrap().enabled);
        drop(chat);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(all(test, feature = "sync-test"))]
#[path = "sync_http_tests.rs"]
pub(crate) mod http_tests;
