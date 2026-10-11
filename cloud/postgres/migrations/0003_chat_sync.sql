BEGIN;
SET LOCAL ROLE caidex_owner;
ALTER TABLE caidex.users ADD COLUMN chat_settings_version bigint NOT NULL DEFAULT 1 CHECK(chat_settings_version>0);
ALTER TABLE caidex.conversations ADD COLUMN title text NOT NULL DEFAULT '' CHECK(octet_length(title)<=1024);
ALTER TABLE caidex.messages ADD COLUMN ending text NOT NULL DEFAULT 'complete' CHECK(ending IN ('complete','interrupted','incomplete','failed','cancelled','truncated','invalid'));
ALTER TABLE caidex.messages ADD COLUMN created_at_millis bigint NOT NULL DEFAULT 0 CHECK(created_at_millis>=0);
ALTER TABLE caidex.messages ADD COLUMN attachment_ids uuid[] NOT NULL DEFAULT '{}' CHECK(cardinality(attachment_ids)<=4);
-- Attachments can be queued before their first immutable message, as in the local cache.
ALTER TABLE caidex.attachments ALTER COLUMN message_id DROP NOT NULL;
ALTER TABLE caidex.attachments ADD COLUMN conversation_id uuid;
ALTER TABLE caidex.attachments ADD FOREIGN KEY(conversation_id,user_id,scope_key) REFERENCES caidex.conversations(conversation_id,user_id,scope_key);
ALTER TABLE caidex.attachments ADD COLUMN name text CHECK(octet_length(name) BETWEEN 1 AND 256);
ALTER TABLE caidex.attachments ADD COLUMN media_type text CHECK(media_type IN ('image/png','image/jpeg','application/pdf','text/plain'));
ALTER TABLE caidex.attachments ADD COLUMN bytes bytea CHECK(octet_length(bytes) BETWEEN 1 AND 8388608);
ALTER TABLE caidex.sync_events ADD COLUMN conversation_id uuid;
ALTER TABLE caidex.sync_events ADD COLUMN entity_kind text CHECK(entity_kind IN ('conversation','attachment','message','delete'));
ALTER TABLE caidex.sync_events ADD COLUMN operation_hash bytea CHECK(octet_length(operation_hash)=32);
CREATE TABLE caidex.chat_controls (
    user_id uuid NOT NULL REFERENCES caidex.users, operation_id uuid NOT NULL,
    operation_hash bytea NOT NULL CHECK(octet_length(operation_hash)=32),
    enabled boolean NOT NULL, version bigint NOT NULL CHECK(version>0),
    PRIMARY KEY(user_id,operation_id)
);
ALTER TABLE caidex.chat_controls ENABLE ROW LEVEL SECURITY;
ALTER TABLE caidex.chat_controls FORCE ROW LEVEL SECURITY;
CREATE POLICY own_control ON caidex.chat_controls TO caidex_app USING(user_id=caidex.actor()) WITH CHECK(user_id=caidex.actor());
CREATE POLICY authentication ON caidex.chat_controls TO caidex_auth USING(true) WITH CHECK(true);
GRANT SELECT,INSERT ON caidex.chat_controls TO caidex_auth;
GRANT UPDATE(chat_sync_enabled,chat_settings_version) ON caidex.users TO caidex_auth;
GRANT SELECT(chat_settings_version) ON caidex.users TO caidex_app;
GRANT UPDATE(title) ON caidex.conversations TO caidex_app;
-- Erasure is allowed only within the same live session and writable scope. No privileged role.
CREATE POLICY scoped_delete ON caidex.attachments FOR DELETE TO caidex_app USING(caidex.scope_access(user_id,project_id,true));
GRANT DELETE ON caidex.attachments TO caidex_app;
-- Keep immutable message identities referenced by Memory, erase only their Chat payload.
-- The guard is non-owner/NOBYPASSRLS; this function cannot edit any active conversation.
CREATE POLICY guard_chat_erasure ON caidex.conversations FOR SELECT TO caidex_guard
    USING(caidex.scope_access(user_id,project_id,true));
CREATE POLICY guard_chat_erasure ON caidex.messages TO caidex_guard
    USING(caidex.scope_access(user_id,project_id,true) AND EXISTS(
        SELECT 1 FROM caidex.conversations c WHERE c.conversation_id=messages.conversation_id AND c.state='deleted'))
    WITH CHECK(caidex.scope_access(user_id,project_id,true) AND EXISTS(
        SELECT 1 FROM caidex.conversations c WHERE c.conversation_id=messages.conversation_id AND c.state='deleted'));
GRANT SELECT ON caidex.conversations,caidex.messages TO caidex_guard;
GRANT UPDATE(body,attachment_ids) ON caidex.messages TO caidex_guard;
CREATE FUNCTION caidex.erase_chat_body(id uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM caidex.conversations c WHERE c.conversation_id=id AND c.user_id=caidex.actor() AND c.state='deleted' AND caidex.scope_access(c.user_id,c.project_id,true)) THEN
        RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='Chat erasure refused';
    END IF;
    UPDATE caidex.messages SET body='',attachment_ids='{}' WHERE conversation_id=id;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA caidex TO caidex_guard;
ALTER FUNCTION caidex.erase_chat_body(uuid) OWNER TO caidex_guard;
REVOKE CREATE ON SCHEMA caidex FROM caidex_guard;
REVOKE ALL ON FUNCTION caidex.erase_chat_body(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION caidex.erase_chat_body(uuid) TO caidex_app;
INSERT INTO caidex.schema_migrations(version) VALUES(3);
COMMIT;
