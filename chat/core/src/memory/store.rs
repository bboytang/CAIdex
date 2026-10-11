use super::*;
use crate::client::State;
use rusqlite::{OptionalExtension, params};

pub(crate) fn authorize(s: &State, scope: &Scope) -> Result<()> {
    if let Scope::Project { project_id } = scope
        && s.cache
            .db
            .query_row(
                "SELECT 1 FROM memory_projects WHERE project=?1",
                [String::from(project_id.clone())],
                |r| r.get::<_, i32>(0),
            )
            .optional()?
            .is_none()
    {
        return Err(Error::NotFound);
    }
    Ok(())
}
pub(crate) fn automatic(s: &State) -> Result<bool> {
    Ok(s.cache.db.query_row(
        "SELECT automatic FROM memory_settings WHERE id=1",
        [],
        |r| r.get::<_, bool>(0),
    )?)
}
pub(crate) fn source_bytes(s: &State, source: &Source) -> Result<Vec<u8>> {
    authorize(s, &source.scope)?;
    let bytes = match &source.origin {
        Origin::User => source.excerpt.as_bytes().to_vec(),
        Origin::Chat {
            conversation,
            message,
        } => {
            let project: Option<String> = s
                .cache
                .db
                .query_row(
                    "SELECT project_id FROM conversations WHERE id=?1",
                    [conversation.to_string()],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or(Error::NotFound)?;
            let expected = project
                .map(|p| Id::try_from(p).map(|project_id| Scope::Project { project_id }))
                .transpose()
                .map_err(|_| Error::UnsafeStorage)?
                .unwrap_or(Scope::Personal);
            if source.scope != expected {
                return Err(Error::NotFound);
            }
            let message = s.cache.message(*conversation, *message)?;
            if message.end != crate::End::Complete {
                return Err(Error::NotFound);
            }
            message.content.text.into_bytes()
        }
        _ => s
            .memory
            .resolver
            .as_ref()
            .and_then(|r| r.read(&source.scope, &source.origin))
            .ok_or(Error::NotFound)?,
    };
    if bytes.len() > 1024 * 1024 || source.sha256 != digest(&bytes) {
        return Err(Error::NotFound);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| Error::Invalid)?;
    if !text.contains(&source.excerpt) || !s.memory.safe(text) {
        return Err(Error::Invalid);
    }
    Ok(bytes)
}
pub(crate) fn validate(s: &State, d: &Draft) -> Result<()> {
    authorize(s, &d.scope)?;
    if d.text.trim().is_empty()
        || d.text.len() > 16384
        || d.tags.len() > 16
        || d.tags.iter().any(|t| t.is_empty() || t.len() > 128)
        || d.sources.is_empty()
        || d.sources.len() > 8
        || d.valid_until_millis.is_some_and(|t| t < 0)
        || encode(d)?.len() > 65536
        || !s.memory.safe(&encode(d)?)
    {
        return Err(Error::Invalid);
    }
    for source in &d.sources {
        if source.scope != d.scope
            || source.excerpt.is_empty()
            || source.excerpt.len() > 16384
            || source.sha256.len() != 64
            || !source.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Invalid);
        }
        if let Origin::File { reference }
        | Origin::Git { reference }
        | Origin::Execution { reference }
        | Origin::External { reference } = &source.origin
            && (reference.is_empty() || reference.len() > 1024)
        {
            return Err(Error::Invalid);
        }
        source_bytes(s, source)?;
    }
    match d.kind {
        Kind::FileEvidence
            if !d
                .sources
                .iter()
                .any(|src| matches!(src.origin, Origin::File { .. } | Origin::Git { .. })) =>
        {
            return Err(Error::Invalid);
        }
        Kind::ExecutionResult
            if !d
                .sources
                .iter()
                .any(|src| matches!(src.origin, Origin::Execution { .. })) =>
        {
            return Err(Error::Invalid);
        }
        _ => (),
    }
    Ok(())
}
pub(crate) fn load(s: &State, id: Uuid) -> Result<Memory> {
    let row: Option<(String, String, String)> = s
        .cache
        .db
        .query_row(
            "SELECT scope,head,state FROM memories WHERE id=?1",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (scope, head, state) = row.ok_or(Error::NotFound)?;
    authorize(s, &scope_from_key(&scope)?)?;
    let mut stmt=s.cache.db.prepare("SELECT r.id,r.parent,r.created,r.conflict,p.value FROM memory_revisions r JOIN memory_payloads p ON p.revision=r.id WHERE r.memory=?1 ORDER BY r.created,r.rowid LIMIT 1025")?;
    let rows = stmt.query_map([id.to_string()], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, bool>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    let mut revisions = Vec::new();
    for row in rows {
        let (rid, parent, created_millis, conflict, value) = row?;
        if !s.memory.safe(&value) {
            return Err(Error::Invalid);
        }
        let draft: Draft = decode(&value)?;
        if draft.scope.key() != scope {
            return Err(Error::UnsafeStorage);
        }
        revisions.push(Revision {
            id: parse(&rid)?,
            memory: id,
            parent: parent.map(|p| parse(&p)).transpose()?,
            created_millis,
            conflict,
            source_current: validate(s, &draft).is_ok(),
            draft,
        });
    }
    if revisions.len() > 1024 {
        return Err(Error::Invalid);
    }
    Ok(Memory {
        id,
        owner_user_id: s.cache.namespace().owner_user_id().cloned(),
        head: parse(&head)?,
        state,
        revisions,
    })
}
pub(crate) fn scope_from_key(key: &str) -> Result<Scope> {
    if key == "personal" {
        Ok(Scope::Personal)
    } else {
        Ok(Scope::Project {
            project_id: Id::try_from(
                key.strip_prefix("project:")
                    .ok_or(Error::UnsafeStorage)?
                    .to_owned(),
            )
            .map_err(|_| Error::UnsafeStorage)?,
        })
    }
}
fn parse(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| Error::UnsafeStorage)
}
pub(crate) fn deleted_source(s: &State, source: &Source) -> Result<bool> {
    Ok(s.cache
        .db
        .query_row(
            "SELECT 1 FROM memory_deleted_sources WHERE scope=?1 AND fingerprint=?2",
            params![source.scope.key(), source.fingerprint()?],
            |r| r.get::<_, i32>(0),
        )
        .optional()?
        .is_some())
}
pub(crate) fn insert(s: &mut State, draft: Draft, candidate: bool) -> Result<Uuid> {
    validate(s, &draft)?;
    let count: i64 = s.cache.db.query_row(
        "SELECT count(*) FROM memories WHERE scope=?1 AND state!='deleted'",
        [draft.scope.key()],
        |r| r.get(0),
    )?;
    if count >= 4096 {
        return Err(Error::Invalid);
    }
    if candidate
        && draft
            .sources
            .iter()
            .any(|src| deleted_source(s, src).unwrap_or(true))
    {
        return Err(Error::NotFound);
    }
    let id = Uuid::new_v4();
    let revision = Uuid::new_v4();
    let value = encode(&draft)?;
    let created = now()?;
    let tx = s.cache.db.transaction()?;
    tx.execute(
        "INSERT INTO memories VALUES(?1,?2,?3,?4)",
        params![
            id.to_string(),
            draft.scope.key(),
            revision.to_string(),
            if candidate { "candidate" } else { "active" }
        ],
    )?;
    tx.execute(
        "INSERT INTO memory_revisions VALUES(?1,?2,NULL,?3,0)",
        params![revision.to_string(), id.to_string(), created],
    )?;
    tx.execute(
        "INSERT INTO memory_payloads VALUES(?1,?2)",
        params![revision.to_string(), value],
    )?;
    queue(&tx, id, "create")?;
    tx.commit()?;
    Ok(id)
}
fn queue(tx: &rusqlite::Transaction<'_>, id: Uuid, kind: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO memory_outbox(operation,entity,kind) VALUES(?1,?2,?3)",
        params![Uuid::new_v4().to_string(), id.to_string(), kind],
    )?;
    Ok(())
}
pub(crate) fn eligible(s: &State, memory: &Memory, time: i64) -> bool {
    memory.state == "active"
        && memory
            .revisions
            .iter()
            .find(|r| r.id == memory.head)
            .is_some_and(|r| {
                r.draft.valid_until_millis.is_none_or(|t| t > time) && validate(s, &r.draft).is_ok()
            })
}
pub(crate) fn search(
    s: &State,
    scope: &Scope,
    keyword: &str,
    tag: Option<&str>,
    after_millis: Option<i64>,
    limit: usize,
) -> Result<Vec<Memory>> {
    authorize(s, scope)?;
    if !(1..=4096).contains(&limit) || keyword.len() > 1024 || tag.is_some_and(|t| t.len() > 128) {
        return Err(Error::Invalid);
    }
    let mut stmt=s.cache.db.prepare("SELECT m.id FROM memories m JOIN memory_revisions r ON r.id=m.head WHERE m.scope=?1 AND m.state='active' AND (?2 IS NULL OR r.created>=?2) ORDER BY r.created DESC,m.id LIMIT 4097")?;
    let ids = stmt
        .query_map(params![scope.key(), after_millis], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if ids.len() > 4096 {
        return Err(Error::Invalid);
    }
    let time = now()?;
    let needle = keyword.to_lowercase();
    let mut result = Vec::new();
    for id in ids {
        let m = load(s, parse(&id)?)?;
        if !eligible(s, &m, time) {
            continue;
        }
        let d = &m
            .revisions
            .iter()
            .find(|r| r.id == m.head)
            .ok_or(Error::UnsafeStorage)?
            .draft;
        if d.text.to_lowercase().contains(&needle)
            && tag.is_none_or(|tag| d.tags.iter().any(|t| t == tag))
        {
            result.push(m);
            if result.len() == limit {
                break;
            }
        }
    }
    Ok(result)
}
pub(crate) fn permitted(
    s: &State,
    target: &ProviderTarget,
    purpose: Purpose,
    draft: &Draft,
) -> Result<bool> {
    authorize(s, &draft.scope)?;
    let mut stmt = s
        .cache
        .db
        .prepare("SELECT value FROM memory_permissions LIMIT 1025")?;
    let values = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if values.len() > 1024 {
        return Err(Error::Invalid);
    }
    for value in values {
        let p: Permission = decode(&value)?;
        if &p.target == target
            && p.purpose == purpose
            && p.scope == draft.scope
            && p.kinds.contains(&draft.kind)
            && draft
                .sources
                .iter()
                .all(|src| p.source_types.contains(&src.origin.source_type()))
            && (draft.privacy == Privacy::Shareable || p.allow_local_only)
        {
            return Ok(true);
        }
    }
    Ok(false)
}
impl MemoryEngine<'_> {
    pub fn automatic(&self) -> Result<bool> {
        automatic(&*self.chat.state()?)
    }
    pub fn set_automatic(&self, enabled: bool) -> Result<()> {
        let mut s = self.chat.state()?;
        s.memory.cancel_all();
        s.cache.db.execute(
            "UPDATE memory_jobs SET state='cancelled' WHERE state='running'",
            [],
        )?;
        s.cache.db.execute(
            "UPDATE memory_settings SET automatic=?1 WHERE id=1",
            [enabled],
        )?;
        Ok(())
    }
    /// Local project selection is explicit and does not claim cloud membership.
    pub fn grant_project(&self, project: &Id) -> Result<()> {
        self.chat.state()?.cache.db.execute(
            "INSERT OR IGNORE INTO memory_projects VALUES(?1)",
            [String::from(project.clone())],
        )?;
        Ok(())
    }
    pub fn revoke_project(&self, project: &Id) -> Result<()> {
        let mut s = self.chat.state()?;
        s.memory.cancel_all();
        let scope = Scope::Project {
            project_id: project.clone(),
        };
        let tx = s.cache.db.transaction()?;
        tx.execute(
            "DELETE FROM memory_projects WHERE project=?1",
            [String::from(project.clone())],
        )?;
        tx.execute("DELETE FROM memory_permissions WHERE id IN(SELECT id FROM memory_permissions WHERE json_extract(value,'$.scope.project_id')=?1)",[String::from(project.clone())])?;
        tx.execute("UPDATE memory_jobs SET state='cancelled' WHERE scope=?1 AND state IN('waiting','running')",[scope.key()])?;
        tx.commit()?;
        Ok(())
    }
    pub fn allow(&self, permission: Permission) -> Result<Uuid> {
        let s = self.chat.state()?;
        authorize(&s, &permission.scope)?;
        if permission.target.endpoint.is_empty()
            || permission.target.endpoint.len() > 2048
            || permission.target.native_model.is_empty()
            || permission.target.native_model.len() > 256
            || permission.kinds.is_empty()
            || permission.kinds.len() > 5
            || permission.source_types.is_empty()
            || permission.source_types.len() > 6
            || !s.memory.safe(&encode(&permission)?)
        {
            return Err(Error::Invalid);
        }
        let actual = match permission.purpose {
            Purpose::Chat => {
                let m = s.model.as_ref().ok_or(Error::ModelUnavailable)?;
                m.provider.memory_stream_target(&m.model)
            }
            Purpose::Integration => {
                let m = s
                    .memory
                    .integration
                    .as_ref()
                    .ok_or(Error::ModelUnavailable)?;
                m.provider.memory_target(&m.model, false)
            }
            Purpose::Embedding => {
                let (m, _) = s.memory.embedding.as_ref().ok_or(Error::ModelUnavailable)?;
                m.provider.memory_target(&m.model, true)
            }
        }
        .map_err(|_| Error::ModelUnavailable)?;
        if actual != permission.target {
            return Err(Error::Invalid);
        }
        let count: i64 =
            s.cache
                .db
                .query_row("SELECT count(*) FROM memory_permissions", [], |r| r.get(0))?;
        if count >= 1024 {
            return Err(Error::Invalid);
        }
        let id = Uuid::new_v4();
        s.cache.db.execute(
            "INSERT INTO memory_permissions VALUES(?1,?2)",
            params![id.to_string(), encode(&permission)?],
        )?;
        Ok(id)
    }
    pub fn revoke(&self, id: Uuid) -> Result<()> {
        let mut s = self.chat.state()?;
        s.memory.cancel_all();
        s.cache.db.execute(
            "DELETE FROM memory_permissions WHERE id=?1",
            [id.to_string()],
        )?;
        s.cache.db.execute(
            "UPDATE memory_jobs SET state='cancelled' WHERE state='running'",
            [],
        )?;
        Ok(())
    }
    pub fn create(&self, draft: Draft) -> Result<Uuid> {
        insert(&mut *self.chat.state()?, draft, false)
    }
    pub fn get(&self, id: Uuid) -> Result<Memory> {
        load(&*self.chat.state()?, id)
    }
    pub fn correct(&self, id: Uuid, base: Uuid, draft: Draft) -> Result<Uuid> {
        let mut s = self.chat.state()?;
        let m = load(&s, id)?;
        if m.state == "deleted" || m.revisions.len() >= 1024 {
            return Err(Error::NotFound);
        }
        if !m.revisions.iter().any(|r| r.id == base) || m.revisions[0].draft.scope != draft.scope {
            return Err(Error::Invalid);
        }
        validate(&s, &draft)?;
        let conflict = m.head != base || m.state == "disputed";
        let revision = Uuid::new_v4();
        s.memory.cancel_all();
        let tx = s.cache.db.transaction()?;
        tx.execute(
            "INSERT INTO memory_revisions VALUES(?1,?2,?3,?4,?5)",
            params![
                revision.to_string(),
                id.to_string(),
                base.to_string(),
                now()?,
                conflict
            ],
        )?;
        tx.execute(
            "INSERT INTO memory_payloads VALUES(?1,?2)",
            params![revision.to_string(), encode(&draft)?],
        )?;
        if conflict {
            tx.execute(
                "UPDATE memories SET state='disputed' WHERE id=?1",
                [id.to_string()],
            )?;
        } else {
            tx.execute(
                "UPDATE memories SET head=?1,state='active' WHERE id=?2",
                params![revision.to_string(), id.to_string()],
            )?;
        }
        tx.execute("DELETE FROM memory_vectors WHERE revision IN(SELECT id FROM memory_revisions WHERE memory=?1)",[id.to_string()])?;
        queue(&tx, id, "revision")?;
        tx.commit()?;
        Ok(revision)
    }
    /// Explicit human selection resolves conflicts or accepts an AI candidate without changing its kind.
    pub fn approve(&self, id: Uuid, revision: Uuid) -> Result<()> {
        let mut s = self.chat.state()?;
        let m = load(&s, id)?;
        if m.state == "deleted" {
            return Err(Error::NotFound);
        }
        let r = m
            .revisions
            .iter()
            .find(|r| r.id == revision)
            .ok_or(Error::NotFound)?;
        validate(&s, &r.draft)?;
        s.memory.cancel_all();
        let tx = s.cache.db.transaction()?;
        tx.execute(
            "UPDATE memories SET head=?1,state='active' WHERE id=?2",
            params![revision.to_string(), id.to_string()],
        )?;
        tx.execute("DELETE FROM memory_vectors WHERE revision IN(SELECT id FROM memory_revisions WHERE memory=?1)",[id.to_string()])?;
        queue(&tx, id, "approve")?;
        tx.commit()?;
        Ok(())
    }
    pub fn search(
        &self,
        scope: &Scope,
        keyword: &str,
        tag: Option<&str>,
        after_millis: Option<i64>,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        if !(1..=100).contains(&limit) {
            return Err(Error::Invalid);
        }
        search(
            &*self.chat.state()?,
            scope,
            keyword,
            tag,
            after_millis,
            limit,
        )
    }
    pub fn list(&self, scope: &Scope, after: Option<Uuid>, limit: usize) -> Result<Vec<Memory>> {
        let s = self.chat.state()?;
        authorize(&s, scope)?;
        if !(1..=100).contains(&limit) {
            return Err(Error::Invalid);
        }
        let mut stmt=s.cache.db.prepare("SELECT id FROM memories WHERE scope=?1 AND state!='deleted' AND (?2 IS NULL OR id>?2) ORDER BY id LIMIT ?3")?;
        let ids = stmt
            .query_map(
                params![scope.key(), after.map(|id| id.to_string()), limit as i64],
                |r| r.get::<_, String>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.into_iter().map(|id| load(&s, parse(&id)?)).collect()
    }
    pub fn permissions(&self, scope: &Scope) -> Result<Vec<(Uuid, Permission)>> {
        let s = self.chat.state()?;
        authorize(&s, scope)?;
        let mut stmt = s
            .cache
            .db
            .prepare("SELECT id,value FROM memory_permissions ORDER BY id LIMIT 1025")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut result = Vec::new();
        let mut count = 0;
        for row in rows {
            count += 1;
            if count > 1024 {
                return Err(Error::Invalid);
            }
            let (id, value) = row?;
            if !s.memory.safe(&value) {
                return Err(Error::Invalid);
            }
            let p: Permission = decode(&value)?;
            if &p.scope == scope {
                result.push((parse(&id)?, p));
            }
        }
        Ok(result)
    }
    pub fn export(&self, scope: &Scope) -> Result<String> {
        let s = self.chat.state()?;
        authorize(&s, scope)?;
        let mut stmt = s.cache.db.prepare(
            "SELECT id FROM memories WHERE scope=?1 AND state!='deleted' ORDER BY id LIMIT 4097",
        )?;
        let ids = stmt
            .query_map([scope.key()], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if ids.len() > 4096 {
            return Err(Error::Invalid);
        }
        let mut rows = Vec::new();
        for id in ids {
            rows.push(load(&s, parse(&id)?)?);
        }
        let output = encode(&rows)?;
        if !s.memory.safe(&output) {
            return Err(Error::Invalid);
        }
        Ok(output)
    }
    pub fn delete(&self, id: Uuid) -> Result<()> {
        let mut s = self.chat.state()?;
        let m = load(&s, id)?;
        let affected = super::jobs::affected_jobs(&s, &m)?;
        s.memory.cancel_all();
        let tx = s.cache.db.transaction()?;
        for r in &m.revisions {
            for source in &r.draft.sources {
                tx.execute(
                    "INSERT OR IGNORE INTO memory_deleted_sources VALUES(?1,?2)",
                    params![source.scope.key(), source.fingerprint()?],
                )?;
            }
        }
        tx.execute(
            "UPDATE memories SET state='deleted' WHERE id=?1",
            [id.to_string()],
        )?;
        tx.execute("DELETE FROM memory_vectors WHERE revision IN(SELECT id FROM memory_revisions WHERE memory=?1)",[id.to_string()])?;
        tx.execute("DELETE FROM memory_payloads WHERE revision IN(SELECT id FROM memory_revisions WHERE memory=?1)",[id.to_string()])?;
        tx.execute(
            "UPDATE memory_jobs SET state='cancelled' WHERE state='running'",
            [],
        )?;
        for job in affected {
            tx.execute(
                "UPDATE memory_jobs SET state='cancelled',reference='' WHERE id=?1",
                [job],
            )?;
        }
        queue(&tx, id, "delete")?;
        tx.commit()?;
        Ok(())
    }
    pub fn chat_source(&self, conversation: Uuid, message: Uuid) -> Result<Source> {
        let s = self.chat.state()?;
        let m = s.cache.message(conversation, message)?;
        let project: Option<String> = s.cache.db.query_row(
            "SELECT project_id FROM conversations WHERE id=?1",
            [conversation.to_string()],
            |r| r.get(0),
        )?;
        let scope = project
            .map(|p| Id::try_from(p).map(|project_id| Scope::Project { project_id }))
            .transpose()
            .map_err(|_| Error::UnsafeStorage)?
            .unwrap_or(Scope::Personal);
        let source = Source::evidence(
            scope,
            Origin::Chat {
                conversation,
                message,
            },
            m.content.text.as_bytes(),
            m.content.text.clone(),
        );
        source_bytes(&s, &source)?;
        Ok(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_full_database_correction_rolls_back_head_payload_and_outbox() {
        let root = std::env::temp_dir().join(format!("caidex-i4-full-{}", Uuid::new_v4()));
        let chat = Chat::open(&root, crate::Namespace::guest()).unwrap();
        let make = |text: String| Draft {
            scope: Scope::Personal,
            text: text.clone(),
            kind: Kind::Preference,
            privacy: Privacy::LocalOnly,
            tags: vec![],
            sources: vec![Source::user(Scope::Personal, text)],
            valid_until_millis: None,
        };
        let id = chat.memory().create(make("preserved".into())).unwrap();
        let original = chat.memory().get(id).unwrap();
        let queued = {
            let s = chat.state().unwrap();
            let pages: i64 = s
                .cache
                .db
                .pragma_query_value(None, "page_count", |r| r.get(0))
                .unwrap();
            s.cache
                .db
                .pragma_update(None, "max_page_count", pages)
                .unwrap();
            s.cache
                .db
                .query_row("SELECT count(*) FROM memory_outbox", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap()
        };
        assert!(matches!(
            chat.memory()
                .correct(id, original.head, make("x".repeat(16000))),
            Err(Error::Storage)
        ));
        let preserved = chat.memory().get(id).unwrap();
        assert_eq!(preserved.head, original.head);
        assert_eq!(preserved.revisions.len(), 1);
        assert_eq!(preserved.revisions[0].draft.text, "preserved");
        assert_eq!(
            chat.state()
                .unwrap()
                .cache
                .db
                .query_row("SELECT count(*) FROM memory_outbox", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            queued
        );
        drop(chat);
        std::fs::remove_dir_all(root).unwrap();
    }
}
