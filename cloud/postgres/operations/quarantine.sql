-- Administrator-only, on a newly created disconnected recovery database, never an API/RLS bypass.
SET LOCAL statement_timeout='30s';
SET LOCAL lock_timeout='5s';
CREATE TEMP TABLE previous_epochs AS SELECT user_id,cloud_epoch FROM caidex.users;
CREATE TEMP TABLE latest_users AS SELECT * FROM jsonb_populate_recordset(NULL::caidex.users,(SELECT coalesce(value->'users','[]'::jsonb) FROM safety));
CREATE TEMP TABLE latest_memories AS SELECT * FROM jsonb_populate_recordset(NULL::caidex.memories,(SELECT coalesce(value->'memories','[]'::jsonb) FROM safety));
CREATE TEMP TABLE latest_identities AS SELECT * FROM jsonb_populate_recordset(NULL::caidex.user_identities,(SELECT coalesce(value->'identities','[]'::jsonb) FROM safety));
CREATE TEMP TABLE latest_credentials AS SELECT * FROM jsonb_to_recordset((SELECT coalesce(value->'credentials','[]'::jsonb) FROM safety)) AS x(credential_id uuid,fingerprint text);
-- An unproved identity/password/Passkey cannot authenticate after recovery, including changed email.
UPDATE caidex.auth_credentials c SET revoked_at=coalesce(c.revoked_at,now())
 WHERE NOT EXISTS(SELECT 1 FROM latest_credentials l WHERE l.credential_id=c.credential_id AND l.fingerprint=encode(digest(to_jsonb(c)::text,'sha256'),'hex'));
UPDATE caidex.user_identities i SET state='revoked' WHERE NOT EXISTS(
 SELECT 1 FROM latest_identities l WHERE l.identity_id=i.identity_id AND l.user_id=i.user_id AND l.kind=i.kind AND l.issuer=i.issuer AND l.subject=i.subject AND l.state=i.state);
UPDATE caidex.auth_credentials c SET revoked_at=coalesce(c.revoked_at,now()) WHERE c.identity_id IN(SELECT identity_id FROM caidex.user_identities WHERE state='revoked');
UPDATE caidex.users u SET state=l.state,settings_version=greatest(u.settings_version,l.settings_version),
 cloud_epoch=greatest(u.cloud_epoch,l.cloud_epoch),scope_version=greatest(u.scope_version,l.scope_version),
 chat_settings_version=greatest(u.chat_settings_version,l.chat_settings_version),allowed_scopes=l.allowed_scopes,
 memory_delete_pending=l.memory_delete_pending FROM latest_users l WHERE l.user_id=u.user_id;
UPDATE caidex.users u SET state='suspended' WHERE NOT EXISTS(SELECT 1 FROM latest_users l WHERE l.user_id=u.user_id);
-- Restore the latest proven ACL only for entities that actually survived the data snapshot.
DELETE FROM caidex.project_members WHERE EXISTS(SELECT 1 FROM safety WHERE value ? 'users');
INSERT INTO caidex.project_members SELECT m.* FROM jsonb_populate_recordset(NULL::caidex.project_members,(SELECT coalesce(value->'members','[]'::jsonb) FROM safety)) m
 JOIN caidex.users u ON u.user_id=m.user_id JOIN caidex.projects p ON p.project_id=m.project_id AND p.owner_user_id=m.granted_by;
UPDATE caidex.projects p SET acl_version=greatest(p.acl_version,l.acl_version) FROM jsonb_to_recordset((SELECT coalesce(value->'projects','[]'::jsonb) FROM safety)) AS l(project_id uuid,acl_version bigint) WHERE l.project_id=p.project_id;
-- Latest deletion/source barriers survive even when their original body never existed in this snapshot.
INSERT INTO caidex.deletion_tombstones(user_id,project_id,tombstone_id,entity_id,entity_kind,deletion_revision,seq,cloud_epoch,source_hash,retain_until,memory_id) SELECT t.user_id,t.project_id,t.tombstone_id,t.entity_id,t.entity_kind,t.deletion_revision,t.seq,t.cloud_epoch,t.source_hash,t.retain_until,t.memory_id FROM jsonb_populate_recordset(NULL::caidex.deletion_tombstones,(SELECT coalesce(value->'tombstones','[]'::jsonb) FROM safety)) t
 JOIN caidex.users u ON u.user_id=t.user_id WHERE t.project_id IS NULL OR EXISTS(SELECT 1 FROM caidex.projects p WHERE p.project_id=t.project_id AND p.owner_user_id=t.user_id)
 ON CONFLICT(user_id,scope_key,entity_kind,entity_id) DO UPDATE SET cloud_epoch=greatest(caidex.deletion_tombstones.cloud_epoch,excluded.cloud_epoch),seq=greatest(caidex.deletion_tombstones.seq,excluded.seq),retain_until='infinity';
CREATE TEMP TABLE erased_memories AS SELECT m.* FROM caidex.memories m JOIN caidex.users u USING(user_id) JOIN previous_epochs e USING(user_id)
 WHERE u.state='deleted' OR u.memory_delete_pending OR (EXISTS(SELECT 1 FROM latest_users l WHERE l.user_id=m.user_id) AND
 (e.cloud_epoch<u.cloud_epoch OR u.memory_delete_pending OR NOT EXISTS(SELECT 1 FROM latest_memories l WHERE l.memory_id=m.memory_id AND l.state<>'deleted')));
INSERT INTO caidex.deletion_tombstones(user_id,project_id,entity_id,entity_kind,seq,cloud_epoch,retain_until)
 SELECT m.user_id,m.project_id,m.memory_id,'memory',1,u.cloud_epoch,'infinity' FROM erased_memories m JOIN caidex.users u USING(user_id)
 ON CONFLICT(user_id,scope_key,entity_kind,entity_id) DO NOTHING;
INSERT INTO caidex.deletion_tombstones(user_id,project_id,entity_id,entity_kind,seq,cloud_epoch,source_hash,retain_until,memory_id)
 SELECT DISTINCT m.user_id,m.project_id,s.source_id,'source',1,u.cloud_epoch,s.content_hash,'infinity'::timestamptz,m.memory_id FROM erased_memories m JOIN caidex.users u USING(user_id) JOIN caidex.memory_revisions r USING(memory_id) JOIN caidex.memory_sources s USING(revision_id) WHERE s.content_hash IS NOT NULL
 ON CONFLICT(user_id,scope_key,entity_kind,entity_id) DO NOTHING;
-- No recovered jobs may execute from an old snapshot or restored approval.
DELETE FROM caidex.memory_jobs;
UPDATE caidex.memories SET current_revision_id=NULL WHERE memory_id IN(SELECT memory_id FROM erased_memories);
DELETE FROM caidex.memory_embeddings WHERE revision_id IN(SELECT revision_id FROM caidex.memory_revisions WHERE memory_id IN(SELECT memory_id FROM erased_memories));
DELETE FROM caidex.memory_sources WHERE revision_id IN(SELECT revision_id FROM caidex.memory_revisions WHERE memory_id IN(SELECT memory_id FROM erased_memories));
DELETE FROM caidex.memory_revisions WHERE memory_id IN(SELECT memory_id FROM erased_memories);
DELETE FROM caidex.memories WHERE memory_id IN(SELECT memory_id FROM erased_memories);
-- A stale revision remains available for review, never silently promoted to the latest fact.
UPDATE caidex.memories m SET state='disputed',current_revision_id=NULL FROM latest_memories l WHERE l.memory_id=m.memory_id AND (l.current_revision_id IS DISTINCT FROM m.current_revision_id OR l.sync_generation<>m.sync_generation);
UPDATE caidex.conversations c SET state='deleted',title='' WHERE c.user_id IN(SELECT user_id FROM caidex.users WHERE state='deleted') OR EXISTS(SELECT 1 FROM caidex.deletion_tombstones t WHERE t.entity_kind='conversation' AND t.entity_id=c.conversation_id AND t.user_id=c.user_id);
DELETE FROM caidex.attachments a WHERE EXISTS(SELECT 1 FROM caidex.conversations c WHERE c.conversation_id=a.conversation_id AND c.state='deleted') OR EXISTS(SELECT 1 FROM caidex.deletion_tombstones t WHERE t.user_id=a.user_id AND t.entity_kind='attachment' AND t.entity_id=a.attachment_id);
-- Keep immutable message identities used by independently retained Memory sources.
UPDATE caidex.messages m SET body='',attachment_ids='{}' WHERE EXISTS(SELECT 1 FROM caidex.conversations c WHERE c.conversation_id=m.conversation_id AND c.state='deleted') OR EXISTS(SELECT 1 FROM caidex.deletion_tombstones t WHERE t.user_id=m.user_id AND t.entity_kind='message' AND t.entity_id=m.message_id);
UPDATE caidex.auth_sessions SET revoked_at=coalesce(revoked_at,now());
UPDATE caidex.auth_challenges SET consumed_at=coalesce(consumed_at,now()),state='{}';
UPDATE caidex.auth_grants SET status='cancelled';
DELETE FROM caidex.refresh_history;
DELETE FROM caidex.sync_cursors;
DELETE FROM caidex.memory_controls;
DELETE FROM caidex.chat_controls;
-- Receipts and queued events cannot re-authorize a pre-recovery operation.
DELETE FROM caidex.sync_events;
UPDATE caidex.users SET memory_enabled=false,chat_sync_enabled=false,settings_version=settings_version+1,
 chat_settings_version=chat_settings_version+1,cloud_epoch=cloud_epoch+1,scope_version=scope_version+1,allowed_scopes='[]',memory_delete_pending=false;
