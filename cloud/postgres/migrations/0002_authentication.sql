-- I-2: apply after 0001 using the migration administrator. Never grant this role to clients/app.
BEGIN;
SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
CREATE ROLE caidex_auth NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT USAGE ON SCHEMA caidex TO caidex_auth;
SET LOCAL ROLE caidex_owner;
ALTER TABLE caidex.auth_sessions ADD COLUMN issuer text NOT NULL DEFAULT 'i1-fixture-only';
ALTER TABLE caidex.auth_credentials ADD COLUMN passkey_state jsonb;
CREATE UNIQUE INDEX live_password ON caidex.auth_credentials(user_id) WHERE kind = 'password' AND revoked_at IS NULL;
CREATE TABLE caidex.auth_challenges (
    challenge_hash bytea PRIMARY KEY CHECK (octet_length(challenge_hash) = 32),
    purpose text NOT NULL CHECK (purpose IN ('register','recover','email','proof','passkey_register','passkey_login','passkey_proof')),
    user_id uuid REFERENCES caidex.users,
    session_id uuid REFERENCES caidex.auth_sessions,
    target text NOT NULL DEFAULT '',
    state jsonb NOT NULL DEFAULT '{}',
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 5),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE caidex.refresh_history (
    token_hash bytea PRIMARY KEY CHECK (octet_length(token_hash) = 32),
    session_id uuid NOT NULL REFERENCES caidex.auth_sessions,
    consumed_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE caidex.auth_grants (
    grant_hash bytea PRIMARY KEY CHECK (octet_length(grant_hash) = 32),
    cancel_hash bytea NOT NULL UNIQUE CHECK (octet_length(cancel_hash) = 32),
    user_code_hash bytea UNIQUE CHECK (octet_length(user_code_hash) = 32),
    kind text NOT NULL CHECK (kind IN ('pkce','device')),
    client_id text NOT NULL CHECK (client_id = 'caidex-cli'),
    audience text NOT NULL CHECK (audience = 'caidex-data-v1'),
    issuer text NOT NULL,
    redirect_uri text,
    pkce_challenge text,
    login_state text,
    user_id uuid REFERENCES caidex.users,
    source_session_id uuid REFERENCES caidex.auth_sessions,
    issued_session_id uuid UNIQUE REFERENCES caidex.auth_sessions,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','denied','consumed','cancelled')),
    interval_seconds integer NOT NULL DEFAULT 5 CHECK (interval_seconds BETWEEN 5 AND 60),
    next_poll_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    FOREIGN KEY (source_session_id,user_id) REFERENCES caidex.auth_sessions(session_id,user_id),
    FOREIGN KEY (issued_session_id,user_id) REFERENCES caidex.auth_sessions(session_id,user_id),
    CHECK (status NOT IN ('approved','denied','consumed') OR (user_id IS NOT NULL AND source_session_id IS NOT NULL)),
    CHECK (status <> 'consumed' OR issued_session_id IS NOT NULL),
    CHECK ((kind = 'pkce' AND redirect_uri IS NOT NULL AND pkce_challenge IS NOT NULL AND login_state IS NOT NULL AND user_code_hash IS NULL)
        OR (kind = 'device' AND redirect_uri IS NULL AND pkce_challenge IS NULL AND login_state IS NULL AND user_code_hash IS NOT NULL))
);
CREATE TABLE caidex.auth_limits (
    bucket_hash bytea PRIMARY KEY CHECK (octet_length(bucket_hash) = 32),
    count integer NOT NULL CHECK (count > 0),
    last_at timestamptz NOT NULL DEFAULT now(),
    reset_at timestamptz NOT NULL
);
DO $$
DECLARE name text;
BEGIN
    FOREACH name IN ARRAY ARRAY['users','user_identities','auth_credentials','auth_sessions','security_events',
                               'auth_challenges','refresh_history','auth_grants','auth_limits'] LOOP
        EXECUTE format('ALTER TABLE caidex.%I ENABLE ROW LEVEL SECURITY',name);
        EXECUTE format('ALTER TABLE caidex.%I FORCE ROW LEVEL SECURITY',name);
        EXECUTE format('CREATE POLICY authentication ON caidex.%I TO caidex_auth USING (true) WITH CHECK (true)',name);
    END LOOP;
END $$;
GRANT SELECT ON caidex.users,caidex.schema_migrations TO caidex_auth;
GRANT INSERT(user_id) ON caidex.users TO caidex_auth;
GRANT SELECT,INSERT ON caidex.user_identities,caidex.auth_credentials,caidex.auth_sessions,caidex.security_events TO caidex_auth;
GRANT UPDATE(state,display_name) ON caidex.users TO caidex_auth;
GRANT UPDATE(subject,state,verified_at) ON caidex.user_identities TO caidex_auth;
GRANT UPDATE(password_hash,algorithm_version,revoked_at,passkey_state) ON caidex.auth_credentials TO caidex_auth;
GRANT UPDATE(access_hash,refresh_hash,rotation,last_active_at,access_expires_at,refresh_expires_at,revoked_at,display_name) ON caidex.auth_sessions TO caidex_auth;
GRANT SELECT,INSERT,UPDATE ON caidex.auth_challenges,caidex.auth_grants,caidex.auth_limits TO caidex_auth;
GRANT SELECT,INSERT ON caidex.refresh_history TO caidex_auth;
INSERT INTO caidex.schema_migrations(version) VALUES (2);
COMMIT;
