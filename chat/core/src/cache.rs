use crate::{Content, End, Error, MAX_ATTACHMENT, MAX_HISTORY, MAX_TEXT, Namespace, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use caidex_cloud_core::Id;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use serde_json::{Value, json};
use std::{fs::File, path::Path, time::Duration};
use uuid::Uuid;

const APPLICATION_ID: i64 = 0x43414943;
const SCHEMA: &str = include_str!("schema.sql");

#[derive(Clone, Serialize)]
pub struct Conversation {
    pub id: Uuid,
    pub project_id: Option<Id>,
    pub title: String,
}
#[derive(Clone, Serialize)]
pub struct Message {
    pub sequence: i64,
    pub created_at_millis: i64,
    pub id: Uuid,
    pub conversation: Uuid,
    pub parent: Option<Uuid>,
    pub role: String,
    pub content: Content,
    pub end: End,
}
#[derive(Serialize)]
pub struct Attachment {
    pub id: Uuid,
    pub name: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}
#[derive(Serialize)]
pub struct OutboxEntry {
    pub sequence: i64,
    pub operation: Uuid,
    pub entity: Uuid,
    pub kind: String,
}

/// One exclusive local partition. The OS owner can read it; this is not E2EE.
pub struct Cache {
    db: Connection,
    namespace: Namespace,
    _lock: File,
}
impl Cache {
    pub fn open(root: &Path, namespace: Namespace) -> Result<Self> {
        if !root.is_absolute() {
            return Err(Error::UnsafeStorage);
        }
        caidex_credentials::private_directory(root).map_err(|_| Error::UnsafeStorage)?;
        let root = root.canonicalize()?;
        if root.ancestors().any(|p| p.join(".git").exists()) {
            return Err(Error::UnsafeStorage);
        }
        let dir = root.join(&namespace.0);
        caidex_credentials::private_directory(&dir).map_err(|_| Error::UnsafeStorage)?;
        for name in [
            "cache.sqlite3",
            "cache.sqlite3-journal",
            "cache.sqlite3-wal",
            "cache.sqlite3-shm",
            "cache.lock",
        ] {
            match std::fs::symlink_metadata(dir.join(name)) {
                Ok(meta) => {
                    if !meta.is_file() || meta.is_symlink() {
                        return Err(Error::UnsafeStorage);
                    }
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        // SAFETY: geteuid has no pointer arguments.
                        if meta.nlink() != 1 || meta.uid() != unsafe { libc::geteuid() } {
                            return Err(Error::UnsafeStorage);
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(_) => return Err(Error::UnsafeStorage),
            }
        }
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("cache.lock"))?;
        lock.try_lock().map_err(|_| Error::UnsafeStorage)?;
        let mut db = Connection::open(dir.join("cache.sqlite3"))?;
        db.busy_timeout(Duration::from_millis(100))?;
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let application: i64 = db.pragma_query_value(None, "application_id", |r| r.get(0))?;
        if version == 0 && application == 0 {
            let tables: i64 =
                db.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
            if tables != 0 {
                return Err(Error::UnsafeStorage);
            }
            db.pragma_update(None, "page_size", 4096)?;
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(SCHEMA)?;
            tx.execute("INSERT INTO identity VALUES (?1)", [&namespace.0])?;
            tx.pragma_update(None, "application_id", APPLICATION_ID)?;
            tx.pragma_update(None, "user_version", 1)?;
            tx.commit()?;
        } else if version != 1 || application != APPLICATION_ID {
            return Err(Error::UnsafeStorage);
        }
        let identity: String = db.query_row("SELECT namespace FROM identity", [], |r| r.get(0))?;
        if identity != namespace.0 {
            return Err(Error::UnsafeStorage);
        }
        let check: String = db.pragma_query_value(None, "quick_check", |r| r.get(0))?;
        if check != "ok" {
            return Err(Error::UnsafeStorage);
        }
        db.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA journal_mode=DELETE;",
        )?;
        db.pragma_update(None, "max_page_count", 65536)?;
        let mut cache = Self {
            db,
            namespace,
            _lock: lock,
        };
        cache.interrupt_pending(End::Interrupted)?;
        Ok(cache)
    }
    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }
    pub fn create_conversation(&mut self, project_id: Option<Id>, title: String) -> Result<Uuid> {
        if title.is_empty() || title.len() > 1024 {
            return Err(Error::Invalid);
        }
        let id = Uuid::new_v4();
        let tx = self.db.transaction()?;
        tx.execute(
            "INSERT INTO conversations VALUES (?1,?2,?3)",
            params![id.to_string(), project_id.map(String::from), title],
        )?;
        queue(&tx, id, "conversation")?;
        tx.commit()?;
        Ok(id)
    }
    pub fn conversations(&self, after: Option<Uuid>, limit: usize) -> Result<Vec<Conversation>> {
        page(limit)?;
        let mut q = self.db.prepare(
            "SELECT id,project_id,title FROM conversations WHERE id>?1 ORDER BY id LIMIT ?2",
        )?;
        let rows = q.query_map(
            params![
                after.map(|v| v.to_string()).unwrap_or_default(),
                limit as i64
            ],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )?;
        rows.map(|r| {
            let (id, project, title) = r?;
            Ok(Conversation {
                id: uuid(&id)?,
                project_id: project
                    .map(Id::try_from)
                    .transpose()
                    .map_err(|_| Error::UnsafeStorage)?,
                title,
            })
        })
        .collect()
    }
    fn conversation(&self, id: Uuid) -> Result<()> {
        if self
            .db
            .query_row(
                "SELECT 1 FROM conversations WHERE id=?1",
                [id.to_string()],
                |r| r.get::<_, i32>(0),
            )
            .optional()?
            .is_none()
        {
            return Err(Error::NotFound);
        }
        Ok(())
    }
    pub fn attach(
        &mut self,
        conversation: Uuid,
        name: String,
        media_type: String,
        bytes: Vec<u8>,
    ) -> Result<Uuid> {
        self.conversation(conversation)?;
        if name.is_empty()
            || name.len() > 256
            || name
                .chars()
                .any(|c| c.is_control() || c == '/' || c == '\\')
            || !matches!(
                media_type.as_str(),
                "image/png" | "image/jpeg" | "application/pdf" | "text/plain"
            )
            || bytes.is_empty()
            || bytes.len() > MAX_ATTACHMENT
        {
            return Err(Error::Invalid);
        }
        let id = Uuid::new_v4();
        let tx = self.db.transaction()?;
        tx.execute(
            "INSERT INTO attachments VALUES (?1,?2,?3,?4,?5)",
            params![
                id.to_string(),
                conversation.to_string(),
                name,
                media_type,
                bytes
            ],
        )?;
        queue(&tx, id, "attachment")?;
        tx.commit()?;
        Ok(id)
    }
    pub fn attachment(&self, conversation: Uuid, id: Uuid) -> Result<Attachment> {
        let (name, media_type, bytes) = self
            .db
            .query_row(
                "SELECT name,media_type,bytes FROM attachments WHERE id=?1 AND conversation=?2",
                params![id.to_string(), conversation.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        Ok(Attachment {
            id,
            name,
            media_type,
            bytes,
        })
    }
    pub fn messages(&self, conversation: Uuid, after: i64, limit: usize) -> Result<Vec<Message>> {
        self.conversation(conversation)?;
        page(limit)?;
        let mut q=self.db.prepare("SELECT sequence,id,conversation,parent,role,content,ending,created_at_millis FROM messages WHERE conversation=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3")?;
        let rows = q.query_map(
            params![conversation.to_string(), after, limit as i64],
            read_message,
        )?;
        rows.map(|r| decode_message(r?)).collect()
    }
    fn message(&self, conversation: Uuid, id: Uuid) -> Result<Message> {
        let row=self.db.query_row("SELECT sequence,id,conversation,parent,role,content,ending,created_at_millis FROM messages WHERE id=?1 AND conversation=?2",params![id.to_string(),conversation.to_string()],read_message).optional()?.ok_or(Error::NotFound)?;
        decode_message(row)
    }
    pub fn outbox(&self, after: i64, limit: usize) -> Result<Vec<OutboxEntry>> {
        page(limit)?;
        let mut q=self.db.prepare("SELECT sequence,operation,entity,kind FROM outbox WHERE sequence>?1 ORDER BY sequence LIMIT ?2")?;
        q.query_map(params![after, limit as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .map(|r| {
            let (sequence, operation, entity, kind) = r?;
            Ok(OutboxEntry {
                sequence,
                operation: uuid(&operation)?,
                entity: uuid(&entity)?,
                kind,
            })
        })
        .collect()
    }
    /// Build only the selected branch. No provider-native history or tool records.
    pub(crate) fn begin(
        &mut self,
        conversation: Uuid,
        parent: Option<Uuid>,
        content: Content,
        operation: Uuid,
    ) -> Result<Value> {
        self.conversation(conversation)?;
        content.validate()?;
        if content.text.is_empty() && content.attachments.is_empty() {
            return Err(Error::Invalid);
        }
        if self
            .db
            .query_row(
                "SELECT 1 FROM turns WHERE operation=?1",
                [operation.to_string()],
                |r| r.get::<_, i32>(0),
            )
            .optional()?
            .is_some()
        {
            return Err(Error::AlreadySubmitted);
        }
        let mut history = vec![];
        let mut cursor = parent;
        let mut bytes = content.text.len();
        while let Some(id) = cursor {
            if history.len() >= MAX_HISTORY - 1 {
                return Err(Error::Invalid);
            }
            let m = self.message(conversation, id)?;
            bytes += m.content.text.len();
            if bytes > MAX_TEXT {
                return Err(Error::Invalid);
            }
            cursor = m.parent;
            history.push(m);
        }
        history.reverse();
        let mut input = vec![];
        let mut attachment_bytes = 0;
        for m in &history {
            input.push(self.input(conversation, &m.role, &m.content, &mut attachment_bytes)?);
        }
        input.push(self.input(conversation, "user", &content, &mut attachment_bytes)?);
        let user = Uuid::new_v4();
        let assistant = Uuid::new_v4();
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_message(
            &tx,
            user,
            conversation,
            parent,
            "user",
            &content,
            End::Complete,
        )?;
        for id in &content.attachments {
            tx.execute(
                "INSERT INTO message_attachments VALUES (?1,?2,?3)",
                params![user.to_string(), id.to_string(), conversation.to_string()],
            )?;
        }
        tx.execute("INSERT INTO turns(operation,conversation,user_message,assistant_message,status,draft) VALUES (?1,?2,?3,?4,'pending','')",params![operation.to_string(),conversation.to_string(),user.to_string(),assistant.to_string()])?;
        tx.commit()?;
        Ok(Value::Array(input))
    }
    fn input(
        &self,
        conversation: Uuid,
        role: &str,
        content: &Content,
        total: &mut usize,
    ) -> Result<Value> {
        let mut parts = vec![
            json!({"type":if role=="user" {"input_text"} else {"output_text"},"text":content.text}),
        ];
        for id in &content.attachments {
            let a = self.attachment(conversation, *id)?;
            *total += a.bytes.len();
            if *total > MAX_ATTACHMENT {
                return Err(Error::Invalid);
            }
            let data = format!("data:{};base64,{}", a.media_type, STANDARD.encode(&a.bytes));
            if a.media_type.starts_with("image/") {
                parts.push(json!({"type":"input_image","image_url":data}));
            } else {
                parts.push(json!({"type":"input_file","filename":a.name,"file_data":data}));
            }
        }
        Ok(json!({"type":"message","role":role,"content":parts}))
    }
    pub(crate) fn checkpoint(&mut self, operation: Uuid, text: &str) -> Result<()> {
        if Content::text(text).validate().is_err() {
            return Err(Error::Invalid);
        }
        if self.db.execute(
            "UPDATE turns SET draft=?2 WHERE operation=?1 AND status='pending'",
            params![operation.to_string(), text],
        )? != 1
        {
            return Err(Error::Cancelled);
        }
        Ok(())
    }
    pub(crate) fn finish(&mut self, operation: Uuid, end: End) -> Result<Message> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (conversation,user,assistant,draft)=tx.query_row("SELECT conversation,user_message,assistant_message,draft FROM turns WHERE operation=?1 AND status='pending'",[operation.to_string()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?))).optional()?.ok_or(Error::Cancelled)?;
        let conversation = uuid(&conversation)?;
        let assistant = uuid(&assistant)?;
        insert_message(
            &tx,
            assistant,
            conversation,
            Some(uuid(&user)?),
            "assistant",
            &Content::text(draft),
            end,
        )?;
        tx.execute(
            "UPDATE turns SET status='finished',draft='' WHERE operation=?1",
            [operation.to_string()],
        )?;
        tx.commit()?;
        self.message(conversation, assistant)
    }
    pub(crate) fn interrupt_pending(&mut self, end: End) -> Result<()> {
        let operations = {
            let mut q = self
                .db
                .prepare("SELECT operation FROM turns WHERE status='pending'")?;
            q.query_map([], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        for operation in operations {
            self.finish(uuid(&operation)?, end)?;
        }
        Ok(())
    }
}
fn page(limit: usize) -> Result<()> {
    if !(1..=1000).contains(&limit) {
        Err(Error::Invalid)
    } else {
        Ok(())
    }
}
fn uuid(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| Error::UnsafeStorage)
}
type StoredMessage = (
    i64,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
    i64,
);
fn read_message(r: &rusqlite::Row<'_>) -> rusqlite::Result<StoredMessage> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
    ))
}
fn decode_message(
    (sequence, id, conversation, parent, role, content, end, created_at_millis): StoredMessage,
) -> Result<Message> {
    Ok(Message {
        sequence,
        created_at_millis,
        id: uuid(&id)?,
        conversation: uuid(&conversation)?,
        parent: parent.map(|p| uuid(&p)).transpose()?,
        role,
        content: serde_json::from_str(&content).map_err(|_| Error::UnsafeStorage)?,
        end: End::parse(&end)?,
    })
}
fn queue(tx: &rusqlite::Transaction<'_>, entity: Uuid, kind: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO outbox(operation,entity,kind) VALUES (?1,?2,?3)",
        params![Uuid::new_v4().to_string(), entity.to_string(), kind],
    )?;
    Ok(())
}
fn insert_message(
    tx: &rusqlite::Transaction<'_>,
    id: Uuid,
    conversation: Uuid,
    parent: Option<Uuid>,
    role: &str,
    content: &Content,
    end: End,
) -> Result<()> {
    content.validate()?;
    let content = serde_json::to_string(content).map_err(|_| Error::Invalid)?;
    let created_at_millis = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::Storage)?
            .as_millis(),
    )
    .map_err(|_| Error::Storage)?;
    tx.execute("INSERT INTO messages(id,conversation,parent,role,content,state,ending,created_at_millis) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",params![id.to_string(),conversation.to_string(),parent.map(|p|p.to_string()),role,content,end.state(),end.as_str(),created_at_millis])?;
    queue(tx, id, "message")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restart_finalizes_persisted_draft_and_full_disk_rolls_back_user_and_outbox() {
        let root = std::env::temp_dir().join(format!("caidex-i3-recovery-{}", Uuid::new_v4()));
        let mut cache = Cache::open(&root, Namespace::guest()).unwrap();
        let c = cache.create_conversation(None, "recovery".into()).unwrap();
        let op = Uuid::new_v4();
        cache
            .begin(c, None, Content::text("unknown POST"), op)
            .unwrap();
        cache.checkpoint(op, "durable partial").unwrap();
        // Simulate process death: Cache has no Drop finalizer; reopen owns recovery.
        drop(cache);
        let mut cache = Cache::open(&root, Namespace::guest()).unwrap();
        let messages = cache.messages(c, 0, 10).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].content.text, "durable partial");
        assert_eq!(messages[1].end, End::Interrupted);
        assert!(matches!(
            cache.begin(c, None, Content::text("retry"), op),
            Err(Error::AlreadySubmitted)
        ));
        let before = cache.outbox(0, 100).unwrap().len();
        let pages: i64 = cache
            .db
            .pragma_query_value(None, "page_count", |r| r.get(0))
            .unwrap();
        cache
            .db
            .pragma_update(None, "max_page_count", pages)
            .unwrap();
        assert!(matches!(
            cache.begin(
                c,
                None,
                Content::text("x".repeat(MAX_TEXT / 2)),
                Uuid::new_v4()
            ),
            Err(Error::Storage)
        ));
        assert_eq!(cache.outbox(0, 100).unwrap().len(), before);
        assert_eq!(cache.messages(c, 0, 10).unwrap().len(), 2);
        drop(cache);
        std::fs::remove_dir_all(root).unwrap();
    }
}
