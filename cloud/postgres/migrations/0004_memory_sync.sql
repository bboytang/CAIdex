BEGIN;
SET LOCAL ROLE caidex_owner;
ALTER TABLE caidex.users ADD COLUMN memory_delete_pending boolean NOT NULL DEFAULT false;
ALTER TABLE caidex.memories ADD COLUMN sync_generation bigint NOT NULL DEFAULT 1 CHECK(sync_generation>0);
ALTER TABLE caidex.memory_revisions ADD COLUMN payload jsonb CHECK(octet_length(payload::text)<=65536);
ALTER TABLE caidex.memory_revisions ADD COLUMN created_millis bigint NOT NULL DEFAULT 0 CHECK(created_millis>=0);
ALTER TABLE caidex.memory_sources ADD COLUMN excerpt text NOT NULL DEFAULT '' CHECK(octet_length(excerpt)<=16384);
-- Local Chat may be explicitly shared as a source without enabling Chat sync.
ALTER TABLE caidex.memory_sources DROP CONSTRAINT memory_sources_check;
ALTER TABLE caidex.memory_sources ADD CHECK(source_kind='chat' OR message_id IS NULL);
ALTER TABLE caidex.deletion_tombstones ADD COLUMN memory_id uuid;
ALTER TABLE caidex.sync_events ADD COLUMN memory_generation bigint CHECK(memory_generation>0);
ALTER TABLE caidex.sync_events ADD COLUMN submitted_by uuid REFERENCES caidex.users;
CREATE TABLE caidex.memory_controls(
 user_id uuid NOT NULL REFERENCES caidex.users,operation_id uuid NOT NULL,
 operation_hash bytea NOT NULL CHECK(octet_length(operation_hash)=32),result jsonb NOT NULL,
 PRIMARY KEY(user_id,operation_id)
);
ALTER TABLE caidex.memory_controls ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.memory_controls FORCE ROW LEVEL SECURITY;
CREATE POLICY authentication ON caidex.memory_controls TO caidex_auth USING(true) WITH CHECK(true);
GRANT SELECT,INSERT ON caidex.memory_controls TO caidex_auth;
GRANT UPDATE(memory_enabled,settings_version,cloud_epoch,scope_version,allowed_scopes,memory_delete_pending) ON caidex.users TO caidex_auth;
GRANT SELECT(memory_delete_pending) ON caidex.users TO caidex_app;
GRANT UPDATE(sync_generation) ON caidex.memories TO caidex_app;
RESET ROLE;
GRANT CREATE ON SCHEMA caidex TO caidex_guard;
CREATE OR REPLACE FUNCTION caidex.memory_access(owner_id uuid,project uuid) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT caidex.scope_access(owner_id,project,false)
 AND EXISTS(SELECT 1 FROM caidex.users WHERE user_id=owner_id AND state='active' AND memory_enabled AND NOT memory_delete_pending
   AND allowed_scopes @> jsonb_build_array(CASE WHEN project IS NULL THEN jsonb_build_object('type','personal') ELSE jsonb_build_object('type','project','project_id',project::text) END))
 AND EXISTS(SELECT 1 FROM caidex.users WHERE user_id=caidex.actor() AND state='active' AND memory_enabled AND NOT memory_delete_pending
   AND allowed_scopes @> jsonb_build_array(CASE WHEN project IS NULL THEN jsonb_build_object('type','personal') ELSE jsonb_build_object('type','project','project_id',project::text) END))
$$;
CREATE FUNCTION caidex.memory_permit(owner_id uuid,project uuid) RETURNS TABLE(version bigint,epoch bigint,scope_version bigint,enabled boolean,delete_pending boolean) LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT u.settings_version,u.cloud_epoch,u.scope_version,caidex.memory_access(owner_id,project),u.memory_delete_pending FROM caidex.users u WHERE u.user_id=owner_id AND u.state='active' AND caidex.scope_access(owner_id,project,false)
$$;
CREATE FUNCTION caidex.memory_job_current(owner_id uuid,version bigint,epoch bigint) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT EXISTS(SELECT 1 FROM caidex.users u WHERE u.user_id=owner_id AND u.settings_version=version AND u.cloud_epoch=epoch AND u.memory_enabled AND NOT u.memory_delete_pending)
$$;
-- Hold ACL rows until the data transaction ends; authorization is rechecked after locking.
CREATE FUNCTION caidex.lock_memory_acl(owner_id uuid,project uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
BEGIN
 IF project IS NOT NULL THEN
  PERFORM 1 FROM caidex.projects WHERE project_id=project AND owner_user_id=owner_id FOR SHARE;
  PERFORM 1 FROM caidex.project_members WHERE project_id=project AND user_id=caidex.actor() FOR SHARE;
 END IF;
 IF NOT caidex.scope_access(owner_id,project,false) THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='Memory scope refused'; END IF;
END $$;
ALTER FUNCTION caidex.memory_permit(uuid,uuid) OWNER TO caidex_guard;
ALTER FUNCTION caidex.memory_job_current(uuid,bigint,bigint) OWNER TO caidex_guard;
ALTER FUNCTION caidex.lock_memory_acl(uuid,uuid) OWNER TO caidex_guard;
REVOKE CREATE ON SCHEMA caidex FROM caidex_guard;
GRANT UPDATE(title) ON caidex.projects TO caidex_guard;
GRANT UPDATE(authorization_version) ON caidex.project_members TO caidex_guard;
REVOKE ALL ON FUNCTION caidex.memory_permit(uuid,uuid),caidex.memory_job_current(uuid,bigint,bigint),caidex.lock_memory_acl(uuid,uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION caidex.memory_permit(uuid,uuid),caidex.memory_job_current(uuid,bigint,bigint),caidex.lock_memory_acl(uuid,uuid) TO caidex_app;
SET LOCAL ROLE caidex_owner;
CREATE POLICY current_job ON caidex.memory_jobs AS RESTRICTIVE TO caidex_app USING(caidex.memory_job_current(user_id,settings_version,cloud_epoch)) WITH CHECK(caidex.memory_job_current(user_id,settings_version,cloud_epoch));
-- The non-owner guard sees only an explicit deletion barrier or a live scoped memory.
CREATE POLICY guard_erasure ON caidex.memories TO caidex_guard
 USING((user_id=caidex.actor() AND EXISTS(SELECT 1 FROM caidex.users u WHERE u.user_id=memories.user_id AND u.memory_delete_pending AND NOT u.memory_enabled)) OR (caidex.memory_access(user_id,project_id) AND caidex.scope_access(user_id,project_id,true)))
 WITH CHECK(false);
DO $$ DECLARE t text; BEGIN
 FOREACH t IN ARRAY ARRAY['memory_revisions','memory_sources','memory_embeddings','memory_jobs'] LOOP
  EXECUTE format('CREATE POLICY guard_erasure ON caidex.%I TO caidex_guard USING((user_id=caidex.actor() AND EXISTS(SELECT 1 FROM caidex.users u WHERE u.user_id=%I.user_id AND u.memory_delete_pending AND NOT u.memory_enabled)) OR EXISTS(SELECT 1 FROM caidex.memories m WHERE m.user_id=%I.user_id AND m.scope_key=%I.scope_key AND m.state=''deleted'')) WITH CHECK(false)',t,t,t,t);
 END LOOP;
END $$;
CREATE POLICY guard_erasure ON caidex.embedding_spaces TO caidex_guard USING(user_id=caidex.actor() AND EXISTS(SELECT 1 FROM caidex.users u WHERE u.user_id=embedding_spaces.user_id AND u.memory_delete_pending AND NOT u.memory_enabled)) WITH CHECK(false);
CREATE POLICY guard_erasure ON caidex.deletion_tombstones TO caidex_guard USING(
 (user_id=caidex.actor() AND EXISTS(SELECT 1 FROM caidex.users u WHERE u.user_id=deletion_tombstones.user_id AND u.memory_delete_pending AND NOT u.memory_enabled)) OR (caidex.memory_access(user_id,project_id) AND caidex.scope_access(user_id,project_id,true))) WITH CHECK(
 (user_id=caidex.actor() AND EXISTS(SELECT 1 FROM caidex.users u WHERE u.user_id=deletion_tombstones.user_id AND u.memory_delete_pending AND NOT u.memory_enabled)) OR (caidex.memory_access(user_id,project_id) AND caidex.scope_access(user_id,project_id,true)));
CREATE POLICY guard_erasure ON caidex.sync_events FOR SELECT TO caidex_guard USING(channel='memory' AND ((user_id=caidex.actor() AND EXISTS(SELECT 1 FROM caidex.users u WHERE u.user_id=sync_events.user_id AND u.memory_delete_pending)) OR caidex.memory_access(user_id,project_id)));
GRANT SELECT,DELETE ON caidex.memories,caidex.memory_revisions,caidex.memory_sources,caidex.memory_embeddings,caidex.memory_jobs,caidex.embedding_spaces TO caidex_guard;
GRANT SELECT,INSERT ON caidex.deletion_tombstones TO caidex_guard;
GRANT SELECT ON caidex.sync_events TO caidex_guard;
CREATE FUNCTION caidex.erase_memory(id uuid) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
DECLARE m record; seq bigint; epoch bigint;
BEGIN
 IF id IS NULL THEN
  IF NOT EXISTS(SELECT 1 FROM caidex.users WHERE user_id=caidex.actor() AND memory_delete_pending AND NOT memory_enabled) THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='Memory deletion barrier required'; END IF;
 ELSE
  IF NOT EXISTS(SELECT 1 FROM caidex.memories WHERE memory_id=id AND state='deleted' AND caidex.memory_access(user_id,project_id) AND caidex.scope_access(user_id,project_id,true)) THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='Memory erasure refused'; END IF;
 END IF;
 FOR m IN SELECT * FROM caidex.memories WHERE (id IS NULL AND user_id=caidex.actor()) OR memory_id=id ORDER BY memory_id LIMIT 32 LOOP
  SELECT u.cloud_epoch INTO epoch FROM caidex.users u WHERE u.user_id=m.user_id;
  SELECT coalesce(max(e.seq),1) INTO seq FROM caidex.sync_events e WHERE e.user_id=m.user_id AND e.channel='memory';
  INSERT INTO caidex.deletion_tombstones(user_id,project_id,entity_id,entity_kind,seq,cloud_epoch,retain_until) VALUES(m.user_id,m.project_id,m.memory_id,'memory',seq,epoch,'infinity') ON CONFLICT(user_id,scope_key,entity_kind,entity_id) DO NOTHING;
  INSERT INTO caidex.deletion_tombstones(user_id,project_id,entity_id,entity_kind,seq,cloud_epoch,source_hash,retain_until,memory_id)
   SELECT DISTINCT m.user_id,m.project_id,s.source_id,'source',seq,epoch,s.content_hash,'infinity'::timestamptz,m.memory_id FROM caidex.memory_sources s JOIN caidex.memory_revisions r USING(revision_id) WHERE r.memory_id=m.memory_id AND s.content_hash IS NOT NULL ON CONFLICT(user_id,scope_key,entity_kind,entity_id) DO NOTHING;
  DELETE FROM caidex.memory_jobs WHERE source_id IN(SELECT s.source_id FROM caidex.memory_sources s JOIN caidex.memory_revisions r USING(revision_id) WHERE r.memory_id=m.memory_id) OR base_revision_id IN(SELECT revision_id FROM caidex.memory_revisions WHERE memory_id=m.memory_id);
  DELETE FROM caidex.memory_embeddings WHERE revision_id IN(SELECT revision_id FROM caidex.memory_revisions WHERE memory_id=m.memory_id);
  DELETE FROM caidex.memory_sources WHERE revision_id IN(SELECT revision_id FROM caidex.memory_revisions WHERE memory_id=m.memory_id);
  DELETE FROM caidex.memory_revisions WHERE memory_id=m.memory_id;
  DELETE FROM caidex.memories WHERE memory_id=m.memory_id;
 END LOOP;
 IF id IS NULL THEN
  IF EXISTS(SELECT 1 FROM caidex.memories WHERE user_id=caidex.actor()) THEN RETURN false; END IF;
  DELETE FROM caidex.memory_jobs WHERE user_id=caidex.actor();
  DELETE FROM caidex.embedding_spaces WHERE user_id=caidex.actor();
 END IF;
 RETURN true;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA caidex TO caidex_guard;
ALTER FUNCTION caidex.erase_memory(uuid) OWNER TO caidex_guard;
REVOKE CREATE ON SCHEMA caidex FROM caidex_guard;
REVOKE ALL ON FUNCTION caidex.erase_memory(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION caidex.erase_memory(uuid) TO caidex_app;
INSERT INTO caidex.schema_migrations(version) VALUES(4);
COMMIT;
