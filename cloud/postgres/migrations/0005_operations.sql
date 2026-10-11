-- I-7: durable bounded mail status/budget; verification payload exists only in sending process.
BEGIN;
SET LOCAL lock_timeout='5s';
SET LOCAL statement_timeout='30s';
SET LOCAL ROLE caidex_owner;
CREATE TABLE caidex.mail_tasks(
 task_id uuid PRIMARY KEY,
 recipient_hash bytea NOT NULL CHECK(octet_length(recipient_hash)=32),
 payload_hash bytea NOT NULL CHECK(octet_length(payload_hash)=32),
 purpose text NOT NULL CHECK(octet_length(purpose) BETWEEN 1 AND 64),
 provider text NOT NULL CHECK(provider IN('brevo','resend')),
 provider_id text CHECK(octet_length(provider_id) BETWEEN 1 AND 512),
 status text NOT NULL DEFAULT 'unknown' CHECK(status IN('unknown','accepted','rejected','delivered','bounced','complaint')),
 created_at timestamptz NOT NULL DEFAULT now(),
 expires_at timestamptz NOT NULL,
 updated_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(provider,provider_id)
);
CREATE INDEX mail_daily_budget ON caidex.mail_tasks(created_at);
CREATE TABLE caidex.mail_suppressions(
 recipient_hash bytea PRIMARY KEY CHECK(octet_length(recipient_hash)=32),
 reason text NOT NULL CHECK(reason IN('bounce','complaint')),
 created_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE caidex.mail_tasks ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.mail_tasks FORCE ROW LEVEL SECURITY;
ALTER TABLE caidex.mail_suppressions ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.mail_suppressions FORCE ROW LEVEL SECURITY;
CREATE POLICY authentication ON caidex.mail_tasks TO caidex_auth USING(true) WITH CHECK(true);
CREATE POLICY authentication ON caidex.mail_suppressions TO caidex_auth USING(true) WITH CHECK(true);
GRANT SELECT,INSERT,UPDATE ON caidex.mail_tasks TO caidex_auth;
GRANT SELECT,INSERT ON caidex.mail_suppressions TO caidex_auth;
RESET ROLE;
-- A statement-local authorized scope set avoids repeating nested account/ACL checks for each vector.
-- No body access or extra privilege: the guard already reads these four authorization tables.
CREATE INDEX memory_revisions_by_memory ON caidex.memory_revisions(memory_id);
CREATE INDEX memory_sources_by_revision ON caidex.memory_sources(revision_id);
CREATE INDEX memory_jobs_by_source ON caidex.memory_jobs(source_id);
CREATE INDEX memory_jobs_by_revision ON caidex.memory_jobs(base_revision_id);
CREATE FUNCTION caidex.read_memory_scopes() RETURNS TABLE(owner_id uuid,scope text)
LANGUAGE sql STABLE SECURITY DEFINER ROWS 64 SET search_path=pg_catalog AS $$
 WITH actor AS MATERIALIZED (
  SELECT user_id,allowed_scopes FROM caidex.users
  WHERE user_id=(SELECT caidex.actor()) AND state='active' AND memory_enabled AND NOT memory_delete_pending
 )
 SELECT a.user_id,'personal'::text FROM actor a WHERE a.allowed_scopes @> '[{"type":"personal"}]'::jsonb
 UNION ALL
 SELECT p.owner_user_id,p.project_id::text FROM caidex.projects p
 JOIN caidex.users u ON u.user_id=p.owner_user_id AND u.state='active' AND u.memory_enabled AND NOT u.memory_delete_pending
 CROSS JOIN actor a
 WHERE (p.owner_user_id=a.user_id OR EXISTS(SELECT 1 FROM caidex.project_members m WHERE m.project_id=p.project_id AND m.user_id=a.user_id AND m.revoked_at IS NULL))
 AND a.allowed_scopes @> jsonb_build_array(jsonb_build_object('type','project','project_id',p.project_id::text))
 AND u.allowed_scopes @> jsonb_build_array(jsonb_build_object('type','project','project_id',p.project_id::text))
$$;
GRANT CREATE ON SCHEMA caidex TO caidex_guard;
ALTER FUNCTION caidex.read_memory_scopes() OWNER TO caidex_guard;
REVOKE CREATE ON SCHEMA caidex FROM caidex_guard;
REVOKE ALL ON FUNCTION caidex.read_memory_scopes() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION caidex.read_memory_scopes() TO caidex_app;
SET LOCAL ROLE caidex_owner;
ALTER POLICY scoped_read ON caidex.memories USING ((user_id,scope_key) IN(SELECT owner_id,scope FROM caidex.read_memory_scopes()));
ALTER POLICY scoped_read ON caidex.memory_revisions USING ((user_id,scope_key) IN(SELECT owner_id,scope FROM caidex.read_memory_scopes()));
ALTER POLICY scoped_read ON caidex.memory_embeddings USING (
 (user_id,scope_key) IN(SELECT owner_id,scope FROM caidex.read_memory_scopes())
 AND state='ready' AND EXISTS(SELECT 1 FROM caidex.memories m JOIN caidex.memory_revisions r ON r.revision_id=m.current_revision_id AND r.memory_id=m.memory_id
  WHERE r.revision_id=memory_embeddings.revision_id AND m.state='active' AND r.valid_from<=statement_timestamp() AND (r.valid_until IS NULL OR r.valid_until>statement_timestamp())));
INSERT INTO caidex.schema_migrations(version) VALUES(5);
COMMIT;
