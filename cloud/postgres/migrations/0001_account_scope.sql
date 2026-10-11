-- I-1 schema version 1. Forward-only, transactional, executed by the migration administrator.
BEGIN;
SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL ROLE caidex_owner;
CREATE TABLE caidex.schema_migrations (
    version integer PRIMARY KEY CHECK (version > 0),
    applied_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE caidex.users (
    user_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    state text NOT NULL DEFAULT 'active' CHECK (state IN ('active','suspended','deleted')),
    display_name text NOT NULL DEFAULT '' CHECK (octet_length(display_name) <= 256),
    memory_enabled boolean NOT NULL DEFAULT false,
    chat_sync_enabled boolean NOT NULL DEFAULT false,
    settings_version bigint NOT NULL DEFAULT 1 CHECK (settings_version > 0),
    cloud_epoch bigint NOT NULL DEFAULT 1 CHECK (cloud_epoch > 0),
    scope_version bigint NOT NULL DEFAULT 1 CHECK (scope_version > 0),
    allowed_scopes jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(allowed_scopes) = 'array'),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE caidex.user_identities (
    identity_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES caidex.users,
    kind text NOT NULL CHECK (kind IN ('email','oidc')),
    issuer text NOT NULL,
    subject text NOT NULL CHECK (octet_length(subject) BETWEEN 1 AND 1024),
    verified_at timestamptz,
    state text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','verified','revoked')),
    UNIQUE (issuer, subject), UNIQUE (identity_id, user_id)
);
CREATE TABLE caidex.auth_credentials (
    credential_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES caidex.users,
    identity_id uuid,
    kind text NOT NULL CHECK (kind IN ('password','passkey')),
    password_hash text,
    passkey_id bytea UNIQUE,
    public_key bytea,
    rp_id text,
    algorithm_version text NOT NULL,
    revoked_at timestamptz,
    FOREIGN KEY (identity_id,user_id) REFERENCES caidex.user_identities(identity_id,user_id),
    CHECK ((kind = 'password' AND password_hash IS NOT NULL AND password_hash LIKE '$argon2id$%' AND passkey_id IS NULL AND public_key IS NULL AND rp_id IS NULL)
        OR (kind = 'passkey' AND password_hash IS NULL AND passkey_id IS NOT NULL AND public_key IS NOT NULL AND rp_id IS NOT NULL))
);
CREATE TABLE caidex.auth_sessions (
    session_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES caidex.users,
    access_hash bytea NOT NULL UNIQUE CHECK (octet_length(access_hash) = 32),
    refresh_hash bytea NOT NULL UNIQUE CHECK (octet_length(refresh_hash) = 32),
    refresh_family uuid NOT NULL DEFAULT gen_random_uuid(),
    rotation bigint NOT NULL DEFAULT 0 CHECK (rotation >= 0),
    client_id text NOT NULL,
    audience text NOT NULL CHECK (audience = 'caidex-data-v1'),
    platform text NOT NULL CHECK (octet_length(platform) <= 128),
    installation_id uuid,
    display_name text NOT NULL DEFAULT '' CHECK (octet_length(display_name) <= 256),
    created_at timestamptz NOT NULL DEFAULT now(),
    last_active_at timestamptz NOT NULL DEFAULT now(),
    access_expires_at timestamptz NOT NULL,
    refresh_expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    UNIQUE (session_id,user_id)
);
CREATE TABLE caidex.security_events (
    event_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES caidex.users,
    session_id uuid,
    kind text NOT NULL CHECK (kind IN ('login','recovery','revocation','identity_change','deletion','alert')),
    outcome text NOT NULL CHECK (outcome IN ('success','refused','pending')),
    created_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (session_id,user_id) REFERENCES caidex.auth_sessions(session_id,user_id)
);
CREATE TABLE caidex.projects (
    project_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_user_id uuid NOT NULL REFERENCES caidex.users,
    title text NOT NULL CHECK (octet_length(title) BETWEEN 1 AND 1024),
    acl_version bigint NOT NULL DEFAULT 1 CHECK (acl_version > 0),
    UNIQUE (project_id,owner_user_id)
);
CREATE TABLE caidex.project_members (
    project_id uuid NOT NULL REFERENCES caidex.projects,
    user_id uuid NOT NULL REFERENCES caidex.users,
    role text NOT NULL CHECK (role IN ('reader','writer')),
    granted_by uuid NOT NULL,
    granted_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz,
    authorization_version bigint NOT NULL DEFAULT 1 CHECK (authorization_version > 0),
    PRIMARY KEY (project_id,user_id),
    FOREIGN KEY (project_id,granted_by) REFERENCES caidex.projects(project_id,owner_user_id)
);
CREATE TABLE caidex.conversations (
    user_id uuid NOT NULL REFERENCES caidex.users,
    project_id uuid,
    scope_key text GENERATED ALWAYS AS (coalesce(project_id::text,'personal')) STORED,
    conversation_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    branch_of uuid CHECK (branch_of IS DISTINCT FROM conversation_id),
    kind text NOT NULL DEFAULT 'chat' CHECK (kind = 'chat'),
    model_ref text,
    version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
    state text NOT NULL DEFAULT 'active' CHECK (state IN ('active','deleted')),
    FOREIGN KEY (branch_of,user_id,scope_key) REFERENCES caidex.conversations(conversation_id,user_id,scope_key),
    UNIQUE (conversation_id,user_id,scope_key),
    FOREIGN KEY (project_id,user_id) REFERENCES caidex.projects(project_id,owner_user_id)
);
CREATE INDEX ON caidex.conversations(user_id,scope_key);
CREATE TABLE caidex.messages (
    user_id uuid NOT NULL REFERENCES caidex.users,
    project_id uuid,
    scope_key text GENERATED ALWAYS AS (coalesce(project_id::text,'personal')) STORED,
    message_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id uuid NOT NULL,
    base_message_id uuid CHECK (base_message_id IS DISTINCT FROM message_id),
    author_user_id uuid NOT NULL REFERENCES caidex.users,
    role text NOT NULL CHECK (role IN ('user','assistant')),
    body text NOT NULL CHECK (octet_length(body) <= 1048576),
    state text NOT NULL CHECK (state IN ('complete','interrupted')),
    created_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (conversation_id,user_id,scope_key) REFERENCES caidex.conversations(conversation_id,user_id,scope_key),
    FOREIGN KEY (base_message_id,user_id,scope_key) REFERENCES caidex.messages(message_id,user_id,scope_key),
    UNIQUE (message_id,user_id,scope_key),
    FOREIGN KEY (project_id,user_id) REFERENCES caidex.projects(project_id,owner_user_id)
);
CREATE INDEX ON caidex.messages(user_id,scope_key);
CREATE TABLE caidex.memories (
    user_id uuid NOT NULL REFERENCES caidex.users,
    project_id uuid,
    scope_key text GENERATED ALWAYS AS (coalesce(project_id::text,'personal')) STORED,
    memory_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    current_revision_id uuid,
    state text NOT NULL DEFAULT 'candidate' CHECK (state IN ('candidate','active','disputed','superseded','deleted')),
    privacy text NOT NULL DEFAULT 'local_only' CHECK (privacy IN ('local_only','sync_allowed')),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (memory_id,user_id,scope_key),
    FOREIGN KEY (project_id,user_id) REFERENCES caidex.projects(project_id,owner_user_id)
);
CREATE INDEX ON caidex.memories(user_id,scope_key);
CREATE TABLE caidex.memory_revisions (
    user_id uuid NOT NULL REFERENCES caidex.users,
    project_id uuid,
    scope_key text GENERATED ALWAYS AS (coalesce(project_id::text,'personal')) STORED,
    revision_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    memory_id uuid NOT NULL,
    base_revision_id uuid CHECK (base_revision_id IS DISTINCT FROM revision_id),
    supersedes_revision_id uuid CHECK (supersedes_revision_id IS DISTINCT FROM revision_id),
    author_user_id uuid NOT NULL REFERENCES caidex.users,
    content text NOT NULL CHECK (octet_length(content) <= 1048576),
    tags text[] NOT NULL DEFAULT '{}',
    kind text NOT NULL CHECK (kind IN ('user_decision','preference','file_evidence','execution_result','ai_inference')),
    valid_from timestamptz NOT NULL DEFAULT now(),
    valid_until timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (revision_id,memory_id,user_id,scope_key),
    FOREIGN KEY (memory_id,user_id,scope_key) REFERENCES caidex.memories(memory_id,user_id,scope_key),
    FOREIGN KEY (base_revision_id,memory_id,user_id,scope_key) REFERENCES caidex.memory_revisions(revision_id,memory_id,user_id,scope_key),
    FOREIGN KEY (supersedes_revision_id,memory_id,user_id,scope_key) REFERENCES caidex.memory_revisions(revision_id,memory_id,user_id,scope_key),
    CHECK (valid_until IS NULL OR valid_until > valid_from),
    UNIQUE (revision_id,user_id,scope_key),
    FOREIGN KEY (project_id,user_id) REFERENCES caidex.projects(project_id,owner_user_id)
);
CREATE INDEX ON caidex.memory_revisions(user_id,scope_key);
CREATE TABLE caidex.memory_sources (
    user_id uuid NOT NULL REFERENCES caidex.users,
    project_id uuid,
    scope_key text GENERATED ALWAYS AS (coalesce(project_id::text,'personal')) STORED,
    source_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    revision_id uuid NOT NULL,
    message_id uuid,
    source_kind text NOT NULL CHECK (source_kind IN ('chat','git','file','execution','user','external')),
    source_ref text NOT NULL CHECK (octet_length(source_ref) <= 4096),
    content_hash bytea CHECK (octet_length(content_hash) = 32),
    verification text NOT NULL DEFAULT 'unknown' CHECK (verification IN ('unknown','verified','invalid')),
    FOREIGN KEY (revision_id,user_id,scope_key) REFERENCES caidex.memory_revisions(revision_id,user_id,scope_key),
    FOREIGN KEY (message_id,user_id,scope_key) REFERENCES caidex.messages(message_id,user_id,scope_key),
    CHECK ((source_kind = 'chat') = (message_id IS NOT NULL)),
    UNIQUE (source_id,user_id,scope_key),
    FOREIGN KEY (project_id,user_id) REFERENCES caidex.projects(project_id,owner_user_id)
);
CREATE INDEX ON caidex.memory_sources(user_id,scope_key);
CREATE TABLE caidex.embedding_spaces (
    user_id uuid NOT NULL REFERENCES caidex.users,
    project_id uuid,
    scope_key text GENERATED ALWAYS AS (coalesce(project_id::text,'personal')) STORED,
    embedding_space_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    provider_ref text NOT NULL CHECK (octet_length(provider_ref) BETWEEN 1 AND 1024),
    endpoint_ref text NOT NULL CHECK (octet_length(endpoint_ref) BETWEEN 1 AND 1024),
    model_ref text NOT NULL CHECK (octet_length(model_ref) BETWEEN 1 AND 1024),
    model_revision text NOT NULL,
    preprocessing_version integer NOT NULL CHECK (preprocessing_version > 0),
    dimensions integer NOT NULL CHECK (dimensions BETWEEN 1 AND 16000),
    distance text NOT NULL CHECK (distance IN ('cosine','l2','inner_product')),
    UNIQUE (embedding_space_id,user_id,scope_key,dimensions),
    UNIQUE (user_id,scope_key,provider_ref,endpoint_ref,model_ref,model_revision,preprocessing_version,dimensions,distance),
    UNIQUE (embedding_space_id,user_id,scope_key),
    FOREIGN KEY (project_id,user_id) REFERENCES caidex.projects(project_id,owner_user_id)
);
CREATE INDEX ON caidex.embedding_spaces(user_id,scope_key);
CREATE TABLE caidex.memory_embeddings (
    user_id uuid NOT NULL REFERENCES caidex.users,
    project_id uuid,
    scope_key text GENERATED ALWAYS AS (coalesce(project_id::text,'personal')) STORED,
    embedding_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    revision_id uuid NOT NULL,
    embedding_space_id uuid NOT NULL,
    dimensions integer NOT NULL,
    embedding public.vector NOT NULL,
    state text NOT NULL DEFAULT 'ready' CHECK (state IN ('ready','stale')),
    UNIQUE (revision_id,embedding_space_id),
    FOREIGN KEY (revision_id,user_id,scope_key) REFERENCES caidex.memory_revisions(revision_id,user_id,scope_key),
    FOREIGN KEY (embedding_space_id,user_id,scope_key,dimensions) REFERENCES caidex.embedding_spaces(embedding_space_id,user_id,scope_key,dimensions),
    CHECK (public.vector_dims(embedding) = dimensions),
    UNIQUE (embedding_id,user_id,scope_key),
    FOREIGN KEY (project_id,user_id) REFERENCES caidex.projects(project_id,owner_user_id)
);
CREATE INDEX ON caidex.memory_embeddings(user_id,scope_key);
CREATE TABLE caidex.memory_jobs (
    user_id uuid NOT NULL REFERENCES caidex.users,
    project_id uuid,
    scope_key text GENERATED ALWAYS AS (coalesce(project_id::text,'personal')) STORED,
    job_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id uuid,
    base_revision_id uuid,
    kind text NOT NULL CHECK (kind IN ('consolidate','embed')),
    execution_owner_ref text NOT NULL CHECK (octet_length(execution_owner_ref) BETWEEN 1 AND 1024),
    operation_id uuid NOT NULL,
    cloud_epoch bigint NOT NULL CHECK (cloud_epoch > 0),
    settings_version bigint NOT NULL CHECK (settings_version > 0),
    state text NOT NULL DEFAULT 'waiting' CHECK (state IN ('waiting','leased','complete','cancelled','failed')),
    lease_until timestamptz,
    retry_count integer NOT NULL DEFAULT 0 CHECK (retry_count BETWEEN 0 AND 10),
    UNIQUE (user_id,operation_id),
    FOREIGN KEY (source_id,user_id,scope_key) REFERENCES caidex.memory_sources(source_id,user_id,scope_key),
    FOREIGN KEY (base_revision_id,user_id,scope_key) REFERENCES caidex.memory_revisions(revision_id,user_id,scope_key),
    UNIQUE (job_id,user_id,scope_key),
    FOREIGN KEY (project_id,user_id) REFERENCES caidex.projects(project_id,owner_user_id)
);
CREATE INDEX ON caidex.memory_jobs(user_id,scope_key);
CREATE TABLE caidex.sync_events (
    user_id uuid NOT NULL REFERENCES caidex.users,
    project_id uuid,
    scope_key text GENERATED ALWAYS AS (coalesce(project_id::text,'personal')) STORED,
    event_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    seq bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
    operation_id uuid NOT NULL,
    entity_id uuid NOT NULL,
    revision_id uuid,
    channel text NOT NULL CHECK (channel IN ('chat','memory')),
    kind text NOT NULL CHECK (kind IN ('upsert','delete','control')),
    cloud_epoch bigint NOT NULL CHECK (cloud_epoch > 0),
    UNIQUE (user_id,operation_id),
    UNIQUE (event_id,user_id,scope_key),
    FOREIGN KEY (project_id,user_id) REFERENCES caidex.projects(project_id,owner_user_id)
);
CREATE INDEX ON caidex.sync_events(user_id,scope_key);
CREATE TABLE caidex.sync_cursors (
    user_id uuid NOT NULL REFERENCES caidex.users,
    project_id uuid,
    scope_key text GENERATED ALWAYS AS (coalesce(project_id::text,'personal')) STORED,
    cursor_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id uuid NOT NULL,
    session_user_id uuid NOT NULL REFERENCES caidex.users,
    channel text NOT NULL CHECK (channel IN ('chat','memory')),
    cloud_epoch bigint NOT NULL CHECK (cloud_epoch > 0),
    acknowledged_seq bigint NOT NULL DEFAULT 0 CHECK (acknowledged_seq >= 0),
    schema_version integer NOT NULL CHECK (schema_version > 0),
    protocol_version integer NOT NULL CHECK (protocol_version > 0),
    FOREIGN KEY (session_id,session_user_id) REFERENCES caidex.auth_sessions(session_id,user_id),
    UNIQUE (session_id,scope_key,channel),
    UNIQUE (cursor_id,user_id,scope_key),
    FOREIGN KEY (project_id,user_id) REFERENCES caidex.projects(project_id,owner_user_id)
);
CREATE INDEX ON caidex.sync_cursors(user_id,scope_key);
CREATE TABLE caidex.deletion_tombstones (
    user_id uuid NOT NULL REFERENCES caidex.users,
    project_id uuid,
    scope_key text GENERATED ALWAYS AS (coalesce(project_id::text,'personal')) STORED,
    tombstone_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_id uuid NOT NULL,
    entity_kind text NOT NULL CHECK (entity_kind IN ('conversation','message','attachment','memory','source','revision')),
    deletion_revision uuid,
    seq bigint NOT NULL CHECK (seq > 0),
    cloud_epoch bigint NOT NULL CHECK (cloud_epoch > 0),
    source_hash bytea CHECK (octet_length(source_hash) = 32),
    retain_until timestamptz NOT NULL,
    UNIQUE (user_id,scope_key,entity_kind,entity_id),
    UNIQUE (tombstone_id,user_id,scope_key),
    FOREIGN KEY (project_id,user_id) REFERENCES caidex.projects(project_id,owner_user_id)
);
CREATE INDEX ON caidex.deletion_tombstones(user_id,scope_key);
CREATE TABLE caidex.attachments (
    user_id uuid NOT NULL REFERENCES caidex.users,
    project_id uuid,
    scope_key text GENERATED ALWAYS AS (coalesce(project_id::text,'personal')) STORED,
    attachment_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    message_id uuid NOT NULL,
    object_ref text NOT NULL CHECK (octet_length(object_ref) BETWEEN 1 AND 1024),
    content_hash bytea NOT NULL CHECK (octet_length(content_hash) = 32),
    size_bytes bigint NOT NULL CHECK (size_bytes BETWEEN 0 AND 10485760),
    state text NOT NULL CHECK (state IN ('pending','ready','deleted')),
    FOREIGN KEY (message_id,user_id,scope_key) REFERENCES caidex.messages(message_id,user_id,scope_key),
    UNIQUE (attachment_id,user_id,scope_key),
    FOREIGN KEY (project_id,user_id) REFERENCES caidex.projects(project_id,owner_user_id)
);
CREATE INDEX ON caidex.attachments(user_id,scope_key);
ALTER TABLE caidex.memories ADD FOREIGN KEY (current_revision_id,memory_id,user_id,scope_key)
    REFERENCES caidex.memory_revisions(revision_id,memory_id,user_id,scope_key) DEFERRABLE INITIALLY DEFERRED;
-- No public function execution, table access, or future automatic privileges.
REVOKE ALL ON ALL TABLES IN SCHEMA caidex FROM PUBLIC;
ALTER DEFAULT PRIVILEGES IN SCHEMA caidex REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
CREATE FUNCTION caidex.actor() RETURNS uuid LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog AS $$
    SELECT s.user_id FROM caidex.auth_sessions s JOIN caidex.users u USING (user_id)
    WHERE s.access_hash = public.digest(nullif(current_setting('caidex.session_token',true),''),'sha256')
      AND s.revoked_at IS NULL AND s.access_expires_at > statement_timestamp()
      AND s.refresh_expires_at > statement_timestamp() AND s.audience = 'caidex-data-v1'
      AND u.state = 'active' AND current_setting('transaction_isolation') = 'read committed'
$$;
CREATE FUNCTION caidex.bind_session(token text) RETURNS uuid LANGUAGE plpgsql SET search_path = pg_catalog AS $$
DECLARE identity uuid;
BEGIN
    IF token IS NULL OR octet_length(token) NOT BETWEEN 32 AND 1024 THEN
        RAISE EXCEPTION USING ERRCODE = '28000', MESSAGE = 'Invalid account session';
    END IF;
    PERFORM set_config('caidex.session_token',token,true);
    identity := caidex.actor();
    IF identity IS NULL THEN
        RAISE EXCEPTION USING ERRCODE = '28000', MESSAGE = 'Invalid account session';
    END IF;
    RETURN identity;
END
$$;
CREATE FUNCTION caidex.project_access(id uuid, writing boolean) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog AS $$
    SELECT EXISTS (
        SELECT 1 FROM caidex.projects p JOIN caidex.users u ON u.user_id = p.owner_user_id
        WHERE p.project_id = id AND u.state = 'active' AND (
            p.owner_user_id = caidex.actor() OR EXISTS (
                SELECT 1 FROM caidex.project_members m
                WHERE m.project_id = p.project_id AND m.user_id = caidex.actor() AND m.revoked_at IS NULL
                  AND (NOT writing OR m.role = 'writer')
            )
        )
    )
$$;
CREATE FUNCTION caidex.scope_access(owner_id uuid, project uuid, writing boolean) RETURNS boolean
LANGUAGE sql STABLE SET search_path = pg_catalog AS $$
    SELECT CASE WHEN project IS NULL THEN owner_id = caidex.actor()
                ELSE caidex.project_access(project,writing) AND EXISTS (
                    SELECT 1 FROM caidex.projects p WHERE p.project_id = project AND p.owner_user_id = owner_id
                ) END
$$;
CREATE FUNCTION caidex.memory_access(owner_id uuid, project uuid) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog AS $$
    SELECT EXISTS (SELECT 1 FROM caidex.users WHERE user_id = owner_id AND state = 'active' AND memory_enabled)
       AND EXISTS (SELECT 1 FROM caidex.users WHERE user_id = caidex.actor() AND memory_enabled)
       AND caidex.scope_access(owner_id,project,false)
$$;
RESET ROLE;
-- Guard is a non-owner role with only the reads required by these three functions.
GRANT CREATE ON SCHEMA caidex TO caidex_guard;
ALTER FUNCTION caidex.actor() OWNER TO caidex_guard;
ALTER FUNCTION caidex.project_access(uuid,boolean) OWNER TO caidex_guard;
ALTER FUNCTION caidex.memory_access(uuid,uuid) OWNER TO caidex_guard;
REVOKE CREATE ON SCHEMA caidex FROM caidex_guard;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA caidex TO caidex_app;
GRANT EXECUTE ON FUNCTION caidex.actor(),caidex.project_access(uuid,boolean),caidex.scope_access(uuid,uuid,boolean) TO caidex_guard;
SET LOCAL ROLE caidex_owner;
GRANT SELECT ON caidex.users,caidex.auth_sessions,caidex.projects,caidex.project_members TO caidex_guard;
ALTER TABLE caidex.users ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.users FORCE ROW LEVEL SECURITY;
CREATE POLICY guard_read ON caidex.users FOR SELECT TO caidex_guard USING (true);
CREATE POLICY own_read ON caidex.users FOR SELECT TO caidex_app USING (user_id = caidex.actor());
ALTER TABLE caidex.user_identities ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.user_identities FORCE ROW LEVEL SECURITY;
ALTER TABLE caidex.auth_credentials ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.auth_credentials FORCE ROW LEVEL SECURITY;
ALTER TABLE caidex.auth_sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.auth_sessions FORCE ROW LEVEL SECURITY;
CREATE POLICY guard_read ON caidex.auth_sessions FOR SELECT TO caidex_guard USING (true);
CREATE POLICY own_read ON caidex.auth_sessions FOR SELECT TO caidex_app USING (user_id = caidex.actor());
ALTER TABLE caidex.security_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.security_events FORCE ROW LEVEL SECURITY;
CREATE POLICY own_read ON caidex.security_events FOR SELECT TO caidex_app USING (user_id = caidex.actor());
ALTER TABLE caidex.projects ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.projects FORCE ROW LEVEL SECURITY;
CREATE POLICY guard_read ON caidex.projects FOR SELECT TO caidex_guard USING (true);
CREATE POLICY project_read ON caidex.projects FOR SELECT TO caidex_app USING (caidex.project_access(project_id,false));
CREATE POLICY project_insert ON caidex.projects FOR INSERT TO caidex_app WITH CHECK (owner_user_id = caidex.actor());
CREATE POLICY project_update ON caidex.projects FOR UPDATE TO caidex_app USING (owner_user_id = caidex.actor()) WITH CHECK (owner_user_id = caidex.actor());
ALTER TABLE caidex.project_members ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.project_members FORCE ROW LEVEL SECURITY;
CREATE POLICY guard_read ON caidex.project_members FOR SELECT TO caidex_guard USING (true);
CREATE POLICY member_read ON caidex.project_members FOR SELECT TO caidex_app USING (caidex.project_access(project_id,false));
CREATE POLICY member_insert ON caidex.project_members FOR INSERT TO caidex_app WITH CHECK (
    granted_by = caidex.actor() AND EXISTS (SELECT 1 FROM caidex.projects p WHERE p.project_id = project_members.project_id AND p.owner_user_id = caidex.actor()));
CREATE POLICY member_update ON caidex.project_members FOR UPDATE TO caidex_app USING (
    EXISTS (SELECT 1 FROM caidex.projects p WHERE p.project_id = project_members.project_id AND p.owner_user_id = caidex.actor())) WITH CHECK (granted_by = caidex.actor());
ALTER TABLE caidex.conversations ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.conversations FORCE ROW LEVEL SECURITY;
CREATE POLICY scoped_read ON caidex.conversations FOR SELECT TO caidex_app
    USING (caidex.scope_access(user_id,project_id,false));
CREATE POLICY scoped_insert ON caidex.conversations FOR INSERT TO caidex_app
    WITH CHECK (caidex.scope_access(user_id,project_id,true));
CREATE POLICY scoped_update ON caidex.conversations FOR UPDATE TO caidex_app
    USING (caidex.scope_access(user_id,project_id,true))
    WITH CHECK (caidex.scope_access(user_id,project_id,true));
ALTER TABLE caidex.messages ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.messages FORCE ROW LEVEL SECURITY;
CREATE POLICY scoped_read ON caidex.messages FOR SELECT TO caidex_app
    USING (caidex.scope_access(user_id,project_id,false));
CREATE POLICY scoped_insert ON caidex.messages FOR INSERT TO caidex_app
    WITH CHECK (author_user_id = caidex.actor() AND caidex.scope_access(user_id,project_id,true));
CREATE POLICY scoped_update ON caidex.messages FOR UPDATE TO caidex_app
    USING (caidex.scope_access(user_id,project_id,true))
    WITH CHECK (caidex.scope_access(user_id,project_id,true));
ALTER TABLE caidex.memories ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.memories FORCE ROW LEVEL SECURITY;
CREATE POLICY scoped_read ON caidex.memories FOR SELECT TO caidex_app
    USING (caidex.scope_access(user_id,project_id,false) AND caidex.memory_access(user_id,project_id));
CREATE POLICY scoped_insert ON caidex.memories FOR INSERT TO caidex_app
    WITH CHECK (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id));
CREATE POLICY scoped_update ON caidex.memories FOR UPDATE TO caidex_app
    USING (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id))
    WITH CHECK (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id));
ALTER TABLE caidex.memory_revisions ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.memory_revisions FORCE ROW LEVEL SECURITY;
CREATE POLICY scoped_read ON caidex.memory_revisions FOR SELECT TO caidex_app
    USING (caidex.scope_access(user_id,project_id,false) AND caidex.memory_access(user_id,project_id));
CREATE POLICY scoped_insert ON caidex.memory_revisions FOR INSERT TO caidex_app
    WITH CHECK (author_user_id = caidex.actor() AND caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id));
CREATE POLICY scoped_update ON caidex.memory_revisions FOR UPDATE TO caidex_app
    USING (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id))
    WITH CHECK (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id));
ALTER TABLE caidex.memory_sources ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.memory_sources FORCE ROW LEVEL SECURITY;
CREATE POLICY scoped_read ON caidex.memory_sources FOR SELECT TO caidex_app
    USING (caidex.scope_access(user_id,project_id,false) AND caidex.memory_access(user_id,project_id));
CREATE POLICY scoped_insert ON caidex.memory_sources FOR INSERT TO caidex_app
    WITH CHECK (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id));
CREATE POLICY scoped_update ON caidex.memory_sources FOR UPDATE TO caidex_app
    USING (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id))
    WITH CHECK (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id));
ALTER TABLE caidex.embedding_spaces ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.embedding_spaces FORCE ROW LEVEL SECURITY;
CREATE POLICY scoped_read ON caidex.embedding_spaces FOR SELECT TO caidex_app
    USING (caidex.scope_access(user_id,project_id,false) AND caidex.memory_access(user_id,project_id));
CREATE POLICY scoped_insert ON caidex.embedding_spaces FOR INSERT TO caidex_app
    WITH CHECK (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id));
CREATE POLICY scoped_update ON caidex.embedding_spaces FOR UPDATE TO caidex_app
    USING (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id))
    WITH CHECK (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id));
ALTER TABLE caidex.memory_embeddings ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.memory_embeddings FORCE ROW LEVEL SECURITY;
CREATE POLICY scoped_read ON caidex.memory_embeddings FOR SELECT TO caidex_app
    USING (caidex.scope_access(user_id,project_id,false) AND caidex.memory_access(user_id,project_id)
        AND state = 'ready' AND EXISTS (
            SELECT 1 FROM caidex.memories m JOIN caidex.memory_revisions r
                ON r.revision_id = m.current_revision_id AND r.memory_id = m.memory_id
            WHERE r.revision_id = memory_embeddings.revision_id AND m.state = 'active'
                AND r.valid_from <= statement_timestamp()
                AND (r.valid_until IS NULL OR r.valid_until > statement_timestamp())
        ));
CREATE POLICY scoped_insert ON caidex.memory_embeddings FOR INSERT TO caidex_app
    WITH CHECK (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id));
CREATE POLICY scoped_update ON caidex.memory_embeddings FOR UPDATE TO caidex_app
    USING (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id))
    WITH CHECK (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id));
ALTER TABLE caidex.memory_jobs ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.memory_jobs FORCE ROW LEVEL SECURITY;
CREATE POLICY scoped_read ON caidex.memory_jobs FOR SELECT TO caidex_app
    USING (caidex.scope_access(user_id,project_id,false) AND caidex.memory_access(user_id,project_id));
CREATE POLICY scoped_insert ON caidex.memory_jobs FOR INSERT TO caidex_app
    WITH CHECK (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id));
CREATE POLICY scoped_update ON caidex.memory_jobs FOR UPDATE TO caidex_app
    USING (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id))
    WITH CHECK (caidex.scope_access(user_id,project_id,true) AND caidex.memory_access(user_id,project_id));
ALTER TABLE caidex.sync_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.sync_events FORCE ROW LEVEL SECURITY;
CREATE POLICY scoped_read ON caidex.sync_events FOR SELECT TO caidex_app
    USING (caidex.scope_access(user_id,project_id,false) AND (channel = 'chat' OR caidex.memory_access(user_id,project_id)));
CREATE POLICY scoped_insert ON caidex.sync_events FOR INSERT TO caidex_app
    WITH CHECK (caidex.scope_access(user_id,project_id,true) AND (channel = 'chat' OR caidex.memory_access(user_id,project_id)));
CREATE POLICY scoped_update ON caidex.sync_events FOR UPDATE TO caidex_app
    USING (caidex.scope_access(user_id,project_id,true) AND (channel = 'chat' OR caidex.memory_access(user_id,project_id)))
    WITH CHECK (caidex.scope_access(user_id,project_id,true) AND (channel = 'chat' OR caidex.memory_access(user_id,project_id)));
ALTER TABLE caidex.sync_cursors ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.sync_cursors FORCE ROW LEVEL SECURITY;
CREATE POLICY scoped_read ON caidex.sync_cursors FOR SELECT TO caidex_app
    USING (session_user_id = caidex.actor() AND caidex.scope_access(user_id,project_id,false) AND (channel = 'chat' OR caidex.memory_access(user_id,project_id)));
CREATE POLICY scoped_insert ON caidex.sync_cursors FOR INSERT TO caidex_app
    WITH CHECK (session_user_id = caidex.actor() AND caidex.scope_access(user_id,project_id,false) AND (channel = 'chat' OR caidex.memory_access(user_id,project_id)));
CREATE POLICY scoped_update ON caidex.sync_cursors FOR UPDATE TO caidex_app
    USING (session_user_id = caidex.actor() AND caidex.scope_access(user_id,project_id,false) AND (channel = 'chat' OR caidex.memory_access(user_id,project_id)))
    WITH CHECK (session_user_id = caidex.actor() AND caidex.scope_access(user_id,project_id,false) AND (channel = 'chat' OR caidex.memory_access(user_id,project_id)));
ALTER TABLE caidex.deletion_tombstones ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.deletion_tombstones FORCE ROW LEVEL SECURITY;
CREATE POLICY scoped_read ON caidex.deletion_tombstones FOR SELECT TO caidex_app
    USING (caidex.scope_access(user_id,project_id,false));
CREATE POLICY scoped_insert ON caidex.deletion_tombstones FOR INSERT TO caidex_app
    WITH CHECK (caidex.scope_access(user_id,project_id,true));
CREATE POLICY scoped_update ON caidex.deletion_tombstones FOR UPDATE TO caidex_app
    USING (caidex.scope_access(user_id,project_id,true))
    WITH CHECK (caidex.scope_access(user_id,project_id,true));
ALTER TABLE caidex.attachments ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.attachments FORCE ROW LEVEL SECURITY;
CREATE POLICY scoped_read ON caidex.attachments FOR SELECT TO caidex_app
    USING (caidex.scope_access(user_id,project_id,false));
CREATE POLICY scoped_insert ON caidex.attachments FOR INSERT TO caidex_app
    WITH CHECK (caidex.scope_access(user_id,project_id,true));
CREATE POLICY scoped_update ON caidex.attachments FOR UPDATE TO caidex_app
    USING (caidex.scope_access(user_id,project_id,true))
    WITH CHECK (caidex.scope_access(user_id,project_id,true));
GRANT SELECT (user_id,state,display_name,memory_enabled,chat_sync_enabled,settings_version,cloud_epoch,scope_version,allowed_scopes,created_at)
    ON caidex.users TO caidex_app;
GRANT UPDATE (display_name) ON caidex.users TO caidex_app;
GRANT SELECT (session_id,user_id,client_id,audience,platform,installation_id,display_name,created_at,last_active_at,access_expires_at,refresh_expires_at,revoked_at)
    ON caidex.auth_sessions TO caidex_app;
GRANT SELECT ON caidex.security_events,caidex.projects,caidex.project_members TO caidex_app;
GRANT INSERT ON caidex.projects,caidex.project_members TO caidex_app;
GRANT UPDATE (title) ON caidex.projects TO caidex_app;
GRANT UPDATE (role,revoked_at,authorization_version) ON caidex.project_members TO caidex_app;
GRANT SELECT,INSERT ON caidex.conversations TO caidex_app;
GRANT UPDATE (state,version) ON caidex.conversations TO caidex_app;
GRANT SELECT,INSERT ON caidex.messages TO caidex_app;
GRANT SELECT,INSERT ON caidex.memories TO caidex_app;
GRANT UPDATE (state,current_revision_id) ON caidex.memories TO caidex_app;
GRANT SELECT,INSERT ON caidex.memory_revisions TO caidex_app;
GRANT SELECT,INSERT ON caidex.memory_sources TO caidex_app;
GRANT SELECT,INSERT ON caidex.embedding_spaces TO caidex_app;
GRANT SELECT,INSERT ON caidex.memory_embeddings TO caidex_app;
GRANT SELECT,INSERT ON caidex.memory_jobs TO caidex_app;
GRANT UPDATE (state,lease_until,retry_count) ON caidex.memory_jobs TO caidex_app;
GRANT SELECT,INSERT ON caidex.sync_events TO caidex_app;
GRANT SELECT,INSERT ON caidex.sync_cursors TO caidex_app;
GRANT UPDATE (acknowledged_seq,cloud_epoch,schema_version,protocol_version) ON caidex.sync_cursors TO caidex_app;
GRANT SELECT,INSERT ON caidex.deletion_tombstones TO caidex_app;
GRANT SELECT,INSERT ON caidex.attachments TO caidex_app;
GRANT UPDATE (state) ON caidex.attachments TO caidex_app;
GRANT USAGE ON SEQUENCE caidex.sync_events_seq_seq TO caidex_app;
INSERT INTO caidex.schema_migrations(version) VALUES (1);
COMMIT;
