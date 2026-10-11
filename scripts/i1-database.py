#!/usr/bin/env python3
"""Rebuild and exercise I-1 in a private, networkless PostgreSQL container."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
IMAGE = 'pgvector/pgvector@sha256:feb68f4f15446397d8cac7f4fe48fe4586de83160d1fc48b46283312d1a33966'
A = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'
B = 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb'
C = 'cccccccc-cccc-cccc-cccc-cccccccccccc'
PA = '11111111-1111-1111-1111-111111111111'
PB = '22222222-2222-2222-2222-222222222222'
MA = 'aaaaaaaa-0000-0000-0000-000000000001'
MB = 'bbbbbbbb-0000-0000-0000-000000000001'
MP = 'aaaaaaaa-0000-0000-0000-000000000002'
NEW = 'dddddddd-dddd-dddd-dddd-dddddddddddd'
PRIVATE = ['conversations', 'messages', 'memories', 'memory_revisions', 'memory_sources',
           'embedding_spaces', 'memory_embeddings', 'memory_jobs', 'sync_events',
           'sync_cursors', 'deletion_tombstones', 'attachments']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, help='Write evidence to a new file; never overwrite')
    args = parser.parse_args()
    if args.output and args.output.exists():
        parser.error('output already exists')
    env = os.environ.copy()
    for key in ['DOCKER_HOST', 'DOCKER_CONTEXT', 'DOCKER_TLS', 'DOCKER_TLS_VERIFY', 'DOCKER_CERT_PATH']:
        env.pop(key, None)
    docker = ['docker', '--host=unix:///var/run/docker.sock']
    name = 'caidex-i1-' + uuid.uuid4().hex
    checks = []
    started = time.monotonic()

    def run(arguments, stdin=None):
        return subprocess.run(docker + arguments, input=stdin, text=True, capture_output=True,
                              env=env, timeout=90, check=False)

    def command(arguments):
        result = run(arguments)
        if result.returncode:
            raise RuntimeError(result.stderr)
        return result.stdout.strip()

    def sql(query, user='caidex_test', error=None, expected=None, label=None, database='postgres'):
        result = run(['exec', '-i', name, 'psql', '-X', '-qAt', '-v', 'ON_ERROR_STOP=1',
                      '-v', 'VERBOSITY=sqlstate', '-U', user, '-d', database], query)
        if error:
            if not result.returncode or not re.search(r'ERROR:\s+' + re.escape(error), result.stderr):
                raise AssertionError(f'{label}: expected SQLSTATE {error}, got {result.returncode}: {result.stderr}')
        elif result.returncode:
            raise AssertionError(f'{label}: unexpected database failure: {result.stderr}')
        elif expected is not None and result.stdout.strip() != expected:
            raise AssertionError(f'{label}: expected {expected!r}, got {result.stdout.strip()!r}')
        if label:
            record = {'name': label, 'passed': True, 'sqlstate': error,
                      'output': result.stdout.strip(), 'stderr': result.stderr.strip()}
            checks.append(record)
            print(json.dumps(record), flush=True)
        return result.stdout.strip()

    def account(query, letter='a', **kwargs):
        # Synthetic session; all identity binding stays in this same transaction.
        return sql("BEGIN; SELECT caidex.bind_session(repeat('" + letter + "',64)); " + query + '; COMMIT;', **kwargs)

    bootstrap = (ROOT / 'cloud/postgres/bootstrap.sql').read_text()
    migration = (ROOT / 'cloud/postgres/migrations/0001_account_scope.sql').read_text()
    created = False
    try:
        command(['image', 'inspect', IMAGE])
        command(['run', '-d', '--name', name, '--network', 'none', '--memory', '512m', '--cpus', '2',
                 '--tmpfs', '/var/lib/postgresql/data:rw,size=268435456',
                 '-e', 'POSTGRES_HOST_AUTH_METHOD=trust', IMAGE,
                 '-c', 'shared_buffers=32MB', '-c', 'max_connections=20'])
        created = True
        for _ in range(60):
            if (run(['exec', name, 'cat', '/proc/1/comm']).stdout.strip() == 'postgres'
                    and not run(['exec', name, 'pg_isready', '-U', 'postgres']).returncode):
                break
            time.sleep(0.25)
        else:
            raise RuntimeError('isolated PostgreSQL did not become ready')
        sql(bootstrap, user='postgres', label='bootstrap_roles_and_pinned_extensions')
        sql(migration.replace('INSERT INTO caidex.schema_migrations(version) VALUES (1);', 'SELECT 1/0;'),
            user='postgres', error='22012', label='migration_failure_rolls_back')
        sql("SELECT count(*) FROM pg_tables WHERE schemaname='caidex';", user='postgres', expected='0', label='no_partial_schema')
        sql(migration, user='postgres', label='migration_from_empty_database')
        sql(migration, user='postgres', error='42P07', label='migration_replay_refused')
        sql('SELECT version FROM caidex.schema_migrations;', user='postgres', expected='1', label='migration_version_preserved')
        sql((ROOT / 'cloud/postgres/tests/fixture.sql').read_text(), user='postgres', label='synthetic_fixture')
        sql("SELECT count(*) FROM pg_class c JOIN pg_namespace n ON c.relnamespace=n.oid WHERE n.nspname='caidex' AND c.relkind='r' AND c.relrowsecurity AND c.relforcerowsecurity;",
            expected='19', label='all_private_tables_force_rls')
        sql("SELECT rolsuper,rolbypassrls,rolcreatedb,rolcreaterole,rolreplication,pg_has_role(current_user,'caidex_owner','MEMBER'),pg_has_role(current_user,'caidex_guard','MEMBER') FROM pg_roles WHERE rolname=current_user;",
            expected='f|f|f|f|f|f|f', label='actual_runtime_login_is_unprivileged')
        sql("SELECT count(*) FROM pg_class c JOIN pg_namespace n ON c.relnamespace=n.oid WHERE n.nspname='caidex' AND c.relowner=(SELECT oid FROM pg_roles WHERE rolname=current_user);",
            expected='0', label='runtime_owns_no_tables')
        for table in ['users', 'projects'] + PRIVATE:
            fields = 'user_id' if table == 'users' else '*'
            sql(f'SELECT count({fields}) FROM caidex.{table};', expected='0', label='missing_context_' + table)
        sql(f"BEGIN; SET LOCAL caidex.user_id='{B}'; SET LOCAL caidex.session_token='forged'; SELECT count(*) FROM caidex.messages; COMMIT;",
            expected='0', label='forged_user_and_token_denied')
        sql("BEGIN; SELECT caidex.bind_session(repeat('x',64)); COMMIT;", error='28000', label='unknown_session_denied')
        sql("BEGIN; SELECT caidex.bind_session('short'); COMMIT;", error='28000', label='short_session_denied')
        sql(f"INSERT INTO caidex.projects(project_id,owner_user_id,title) VALUES('{NEW}','{A}','no-context');", error='42501', label='missing_context_write_denied')
        sql(f"BEGIN; SET LOCAL caidex.user_id='{B}'; SELECT caidex.bind_session(repeat('a',64)); SELECT user_id FROM caidex.users; COMMIT;", expected=A+'\n'+A, label='valid_session_ignores_forged_user_id')
        account('SELECT count(*) FROM caidex.security_events', expected=A+'\n1', label='security_events_own_only')
        account('SELECT user_id FROM caidex.auth_sessions', expected=A+'\n'+A, label='session_metadata_own_only')
        account(f"UPDATE caidex.auth_sessions SET access_expires_at=now()+interval '100 years' WHERE user_id='{A}'", error='42501', label='cannot_extend_session_lifetime')
        account(f"SELECT caidex.memory_access('{B}',NULL)", expected=A+'\nf', label='guard_function_does_not_disclose_other_user_state')
        account(f"SELECT caidex.memory_access('{B}','{PA}')", expected=A+'\nf', label='guard_rejects_forged_project_owner')
        sql("BEGIN ISOLATION LEVEL REPEATABLE READ; SELECT caidex.bind_session(repeat('a',64)); COMMIT;", error='28000', label='stale_snapshot_isolation_refused')
        for table in PRIVATE:
            account(f'SELECT count(*) FROM caidex.{table}', expected=A+'\n2', label='A_scope_' + table)
            account(f'SELECT count(*) FROM caidex.{table}', letter='b', expected=B+'\n1', label='B_scope_' + table)
        account("SELECT user_id FROM caidex.users", expected=A+'\n'+A, label='own_profile_only')
        account('SELECT memory_enabled,chat_sync_enabled,settings_version,cloud_epoch FROM caidex.users', letter='c',
                expected=C+'\nf|f|1|1', label='new_account_sync_defaults')
        account(f"INSERT INTO caidex.memories(memory_id,user_id,privacy) VALUES('{NEW}','{C}','sync_allowed')", letter='c', error='42501', label='disabled_cloud_memory_upload_denied')
        account(f"SELECT content FROM caidex.memory_revisions WHERE revision_id='{MB}'", expected=A, label='guessed_foreign_revision_hidden')
        account("SELECT revision_id FROM caidex.memory_embeddings ORDER BY embedding <-> '[0,0,0]'::public.vector,revision_id LIMIT 1", expected=A+'\n'+MA, label='vector_candidates_filtered_in_sql')
        account("SELECT count(*) FROM caidex.memory_revisions WHERE content LIKE 'PRIVATE_b%'", expected=A+'\n0', label='keyword_candidates_filtered_in_sql')
        for query, label in [
            ('SELECT password_hash FROM caidex.auth_credentials', 'password_material_hidden'),
            ('SELECT subject FROM caidex.user_identities', 'login_identity_hidden'),
            ('SELECT access_hash FROM caidex.auth_sessions', 'session_hash_hidden'),
            ("UPDATE caidex.users SET memory_enabled=true", 'sync_control_not_user_mutable'),
            (f"UPDATE caidex.users SET user_id='{NEW}'", 'user_id_immutable'),
            ("UPDATE caidex.messages SET body='changed'", 'messages_immutable'),
            ("UPDATE caidex.memory_revisions SET content='changed'", 'revisions_immutable'),
            ("UPDATE caidex.memory_sources SET verification='verified'", 'source_evidence_immutable'),
            ("TRUNCATE caidex.messages", 'truncate_denied'),
            ("CREATE TABLE caidex.injected(id integer)", 'schema_ddl_denied'),
            ("SET ROLE caidex_owner", 'owner_escalation_denied'),
            ("SET ROLE caidex_guard", 'guard_escalation_denied'),
            ("SET row_security=off; SELECT count(*) FROM caidex.messages", 'rls_cannot_be_disabled')]:
            account(query, error='42501', label=label)
        account(f"INSERT INTO caidex.messages(message_id,conversation_id,user_id,author_user_id,role,body,state) VALUES('{NEW}','{MA}','{A}','{B}','user','forged-author','complete')", error='42501', label='message_author_cannot_be_forged')
        account(f"INSERT INTO caidex.memory_revisions(revision_id,memory_id,user_id,author_user_id,content,kind) VALUES('{NEW}','{MA}','{A}','{B}','forged-author','user_decision')", error='42501', label='revision_author_cannot_be_forged')
        account(f"INSERT INTO caidex.memory_embeddings(embedding_id,revision_id,embedding_space_id,user_id,dimensions,embedding) VALUES('{NEW}','{MA}','{MB}','{A}',3,'[1,2,3]')", error='23503', label='foreign_embedding_space_denied')
        account(f"INSERT INTO caidex.memory_embeddings(embedding_id,revision_id,embedding_space_id,user_id,dimensions,embedding) VALUES('{NEW}','{MA}','{MA}','{A}',3,'[1,2]')", error='23514', label='embedding_dimensions_enforced')
        sql(f"UPDATE caidex.memories SET state='deleted' WHERE memory_id='{MA}';", user='postgres')
        account(f"SELECT count(*) FROM caidex.memory_embeddings WHERE revision_id='{MA}'", expected=A+'\n0', label='deleted_memory_vector_hidden')
        sql(f"UPDATE caidex.memories SET state='active' WHERE memory_id='{MA}'; UPDATE caidex.memory_embeddings SET state='stale' WHERE revision_id='{MA}';", user='postgres')
        account(f"SELECT count(*) FROM caidex.memory_embeddings WHERE revision_id='{MA}'", expected=A+'\n0', label='stale_vector_hidden')
        sql(f"UPDATE caidex.memory_embeddings SET state='ready' WHERE revision_id='{MA}'; UPDATE caidex.memory_revisions SET valid_from=now()-interval '2 days',valid_until=now()-interval '1 day' WHERE revision_id='{MA}';", user='postgres')
        account(f"SELECT count(*) FROM caidex.memory_embeddings WHERE revision_id='{MA}'", expected=A+'\n0', label='expired_revision_vector_hidden')
        sql(f"UPDATE caidex.memory_revisions SET valid_until=NULL WHERE revision_id='{MA}'; UPDATE caidex.users SET memory_enabled=false WHERE user_id='{A}';", user='postgres')
        for table in ['memories','memory_revisions','memory_sources','memory_embeddings','memory_jobs','sync_events','sync_cursors']:
            account(f"SELECT count(*) FROM caidex.{table}", expected=A+'\n0', label='disabled_memory_download_' + table)
        sql(f"UPDATE caidex.users SET memory_enabled=true WHERE user_id='{A}';", user='postgres')
        account(f"INSERT INTO caidex.messages(message_id,conversation_id,user_id,author_user_id,role,body,state) VALUES('{NEW}','{MB}','{A}','{A}','user','foreign','complete')", error='23503', label='cross_account_foreign_key_denied')
        account(f"INSERT INTO caidex.memory_sources(source_id,revision_id,message_id,user_id,source_kind,source_ref) VALUES('{NEW}','{MA}','{MP}','{A}','chat','foreign-scope')", error='23503', label='cross_project_source_denied')
        account(f"INSERT INTO caidex.memory_revisions(revision_id,memory_id,user_id,author_user_id,content,kind,base_revision_id) VALUES('{NEW}','{MP}','{A}','{A}','wrong-scope','user_decision','{MA}')", error='23503', label='revision_memory_scope_binding')
        account(f"INSERT INTO caidex.project_members(project_id,user_id,role,granted_by) VALUES('{PB}','{A}','writer','{A}')", error='42501', label='cannot_self_grant_foreign_project')
        account(f"INSERT INTO caidex.project_members(project_id,user_id,role,granted_by) VALUES('{PA}','{B}','reader','{A}')", expected=A, label='owner_grants_reader')
        account(f"SELECT count(*) FROM caidex.messages WHERE project_id='{PA}'", letter='b', expected=B+'\n1', label='reader_can_read_shared_project')
        account(f"INSERT INTO caidex.messages(message_id,conversation_id,user_id,project_id,author_user_id,role,body,state) VALUES('{NEW}','{MP}','{A}','{PA}','{B}','user','reader-write','complete')", letter='b', error='42501', label='reader_cannot_write')
        account(f"INSERT INTO caidex.sync_cursors(cursor_id,user_id,project_id,session_id,session_user_id,channel,cloud_epoch,schema_version,protocol_version) VALUES('{NEW}','{A}','{PA}','{B}','{B}','memory',1,1,1)", letter='b', expected=B, label='project_reader_cursor_binds_own_session')
        account(f"SELECT count(*) FROM caidex.sync_cursors WHERE cursor_id='{NEW}'", expected=A+'\n0', label='project_owner_cannot_read_member_cursor')
        account(f"INSERT INTO caidex.sync_cursors(user_id,project_id,session_id,session_user_id,channel,cloud_epoch,schema_version,protocol_version) VALUES('{A}','{PA}','{A}','{B}','chat',1,1,1)", letter='b', error='23503', label='cursor_cannot_reference_foreign_session')
        account(f"INSERT INTO caidex.sync_cursors(user_id,project_id,session_id,session_user_id,channel,cloud_epoch,schema_version,protocol_version) VALUES('{A}','{PA}','{A}','{A}','chat',1,1,1)", letter='b', error='42501', label='cursor_cannot_impersonate_session_user')
        account(f"UPDATE caidex.project_members SET role='writer' WHERE project_id='{PA}' AND user_id='{B}'", expected=A, label='owner_grants_writer')
        account(f"INSERT INTO caidex.messages(message_id,conversation_id,user_id,project_id,author_user_id,role,body,state) VALUES('{NEW}','{MP}','{A}','{PA}','{B}','user','writer-message','complete')", letter='b', expected=B, label='writer_keeps_project_owner_and_separate_author')
        account(f"UPDATE caidex.project_members SET revoked_at=now(),authorization_version=2 WHERE project_id='{PA}' AND user_id='{B}'", expected=A, label='owner_revokes_member')
        account(f"SELECT count(*) FROM caidex.messages WHERE project_id='{PA}'", letter='b', expected=B+'\n0', label='revoked_project_read_denied')
        account(f"UPDATE caidex.conversations SET version=2 WHERE project_id='{PA}' RETURNING conversation_id", letter='b', expected=B, label='revoked_project_write_denied')
        sql("BEGIN; SELECT caidex.bind_session(repeat('a',64)); SELECT count(*) FROM caidex.messages; COMMIT; SELECT count(*) FROM caidex.messages; BEGIN; SELECT caidex.bind_session(repeat('b',64)); SELECT count(*) FROM caidex.messages; ROLLBACK; SELECT count(*) FROM caidex.messages;",
            expected=A+'\n3\n0\n'+B+'\n1\n0', label='one_connection_commit_rollback_tenant_reset')
        sql(f"UPDATE caidex.auth_sessions SET revoked_at=now() WHERE user_id='{A}';", user='postgres')
        account('SELECT count(*) FROM caidex.messages', error='28000', label='revoked_session_bind_denied')
        sql("BEGIN; SET LOCAL caidex.session_token= '" + 'a'*64 + "'; SELECT count(*) FROM caidex.messages; COMMIT;", expected='0', label='revoked_session_direct_context_denied')
        sql(f"UPDATE caidex.auth_sessions SET access_expires_at=now()-interval '1 second' WHERE user_id='{B}';", user='postgres')
        account('SELECT count(*) FROM caidex.messages', letter='b', error='28000', label='expired_session_denied')
        sql(f"UPDATE caidex.users SET state='deleted' WHERE user_id='{C}';", user='postgres')
        account('SELECT user_id FROM caidex.users', letter='c', error='28000', label='deleted_account_denied')
        sql(f"INSERT INTO caidex.auth_credentials(user_id,kind,algorithm_version) VALUES('{A}','password','fixture');", user='postgres', error='23514', label='password_credential_requires_hash')
        sql(f"INSERT INTO caidex.conversations(conversation_id,user_id,branch_of) VALUES('{NEW}','{A}','{NEW}');", user='postgres', error='23514', label='conversation_self_branch_denied')
        sql(f"INSERT INTO caidex.memory_revisions(revision_id,memory_id,user_id,author_user_id,content,kind,base_revision_id) VALUES('{NEW}','{MA}','{A}','{A}','cycle','user_decision','{NEW}');", user='postgres', error='23514', label='revision_self_parent_denied')
        sql('CREATE DATABASE rebuild;', user='postgres')
        sql('\n'.join(line for line in bootstrap.splitlines() if not line.startswith('CREATE ROLE ')), user='postgres', database='rebuild')
        sql(migration, user='postgres', database='rebuild', label='independent_database_rebuild')
        sql("SELECT count(*) FROM pg_tables WHERE schemaname='caidex'; SELECT version FROM caidex.schema_migrations;", user='postgres', database='rebuild', expected='20\n1', label='rebuild_entities_and_version')
        versions = sql("SELECT current_setting('server_version'); SELECT extversion FROM pg_extension WHERE extname='vector';", user='postgres').splitlines()
        source = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True, capture_output=True, check=True).stdout.strip()
        paths = ['cloud/postgres/bootstrap.sql', 'cloud/postgres/migrations/0001_account_scope.sql',
                 'cloud/postgres/tests/fixture.sql', 'scripts/i1-database.py', 'cloud/core/src/lib.rs']
        result = {'head': source, 'image': IMAGE, 'postgres_version': versions[0], 'vector_version': versions[1],
                  'elapsed_seconds': round(time.monotonic()-started, 3), 'passed': len(checks), 'failed': 0,
                  'synthetic_sessions': True, 'network': 'none', 'checks': checks,
                  'source_sha256': {p: hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in paths}}
    finally:
        if created:
            command(['rm', '-f', name])  # Only the private container created by this invocation.
    if args.output:
        with args.output.open('x') as handle:
            json.dump(result, handle, indent=2)
            handle.write('\n')
    print(json.dumps({k: v for k, v in result.items() if k != 'checks'}))


if __name__ == '__main__':
    main()
