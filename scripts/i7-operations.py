#!/usr/bin/env python3
"""I-7: real isolated PG backups, restore barriers and constrained filesystem failures."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
IMAGE = 'pgvector/pgvector@sha256:feb68f4f15446397d8cac7f4fe48fe4586de83160d1fc48b46283312d1a33966'
spec = importlib.util.spec_from_file_location('backup', ROOT / 'cloud/postgres/operations/backup.py')
ops = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ops)
A = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'
B = 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb'
PA = '11111111-1111-1111-1111-111111111111'
AM = 'aaaaaaaa-0000-0000-0000-000000000001'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('output already exists')
    started = time.monotonic()
    (ROOT / 'target/release').mkdir(exist_ok=True)
    name = 'caidex-i7-' + uuid.uuid4().hex
    env = os.environ.copy()
    for key in ('DOCKER_HOST', 'DOCKER_CONTEXT', 'DOCKER_TLS', 'DOCKER_TLS_VERIFY', 'DOCKER_CERT_PATH'):
        env.pop(key, None)
    docker = ['docker', '--host=unix:///var/run/docker.sock']
    checks = []
    def run(args, data=None):
        result = subprocess.run(docker + args, input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, timeout=90)
        if result.returncode:
            raise RuntimeError('isolated Docker operation failed: ' + result.stderr.decode()[-2000:])
        return result.stdout.decode().strip()
    def check(label, value):
        if not value:
            raise AssertionError(label)
        checks.append({'name': label, 'passed': True})
        print('I7_CHECK ' + json.dumps(checks[-1]), flush=True)
    sources = sorted(ROOT.glob('cloud/auth/src/**/*.rs')) + sorted(ROOT.glob('chat/core/src/**/*.rs')) + [ROOT / 'chat/core/tests/chat.rs', ROOT / 'chat/core/Cargo.toml'] + [ROOT / 'cloud/auth/tests/operations.rs', ROOT / 'cloud/auth/tests/directmail.rs', ROOT / 'cloud/auth/Cargo.toml', ROOT / 'cloud/core/src/lib.rs', ROOT / 'Cargo.lock', ROOT / 'cloud/postgres/operations/backup.py', ROOT / 'cloud/postgres/operations/quarantine.sql', Path(__file__).resolve(), ROOT / '.github/workflows/ci.yml'] + sorted(ROOT.glob('cloud/postgres/migrations/*.sql'))
    fingerprints = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}
    with tempfile.TemporaryDirectory(prefix='caidex-i7-', dir='/var/tmp') as work:
        directory = Path(work) / 'backups'
        socket = Path(work) / 'socket'
        data = Path(work) / 'data'
        data.mkdir(mode=0o700)
        socket.mkdir(mode=0o777)
        socket.chmod(0o777)
        try:
            run(['run', '-d', '--name', name, '--network', 'none', '--memory', '2g', '--memory-swap', '2g', '--cpus', '2', '--mount', f'type=bind,src={data},dst=/var/lib/postgresql/data', '--tmpfs', '/fault:rw,size=16m', '--mount', f'type=bind,src={ROOT / "target/debug"},dst=/artifacts/debug,readonly', '--mount', f'type=bind,src={ROOT / "target/release"},dst=/artifacts/release,readonly', '--mount', 'type=bind,src=/usr/lib/x86_64-linux-gnu,dst=/host/lib,readonly', '--mount', f'type=bind,src={socket},dst=/var/run/postgresql', '-e', 'POSTGRES_HOST_AUTH_METHOD=trust', IMAGE, '-c', 'shared_buffers=32MB', '-c', 'work_mem=2MB', '-c', 'max_connections=12'])
            for _ in range(120):
                result = subprocess.run(docker + ['exec', name, 'sh', '-c', 'test "$(cat /proc/1/comm)" = postgres && pg_isready -U postgres'], capture_output=True, env=env)
                if result.returncode == 0:
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError('database startup timeout')
            db = ops.Database('/var/run/postgresql', 'postgres', docker + ['exec', '-i', name])
            db.sql((ROOT / 'cloud/postgres/bootstrap.sql').read_text())
            for path in sorted(ROOT.glob('cloud/postgres/migrations/000[1-4]_*.sql')):
                db.sql(path.read_text())
            db.sql((ROOT / 'cloud/postgres/tests/fixture.sql').read_text())
            db.sql('CREATE ROLE caidex_auth_test LOGIN INHERIT; GRANT caidex_auth TO caidex_auth_test;')
            db.sql(f"UPDATE caidex.users SET allowed_scopes='[{{\"type\":\"personal\"}},{{\"type\":\"project\",\"project_id\":\"{PA}\"}}]',chat_sync_enabled=true; UPDATE caidex.memory_sources SET content_hash=digest('synthetic source','sha256'); UPDATE caidex.attachments SET conversation_id=message_id,name='test.txt',media_type='text/plain',bytes=convert_to('fixture','UTF8'); INSERT INTO caidex.project_members(project_id,user_id,role,granted_by) VALUES('{PA}','{B}','writer','{A}');")
            db.sql(f"DO $$ DECLARE id uuid; BEGIN FOR i IN 1..998 LOOP id:=gen_random_uuid(); INSERT INTO caidex.memories(memory_id,user_id,state,privacy) VALUES(id,'{A}','active','sync_allowed'); INSERT INTO caidex.memory_revisions(revision_id,memory_id,user_id,author_user_id,content,kind) VALUES(id,id,'{A}','{A}','load fixture','user_decision'); UPDATE caidex.memories SET current_revision_id=id WHERE memory_id=id; INSERT INTO caidex.memory_embeddings(embedding_id,revision_id,embedding_space_id,user_id,dimensions,embedding) VALUES(id,id,'{AM}','{A}',3,'[1,1,1]'); END LOOP; END $$;")
            legacy_directory=Path(work)/'legacy'
            legacy_manifest=ops.backup(db,legacy_directory)
            db.sql((ROOT/'cloud/postgres/migrations/0005_operations.sql').read_text())
            schema5_directory=Path(work)/'schema5'
            schema5_manifest=ops.backup(db,schema5_directory)
            db.sql((ROOT/'cloud/postgres/migrations/0006_aliyun_directmail.sql').read_text())
            db.sql('ANALYZE;')
            def parity(actor):
                letter='a' if actor==A else 'b'
                result=db.sql(f"BEGIN; SET LOCAL ROLE caidex_test; SELECT caidex.bind_session(repeat('{letter}',64)); WITH candidates AS(SELECT user_id AS owner,NULL::uuid AS project FROM caidex.users UNION SELECT owner_user_id,project_id FROM caidex.projects) SELECT bool_and(caidex.memory_access(owner,project)=EXISTS(SELECT 1 FROM caidex.read_memory_scopes() s WHERE s.owner_id=owner AND s.scope=coalesce(project::text,'personal'))) FROM candidates; COMMIT;")
                return result.splitlines()[-1]=='t'
            check('cached_read_scopes_match_original_owner_and_member_permissions',parity(A) and parity(B))
            for column,value in [('memory_enabled','false'),('memory_delete_pending','true'),('allowed_scopes',"'[]'::jsonb")]:
                # Each psql connection is separate; explicitly persist then restore these synthetic states.
                db.sql(f"UPDATE caidex.users SET {column}={value} WHERE user_id='{A}';")
                check('cached_scope_owner_'+column+'_denies_member',parity(B) and db.sql("BEGIN; SET LOCAL ROLE caidex_test; SELECT caidex.bind_session(repeat('b',64)); SELECT count(*)=0 FROM caidex.read_memory_scopes() WHERE owner_id='aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'; COMMIT;").splitlines()[-1]=='t')
                db.sql(f"UPDATE caidex.users SET memory_enabled=true,memory_delete_pending=false,allowed_scopes='[{{\"type\":\"personal\"}},{{\"type\":\"project\",\"project_id\":\"{PA}\"}}]' WHERE user_id='{A}';")

            check('cached_scopes_no_session_default_denies',db.sql('BEGIN; SET LOCAL ROLE caidex_test; SELECT count(*)=0 FROM caidex.read_memory_scopes(); COMMIT;')=='t')
            db.sql(f"UPDATE caidex.users SET memory_enabled=false WHERE user_id='{B}';")
            check('cached_scopes_member_own_sync_off_denies_shared_and_personal',parity(B) and db.sql("BEGIN; SET LOCAL ROLE caidex_test; SELECT caidex.bind_session(repeat('b',64)); SELECT count(*)=0 FROM caidex.read_memory_scopes(); COMMIT;").splitlines()[-1]=='t')
            db.sql(f"UPDATE caidex.users SET memory_enabled=true WHERE user_id='{B}'; UPDATE caidex.project_members SET role='reader' WHERE project_id='{PA}';")
            check('cached_scope_reader_still_reads_but_original_writer_check_refuses',parity(B) and db.sql(f"BEGIN; SET LOCAL ROLE caidex_test; SELECT caidex.bind_session(repeat('b',64)); SELECT NOT caidex.scope_access('{A}','{PA}',true); COMMIT;").splitlines()[-1]=='t')
            db.sql(f"UPDATE caidex.project_members SET revoked_at=now() WHERE project_id='{PA}';")
            check('cached_scope_revocation_rechecked_next_statement',parity(B) and db.sql(f"BEGIN; SET LOCAL ROLE caidex_test; SELECT caidex.bind_session(repeat('b',64)); SELECT count(*)=0 FROM caidex.read_memory_scopes() WHERE owner_id='{A}'; COMMIT;").splitlines()[-1]=='t')
            db.sql(f"UPDATE caidex.project_members SET revoked_at=NULL,role='writer' WHERE project_id='{PA}';")
            # Build outside the measured cgroup, then execute PG and test/service together inside it.
            def build(package, target):
                mode=['--release'] if package=='caidex-auth' else []
                result = subprocess.run(['cargo','test',*mode,'-p',package,'--features','test-auth' if package=='caidex-auth' else 'sync-test','--test',target,'--locked','--no-run','--message-format=json'], cwd=ROOT, env=env, text=True, capture_output=True, timeout=600)
                print(result.stderr,end='',flush=True)
                if result.returncode:
                    raise RuntimeError('I-7 focused test build failed')
                messages=[json.loads(line) for line in result.stdout.splitlines()]
                artifact=next(m['executable'] for m in messages if m.get('executable') and m.get('target',{}).get('name')==target)
                return '/artifacts/' + str(Path(artifact).relative_to(ROOT / 'target'))
            mail_binary=build('caidex-auth','operations')
            chat_binary=build('caidex-chat','chat')
            smtp_binary=build('caidex-auth','directmail')
            def cgroup():
                result=run(['exec',name,'sh','-c','cat /sys/fs/cgroup/cpu.max /sys/fs/cgroup/memory.max /sys/fs/cgroup/memory.current /sys/fs/cgroup/memory.peak /sys/fs/cgroup/memory.events /sys/fs/cgroup/cpu.stat']).splitlines()
                cpu=result[0].split()
                return {'time':round(time.monotonic()-started,3),'cpu_quota':int(cpu[0]),'cpu_period':int(cpu[1]),'memory_max':int(result[1]),'memory_current':int(result[2]),'memory_peak':int(result[3]),'counters':dict(line.split() for line in result[4:])}
            curve=[cgroup()]
            def execute_test(binary, args, variables):
                command=docker+['exec']
                for key,value in variables.items(): command+=['-e',f'{key}={value}']
                command += [name,'/host/lib/ld-linux-x86-64.so.2','--library-path','/host/lib',binary,*args]
                with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
                    child=subprocess.Popen(command,stdout=stdout,stderr=stderr,env=env)
                    deadline=time.monotonic()+120
                    while child.poll() is None:
                        if time.monotonic()>deadline:
                            child.kill();child.wait();raise RuntimeError('constrained I-7 test timeout')
                        curve.append(cgroup())
                        time.sleep(0.2)
                    stdout.seek(0);stderr.seek(0)
                    out=stdout.read().decode();err=stderr.read().decode()
                print(out,end='',flush=True);print(err,end='',flush=True)
                for line in out.splitlines():
                    if line.startswith('I7_CHECK '):checks.append(json.loads(line.removeprefix('I7_CHECK ')))
                    if line.startswith('DIRECTMAIL_CHECK '):checks.append(json.loads(line.removeprefix('DIRECTMAIL_CHECK ')))
                if child.returncode:
                    raise RuntimeError('constrained I-7 test failed')
                return out
            rust_output=execute_test(mail_binary,['--ignored','--nocapture'],{'CAIDEX_I7_ADMIN_DB':'host=/var/run/postgresql user=postgres dbname=postgres','CAIDEX_I7_AUTH_DB':'host=/var/run/postgresql user=caidex_auth_test dbname=postgres','CAIDEX_I7_DATA_DB':'host=/var/run/postgresql user=caidex_test dbname=postgres'})
            check('real_mail_and_resource_integration_completed','test result: ok. 1 passed' in rust_output)
            smtp_output=execute_test(smtp_binary,['--ignored','--nocapture'],{'CAIDEX_I7_ADMIN_DB':'host=/var/run/postgresql user=postgres dbname=postgres','CAIDEX_I7_AUTH_DB':'host=/var/run/postgresql user=caidex_auth_test dbname=postgres','CAIDEX_I7_DATA_DB':'host=/var/run/postgresql user=caidex_test dbname=postgres'})
            check('isolated_directmail_tls_and_account_flow_completed','test result: ok. 1 passed' in smtp_output)
            disk_output=execute_test(chat_binary,['physical_enospc_preserves_committed_cache_and_queue','--ignored','--nocapture'],{'CAIDEX_I7_DISK_ROOT':'/fault'})
            check('physical_sqlite_enospc_integration_completed','test result: ok. 1 passed' in disk_output)
            metrics=[json.loads(line.removeprefix('I7_MEASUREMENT ')) for line in rust_output.splitlines() if line.startswith('I7_MEASUREMENT ')]
            measured=cgroup();curve.append(measured)
            check('actual_combined_two_cpu_two_gib_no_swap_peak_slo_no_oom',measured['cpu_quota']==2*measured['cpu_period'] and measured['memory_max']==2*1024**3 and measured['memory_peak']<1024**3 and int(measured['counters']['oom'])==0)
            db.sql(f"INSERT INTO caidex.auth_challenges(challenge_hash,purpose,user_id,session_id,state,expires_at) VALUES(digest('synthetic old proof','sha256'),'proof','{A}','{A}','{{\"synthetic_state\":true}}',now()+interval '1 hour'); INSERT INTO caidex.refresh_history(token_hash,session_id) VALUES(digest('synthetic old refresh','sha256'),'{A}'); INSERT INTO caidex.auth_grants(grant_hash,cancel_hash,user_code_hash,kind,client_id,audience,issuer,user_id,source_session_id,status,expires_at) VALUES(digest('synthetic old grant','sha256'),digest('synthetic cancel','sha256'),digest('synthetic user code','sha256'),'device','caidex-cli','caidex-data-v1','i1-fixture-only','{A}','{A}','approved',now()+interval '1 hour');")
            manifest = ops.backup(db, directory)
            value, archive = ops.verify(manifest)
            check('real_pg_consistent_archive_private_permissions_hash', value['bytes'] > 0 and value['schema'] == ops.SCHEMA and archive.stat().st_mode & 0o077 == 0)
            # Exercise the deletion barrier through the existing security-definer with a real session.
            db.sql(f"UPDATE caidex.users SET memory_enabled=false,memory_delete_pending=true,cloud_epoch=cloud_epoch+1,settings_version=settings_version+1 WHERE user_id='{A}';")
            for _ in range(128):
                erased=db.sql("BEGIN; SET LOCAL ROLE caidex_test; SELECT caidex.bind_session(repeat('a',64)); SELECT caidex.erase_memory(NULL); COMMIT;")
                if erased.splitlines()[-1]=='t':break
            else:raise RuntimeError('bounded B cleanup did not finish')
            db.sql(f"UPDATE caidex.users SET memory_delete_pending=false WHERE user_id='{A}'; UPDATE caidex.project_members SET revoked_at=now(),authorization_version=authorization_version+1 WHERE project_id='{PA}'; UPDATE caidex.auth_credentials SET revoked_at=now() WHERE user_id='{A}'; UPDATE caidex.auth_sessions SET revoked_at=now() WHERE user_id='{A}';")
            point = Path(work) / 'latest-checkpoint.json'
            cp = ops.checkpoint(db, point)
            original_sql=db.sql
            def protected_creation(query):
                result=original_sql(query)
                if query.startswith('CREATE DATABASE caidex_restore_current '):
                    attempt=subprocess.run(docker+['exec',name,'psql','-h','/var/run/postgresql','-U','caidex_test','-d','caidex_restore_current','-c','SELECT 1'],capture_output=True,env=env)
                    check('restore_creation_blocks_connections_before_privilege_revocation',original_sql("SELECT NOT datallowconn FROM pg_database WHERE datname='caidex_restore_current'")=='t' and attempt.returncode!=0)
                return result
            db.sql=protected_creation
            restored = ops.restore(db, manifest, 'caidex_restore_current', point)
            db.sql=original_sql
            legacy_result=ops.restore(db,legacy_manifest,'caidex_restore_legacy',point)
            legacy_db=ops.Database(db.socket,'caidex_restore_legacy',db.prefix)
            check('schema4_backup_forward_migrates_to6_and_preserves_B_barrier',legacy_result['schema']==6 and legacy_db.sql(f"SELECT max(version)=6 AND NOT EXISTS(SELECT FROM caidex.memories WHERE user_id='{A}') FROM caidex.schema_migrations")=='t')
            target = ops.Database(db.socket, 'caidex_restore_current', db.prefix)
            schema5_result=ops.restore(db,schema5_manifest,'caidex_restore_schema5',point)
            schema5_db=ops.Database('/var/run/postgresql','caidex_restore_schema5',docker+['exec','-i',name])
            check('schema5_backup_forward_migrates_to6_preserves_B_and_mail_constraint',schema5_result['schema']==6 and schema5_db.sql(f"SELECT NOT EXISTS(SELECT FROM caidex.memories WHERE user_id='{A}') AND (SELECT pg_get_constraintdef(oid) LIKE '%aliyun_directmail%' FROM pg_constraint WHERE conrelid='caidex.mail_tasks'::regclass AND conname='mail_tasks_provider_check')")=='t')
            check('old_snapshot_latest_B_erases_only_owner_memory_sources_vectors_jobs', target.sql(f"SELECT NOT EXISTS(SELECT FROM caidex.memories WHERE user_id='{A}') AND EXISTS(SELECT FROM caidex.memories WHERE user_id='{B}') AND NOT EXISTS(SELECT FROM caidex.memory_revisions WHERE user_id='{A}') AND NOT EXISTS(SELECT FROM caidex.memory_sources WHERE user_id='{A}') AND NOT EXISTS(SELECT FROM caidex.memory_embeddings WHERE user_id='{A}') AND NOT EXISTS(SELECT FROM caidex.memory_jobs)") == 't')
            check('B_does_not_erase_chat_or_other_owner_attachment', target.sql(f"SELECT count(*)=3 AND bool_and(body<>'') FROM caidex.messages") == 't' and target.sql("SELECT count(*)=3 AND bool_and(bytes=convert_to('fixture','UTF8')) FROM caidex.attachments") == 't')
            check('latest_acl_revocation_and_source_tombstones_survive', target.sql(f"SELECT EXISTS(SELECT FROM caidex.project_members WHERE project_id='{PA}' AND revoked_at IS NOT NULL) AND EXISTS(SELECT FROM caidex.deletion_tombstones WHERE user_id='{A}' AND entity_kind='source' AND source_hash IS NOT NULL)") == 't')
            check('all_sessions_refresh_proofs_controls_and_epochs_invalidated', target.sql("SELECT NOT EXISTS(SELECT FROM caidex.auth_sessions WHERE revoked_at IS NULL) AND NOT EXISTS(SELECT FROM caidex.users WHERE memory_enabled OR chat_sync_enabled OR cloud_epoch<2) AND NOT EXISTS(SELECT FROM caidex.sync_cursors) AND NOT EXISTS(SELECT FROM caidex.sync_events) AND NOT EXISTS(SELECT FROM caidex.auth_challenges WHERE consumed_at IS NULL OR state<>'{}'::jsonb) AND NOT EXISTS(SELECT FROM caidex.refresh_history) AND NOT EXISTS(SELECT FROM caidex.auth_grants WHERE status<>'cancelled')") == 't')
            check('old_revoked_credentials_cannot_return', target.sql(f"SELECT bool_and(revoked_at IS NOT NULL) FROM caidex.auth_credentials WHERE user_id='{A}'") == 't')
            check('restored_roles_rls_and_no_public_connect', target.sql("SELECT NOT has_database_privilege('caidex_test',current_database(),'CONNECT') AND NOT EXISTS(SELECT FROM pg_roles WHERE rolname IN('caidex_owner','caidex_guard','caidex_app','caidex_auth') AND (rolsuper OR rolbypassrls OR rolcanlogin))") == 't' and restored['quarantined'])
            missing = ops.restore(db, manifest, 'caidex_restore_missing')
            quarantined = ops.Database(db.socket, 'caidex_restore_missing', db.prefix)
            check('missing_latest_checkpoint_keeps_data_quarantined_suspended', missing['latest_checkpoint_applied'] is False and quarantined.sql("SELECT bool_and(state='suspended' AND NOT memory_enabled AND NOT chat_sync_enabled) FROM caidex.users") == 't' and quarantined.sql('SELECT count(*)=1001 FROM caidex.memories') == 't')
            check('missing_checkpoint_revokes_credentials_preserves_unverified_acl_in_quarantine', quarantined.sql("SELECT bool_and(revoked_at IS NOT NULL) FROM caidex.auth_credentials") == 't' and quarantined.sql('SELECT count(*)=1 FROM caidex.project_members') == 't')
            rejected = False
            try:
                ops.restore(db, manifest, 'postgres', point)
            except ValueError:
                rejected = True
            check('restore_cannot_overwrite_online_database', rejected)
            rejected = False
            try:
                ops.backup(db, directory, max_bytes=1, reserve_bytes=0)
            except RuntimeError:
                rejected = True
            check('backup_budget_rejects_before_touching_old_archive', rejected and ops.verify(manifest)[0]['sha256'] == value['sha256'])
            # A deliberately corrupted separate copy must be rejected before target creation.
            corrupt_dir = Path(work) / 'corrupt'
            ops.private_directory(corrupt_dir)
            corrupt = corrupt_dir / manifest.name
            ops.write_json(corrupt, value)
            corrupt_archive = corrupt_dir / archive.name
            corrupt_archive.write_bytes(b'invalid archive')
            corrupt_archive.chmod(0o600)
            rejected = False
            try:
                ops.restore(db, corrupt, 'caidex_restore_corrupt')
            except ValueError:
                rejected = True
            check('corrupt_archive_rejected_before_restore', rejected and db.sql("SELECT count(*)=0 FROM pg_database WHERE datname='caidex_restore_corrupt'") == 't')
            before_fail=db.sql('SELECT md5(string_agg(body,\',\' ORDER BY message_id)) FROM caidex.messages')
            # Own backend transaction is terminated; PostgreSQL rolls it back, committed Chat remains.
            with tempfile.TemporaryFile() as blocked_out, tempfile.TemporaryFile() as blocked_err:
                command=db.command('psql','-X','-qAt','-v','ON_ERROR_STOP=1')
                transaction=subprocess.Popen(command,stdin=subprocess.PIPE,stdout=blocked_out,stderr=blocked_err)
                transaction.stdin.write(b"BEGIN; UPDATE caidex.messages SET body='must roll back'; SELECT pg_sleep(30); COMMIT;")
                transaction.stdin.close()
                for _ in range(100):
                    pid=db.sql("SELECT pid FROM pg_stat_activity WHERE query='SELECT pg_sleep(30);' AND state='active'")
                    if pid:break
                    time.sleep(0.02)
                else:raise RuntimeError('failure fixture did not enter uncommitted transaction')
                db.sql(f'SELECT pg_terminate_backend({int(pid)});')
                transaction.wait(timeout=10)
            check('terminated_db_worker_uncommitted_write_rolls_back',transaction.returncode!=0 and db.sql('SELECT md5(string_agg(body,\',\' ORDER BY message_id)) FROM caidex.messages')==before_fail)
            restart_started=time.monotonic()
            run(['restart',name])
            for _ in range(120):
                ready=subprocess.run(docker+['exec',name,'sh','-c','test "$(cat /proc/1/comm)" = postgres && pg_isready -U postgres'],capture_output=True,env=env)
                if ready.returncode==0:break
                time.sleep(0.1)
            else:raise RuntimeError('real database restart did not recover')
            check('persistent_pg_process_restart_keeps_committed_data_and_B_epoch',db.sql('SELECT md5(string_agg(body,\',\' ORDER BY message_id)) FROM caidex.messages')==before_fail and db.sql(f"SELECT cloud_epoch=2 AND NOT memory_enabled FROM caidex.users WHERE user_id='{A}'")=='t')
            restart_seconds=round(time.monotonic()-restart_started,3)
            count_dir=Path(work)/'count-retention'
            ops.private_directory(count_dir)
            for i in range(8):
                cloned=dict(value)
                basename=f'snapshot-{int(value["captured_at"])}-{uuid.uuid4().hex}'
                clone=count_dir/(basename+'.dump')
                clone.write_bytes(archive.read_bytes());clone.chmod(0o600)
                cloned['archive']=clone.name
                ops.write_json(count_dir/(basename+'.json'),cloned)
            note=count_dir/'operator-note.txt';note.write_text('preserve unrelated file');note.chmod(0o600)
            removed_count=ops.prune(count_dir)
            check('max_seven_registered_backups_without_deleting_unrelated_files',len(removed_count)==1 and len(list(count_dir.glob('*.json')))==7 and note.exists())
            orphan=count_dir/(f'snapshot-{int(value["captured_at"])}-{uuid.uuid4().hex}.partial')
            orphan.write_bytes(b'synthetic interrupted deleted body backup');orphan.chmod(0o600)
            ops.prune(count_dir,now=value['captured_at']+ops.MAX_AGE+1)
            check('interrupted_backup_old_sensitive_partial_expires',not orphan.exists() and note.exists())
            removed = ops.prune(directory, now=value['captured_at'] + ops.MAX_AGE + 1)
            check('seven_day_deleted_body_backup_expires', removed == [manifest.name] and not archive.exists())
            check('source_unchanged_during_verification', all(hashlib.sha256((ROOT / path).read_bytes()).hexdigest() == sha for path, sha in fingerprints.items()))
            storage={'database_bytes':int(db.sql('SELECT pg_database_size(current_database())')),'wal_bytes':int(db.sql('SELECT coalesce(sum(size),0) FROM pg_ls_waldir()')),'snapshot_bytes':value['bytes'],'backup_max_bytes':ops.MAX_BYTES,'backup_max_count':ops.MAX_COUNT,'backup_max_age_seconds':ops.MAX_AGE,'physical_fault_filesystem_bytes':16*1024**2,'production_50GB_growth_verified':False}
            report = {'storage':storage,'source_head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(), 'source_fingerprints': fingerprints, 'checks': checks, 'failure_count': 0, 'elapsed_seconds': round(time.monotonic() - started, 3), 'postgres_version': db.metadata()['postgres'], 'vector_version': db.metadata()['vector'], 'restore_with_latest': restored, 'restore_without_latest': missing, 'database_restart_seconds':restart_seconds,'measurements': metrics, 'resource_curve': curve, 'cgroup': measured, 'checkpoint_contains_body': False, 'production_deployment': False, 'real_email_delivery': False}
            args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
            print(json.dumps(report, ensure_ascii=False), flush=True)
            print('I7_RESULT ' + json.dumps({'checks': len(checks), 'failures': 0}), flush=True)
        finally:
            subprocess.run(docker + ['rm', '-f', name], capture_output=True, env=env, timeout=30)
            # Return only our private bind mounts to the caller, including stopped/OOM containers.
            cleanup = subprocess.run(docker + ['run', '--rm', '--network', 'none', '--entrypoint', 'chown',
                '--mount', f'type=bind,src={data},dst=/cleanup/data',
                '--mount', f'type=bind,src={socket},dst=/cleanup/socket', IMAGE,
                '-R', f'{os.geteuid()}:{os.getegid()}', '/cleanup/data', '/cleanup/socket'],
                capture_output=True, env=env, timeout=30)
            if cleanup.returncode:
                raise RuntimeError('isolated fixture ownership cleanup failed')
            socket.chmod(0o700)


if __name__ == '__main__':
    main()
