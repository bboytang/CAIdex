-- Mail-only forward migration; no account, ACL, RLS or synchronization changes.
BEGIN;
SET LOCAL lock_timeout='5s';
SET LOCAL statement_timeout='30s';
SET LOCAL ROLE caidex_owner;
ALTER TABLE caidex.mail_tasks DROP CONSTRAINT mail_tasks_provider_check;
ALTER TABLE caidex.mail_tasks ADD CONSTRAINT mail_tasks_provider_check
 CHECK(provider IN('brevo','resend','aliyun_directmail'));
INSERT INTO caidex.schema_migrations(version) VALUES(6);
COMMIT;
