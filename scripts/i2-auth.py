#!/usr/bin/env python3
"""Run real I-2 HTTP/PostgreSQL/WebAuthn tests with synthetic accounts and sandbox mail."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
IMAGE = 'pgvector/pgvector@sha256:feb68f4f15446397d8cac7f4fe48fe4586de83160d1fc48b46283312d1a33966'


def process_usage(pid):
    fields = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
    cpu = (int(fields[11]) + int(fields[12])) / os.sysconf('SC_CLK_TCK')
    status = {line.split(':', 1)[0]: line.split(':', 1)[1].strip()
              for line in Path(f'/proc/{pid}/status').read_text().splitlines() if ':' in line}
    return cpu, int(status['VmRSS'].split()[0]), int(status['VmHWM'].split()[0])


def measure_service(binary, work, env):
    # Measure only the standalone service PID, excluding compilation, test client and PostgreSQL.
    with socket.socket() as reservation:
        reservation.bind(('127.0.0.1', 0))
        port = reservation.getsockname()[1]
    base = f'http://127.0.0.1:{port}/'
    mailbox = Path(work) / 'measurement-mail'
    service_env = env | {'CAIDEX_AUTH_DB': env['CAIDEX_I2_AUTH_DB'],
                         'CAIDEX_DATA_DB': env['CAIDEX_I2_DATA_DB']}
    http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    def request(action, body, token=None):
        headers = {'Content-Type': 'application/json'}
        if token:
            headers['Authorization'] = 'Bearer ' + token
        req = urllib.request.Request(base + 'v1/auth/' + action, json.dumps(body).encode(), headers)
        with http.open(req, timeout=30) as reply:
            return json.load(reply)
    with (Path(work) / 'measurement-service.log').open('wb') as log:
        child = subprocess.Popen([binary, str(port), str(mailbox)], env=service_env,
                                 stdout=log, stderr=log)
        try:
            for _ in range(300):
                if child.poll() is not None:
                    raise RuntimeError('standalone measurement service exited during startup')
                try:
                    with http.open(base + '.well-known/oauth-authorization-server', timeout=1) as reply:
                        assert json.load(reply)['issuer'] == f'http://localhost:{port}/'
                    break
                except (urllib.error.URLError, TimeoutError):
                    time.sleep(0.1)
            else:
                raise RuntimeError('standalone measurement service startup timeout')
            cpu_before, idle_rss, _ = process_usage(child.pid)
            started = time.monotonic()
            email = 'resource-measurement@example.test'
            password = 'synthetic resource measurement password'
            assert request('register', {'email': email, 'password': password}) == {'status': 'accepted'}
            mails = [json.loads(p.read_text()) for p in mailbox.glob('*.caidex-secret')
                     if '.device-code-pepper.' not in p.name]
            token = next(m['verification_token'] for m in mails
                         if m['recipient'] == email and m['purpose'] == 'register')
            verified = request('verify', {'purpose': 'register', 'token': token})
            session = request('login', {'email': email, 'password': password})
            me = request('me', {}, session['access_token'])
            assert me['user_id'] == verified['user_id'] == session['user_id']
            cpu_after, rss, peak = process_usage(child.pid)
            elapsed = time.monotonic() - started
            assert cpu_after > cpu_before and peak >= idle_rss > 0 and rss > 0
            return {'kind': 'standalone_service_register_verify_login', 'instances': 1,
                    'elapsed_seconds': round(elapsed, 3), 'cpu_seconds': round(cpu_after - cpu_before, 3),
                    'cpu_percent_one_core': round(100 * (cpu_after - cpu_before) / elapsed, 2),
                    'idle_rss_kib': idle_rss, 'rss_kib': rss, 'peak_rss_kib': peak,
                    'rss_source': 'Linux /proc/PID/status VmRSS/VmHWM; peak includes service startup',
                    'cpu_source': 'Linux /proc/PID/stat utime+stime delta during workload',
                    'logical_cpus': os.cpu_count(),
                    'host_memory_kib': int(Path('/proc/meminfo').read_text().split('MemTotal:')[1].split()[0]),
                    'two_core_two_gib_load_test': False}
        finally:
            if child.poll() is None:
                child.send_signal(signal.SIGINT)
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--chat-sync', action='store_true', help='Run I-5 shared Chat clients against this same isolated account database')
    parser.add_argument('--memory-sync', action='store_true', help='Run I-6 shared Memory clients and project RLS failure paths')
    args = parser.parse_args()
    if args.chat_sync and args.memory_sync:
        parser.error('choose one integration fixture')
    sync = args.chat_sync or args.memory_sync
    if args.output and args.output.exists():
        parser.error('output already exists')
    env = os.environ.copy()
    for key in ['DOCKER_HOST', 'DOCKER_CONTEXT', 'DOCKER_TLS', 'DOCKER_TLS_VERIFY', 'DOCKER_CERT_PATH']:
        env.pop(key, None)
    docker = ['docker', '--host=unix:///var/run/docker.sock']
    name = 'caidex-i2-' + uuid.uuid4().hex
    started = time.monotonic()
    checks = []
    def run(command, stdin=None):
        result = subprocess.run(docker + command, input=stdin, text=True, capture_output=True, env=env, timeout=90)
        if result.returncode:
            raise RuntimeError(result.stderr)
        return result.stdout.strip()
    def sql(query):
        return run(['exec', '-i', name, 'psql', '-X', '-qAt', '-v', 'ON_ERROR_STOP=1', '-U', 'postgres'], query)
    def check(label, passed):
        if not passed:
            raise AssertionError(label)
        item = {'name': label, 'passed': True}
        checks.append(item)
        print('I2_CHECK ' + json.dumps(item), flush=True)
    with tempfile.TemporaryDirectory(prefix='caidex-i2-', dir=os.environ.get('CAIDEX_I2_TEST_TMP_PARENT', '/var/tmp')) as work:
        socket = Path(work) / 'socket'
        socket.mkdir(mode=0o777)
        socket.chmod(0o777)  # Socket parent remains 0700, not reachable by other OS users.
        try:
            run(['run', '-d', '--name', name, '--network', 'none', '--memory', '512m', '--cpus', '2',
                 '--tmpfs', '/var/lib/postgresql/data:rw,size=256m',
                 '--mount', f'type=bind,src={socket},dst=/var/run/postgresql',
                 '-e', 'POSTGRES_HOST_AUTH_METHOD=trust', IMAGE])
            for _ in range(120):
                ready = subprocess.run(docker + ['exec', name, 'sh', '-c',
                    'test "$(cat /proc/1/comm)" = postgres && pg_isready -U postgres'], capture_output=True, env=env)
                if ready.returncode == 0:
                    break
                time.sleep(0.5)
            else:
                raise RuntimeError('isolated database startup timeout')
            sql((ROOT / 'cloud/postgres/bootstrap.sql').read_text())
            sql((ROOT / 'cloud/postgres/migrations/0001_account_scope.sql').read_text())
            migration = (ROOT / 'cloud/postgres/migrations/0002_authentication.sql').read_text()
            failure = subprocess.run(docker + ['exec', '-i', name, 'psql', '-X', '-qAt', '-v', 'ON_ERROR_STOP=1', '-U', 'postgres'],
                                     input=migration.replace('INSERT INTO caidex.schema_migrations(version) VALUES (2);', 'SELECT 1/0;'),
                                     text=True, capture_output=True, env=env)
            check('v2_failed_migration_rolls_back_role_tables_and_columns', failure.returncode != 0 and sql(
                "SELECT NOT EXISTS(SELECT FROM pg_roles WHERE rolname='caidex_auth') AND to_regclass('caidex.auth_grants') IS NULL AND NOT EXISTS(SELECT FROM information_schema.columns WHERE table_schema='caidex' AND table_name='auth_sessions' AND column_name='issuer')") == 't')
            sql(migration)
            check('v2_migration_applied_once', sql('SELECT string_agg(version::text,\',\' ORDER BY version) FROM caidex.schema_migrations') == '1,2')
            chat_migration = (ROOT / 'cloud/postgres/migrations/0003_chat_sync.sql').read_text()
            failed_chat = subprocess.run(docker + ['exec', '-i', name, 'psql', '-X', '-qAt', '-v', 'ON_ERROR_STOP=1', '-U', 'postgres'],
                                         input=chat_migration.replace('INSERT INTO caidex.schema_migrations(version) VALUES(3);', 'SELECT 1/0;'),
                                         text=True, capture_output=True, env=env)
            check('v3_failed_migration_preserves_v2_schema_and_privileges', failed_chat.returncode != 0 and sql(
                "SELECT max(version)=2 AND to_regclass('caidex.chat_controls') IS NULL AND NOT EXISTS(SELECT FROM information_schema.columns WHERE table_schema='caidex' AND table_name='users' AND column_name='chat_settings_version') FROM caidex.schema_migrations") == 't')
            sql(chat_migration)
            check('v3_chat_migration_applied', sql('SELECT max(version) FROM caidex.schema_migrations') == '3')
            memory_migration = (ROOT / 'cloud/postgres/migrations/0004_memory_sync.sql').read_text()
            failed_memory = subprocess.run(docker + ['exec', '-i', name, 'psql', '-X', '-qAt', '-v', 'ON_ERROR_STOP=1', '-U', 'postgres'],
                                          input=memory_migration.replace('INSERT INTO caidex.schema_migrations(version) VALUES(4);', 'SELECT 1/0;'),
                                          text=True, capture_output=True, env=env)
            check('v4_failed_migration_preserves_v3_and_guard', failed_memory.returncode != 0 and sql(
                "SELECT max(version)=3 AND to_regclass('caidex.memory_controls') IS NULL AND NOT EXISTS(SELECT FROM information_schema.columns WHERE table_schema='caidex' AND table_name='users' AND column_name='memory_delete_pending') FROM caidex.schema_migrations") == 't')
            sql(memory_migration)
            check('v4_memory_migration_applied', sql('SELECT max(version) FROM caidex.schema_migrations') == '4')
            operations_migration = (ROOT / 'cloud/postgres/migrations/0005_operations.sql').read_text()
            failed_operations = subprocess.run(docker + ['exec', '-i', name, 'psql', '-X', '-qAt', '-v', 'ON_ERROR_STOP=1', '-U', 'postgres'],
                                                input=operations_migration.replace('INSERT INTO caidex.schema_migrations(version) VALUES(5);', 'SELECT 1/0;'),
                                                text=True, capture_output=True, env=env)
            check('v5_failed_migration_preserves_v4_and_no_mail_grants', failed_operations.returncode != 0 and sql(
                "SELECT max(version)=4 AND to_regclass('caidex.mail_tasks') IS NULL AND to_regclass('caidex.mail_suppressions') IS NULL AND to_regprocedure('caidex.read_memory_scopes()') IS NULL FROM caidex.schema_migrations") == 't')
            sql(operations_migration)
            check('v5_operations_migration_applied', sql('SELECT max(version) FROM caidex.schema_migrations') == '5')
            directmail_migration = (ROOT / 'cloud/postgres/migrations/0006_aliyun_directmail.sql').read_text()
            failed_directmail = subprocess.run(docker + ['exec', '-i', name, 'psql', '-X', '-qAt', '-v', 'ON_ERROR_STOP=1', '-U', 'postgres'],
                                               input=directmail_migration.replace('INSERT INTO caidex.schema_migrations(version) VALUES(6);', 'SELECT 1/0;'),
                                               text=True, capture_output=True, env=env)
            check('v6_failed_mail_migration_keeps_v5_and_provider_constraint', failed_directmail.returncode != 0 and sql(
                "SELECT max(version)=5 AND (SELECT pg_get_constraintdef(oid) NOT LIKE '%aliyun_directmail%' FROM pg_constraint WHERE conrelid='caidex.mail_tasks'::regclass AND conname='mail_tasks_provider_check') FROM caidex.schema_migrations") == 't')
            sql(directmail_migration)
            check('v6_mail_provider_only_migration_applied', sql("SELECT max(version)=6 AND (SELECT pg_get_constraintdef(oid) LIKE '%aliyun_directmail%' FROM pg_constraint WHERE conrelid='caidex.mail_tasks'::regclass AND conname='mail_tasks_provider_check') FROM caidex.schema_migrations") == 't')
            check('all_27_private_tables_force_rls', sql("SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='caidex' AND c.relkind='r' AND c.relrowsecurity AND c.relforcerowsecurity") == '27')
            sql('CREATE ROLE caidex_auth_test LOGIN INHERIT; GRANT caidex_auth TO caidex_auth_test; CREATE ROLE caidex_data_test LOGIN INHERIT; GRANT caidex_app TO caidex_data_test;')
            env.update({'CAIDEX_I2_AUTH_DB': f'host={socket} user=caidex_auth_test dbname=postgres',
                        'CAIDEX_I2_DATA_DB': f'host={socket} user=caidex_data_test dbname=postgres',
                        'CAIDEX_I2_ADMIN_DB': f'host={socket} user=postgres dbname=postgres',
                        'TMPDIR': work})
            if sync:
                env.update({k.replace('I2', 'I6' if args.memory_sync else 'I5'): v for k, v in list(env.items()) if k.startswith('CAIDEX_I2_') and k.endswith('_DB')})
            source_paths = sorted(ROOT.glob('cloud/auth/src/**/*.rs')) + [ROOT / p for p in
                ['cloud/auth/Cargo.toml', 'cloud/auth/src/account.js', 'cloud/core/src/lib.rs',
                 'cloud/postgres/bootstrap.sql', 'cloud/postgres/migrations/0001_account_scope.sql',
                 'cloud/postgres/migrations/0002_authentication.sql', 'cloud/postgres/migrations/0003_chat_sync.sql', 'cloud/postgres/migrations/0004_memory_sync.sql', 'cloud/postgres/migrations/0005_operations.sql', 'cloud/postgres/migrations/0006_aliyun_directmail.sql', 'cloud/core/src/memory.rs', 'cloud/core/Cargo.toml', 'cloud/auth/tests/account.rs',
                 'scripts/i2-auth.py', 'Cargo.toml', 'Cargo.lock', '.github/workflows/ci.yml']]
            if sync:
                source_paths += sorted(ROOT.glob('chat/core/src/**/*.rs')) + sorted(ROOT.glob('chat/core/src/**/*.sql')) + [ROOT / 'cloud/core/src/chat.rs', ROOT / 'chat/core/Cargo.toml', ROOT / 'scripts/i5-chat-sync.py', ROOT / 'scripts/i6-memory-sync.py']
                source_paths = [p for p in source_paths if p.is_file()]
            fingerprints = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in source_paths}
            test_command = (['cargo', 'test', '-p', 'caidex-chat', '--features', 'sync-test', '--locked',
                             '--lib', 'real_http_memory_sync_and_isolation' if args.memory_sync else 'real_http_chat_sync_and_isolation', '--', '--ignored', '--nocapture']
                            if sync else
                            ['cargo', 'test', '-p', 'caidex-auth', '--features', 'test-auth', '--locked',
                             '--test', 'account', '--', '--ignored', '--nocapture'])
            result = subprocess.run(test_command, cwd=ROOT, env=env, text=True, capture_output=True, timeout=600)
            marker = 'I6_CHECK ' if args.memory_sync else ('I5_CHECK ' if args.chat_sync else 'I2_CHECK ')
            print(result.stdout, end='', flush=True)
            print(result.stderr, end='', flush=True)
            for line in result.stdout.splitlines():
                if line.startswith(marker):
                    checks.append(json.loads(line.removeprefix(marker)))
            if result.returncode:
                raise RuntimeError('account/Chat integration test failed')
            measured = None
            if not sync:
                sql("UPDATE caidex.auth_limits SET reset_at=now()-interval '1 second',last_at=now()-interval '1 hour'")
                build = subprocess.run(['cargo', 'build', '-p', 'caidex-auth', '--features', 'test-auth',
                                        '--bin', 'caidex-auth-local', '--locked', '--message-format=json'],
                                       cwd=ROOT, env=env, text=True, capture_output=True, timeout=600)
                print(build.stderr, end='', flush=True)
                if build.returncode:
                    raise RuntimeError('standalone service build failed')
                artifacts = [json.loads(line) for line in build.stdout.splitlines()]
                binary = next(item['executable'] for item in artifacts if item.get('executable')
                              and item.get('target', {}).get('name') == 'caidex-auth-local')
                measured = measure_service(binary, work, env)
                check('standalone_service_real_http_and_measured_rss_cpu', True)
                print('I2_MEASUREMENT ' + json.dumps(measured), flush=True)
            if any(hashlib.sha256((ROOT / p).read_bytes()).hexdigest() != digest for p, digest in fingerprints.items()):
                raise RuntimeError('source changed during verification; retry with unchanged source')
            check('rust_real_memory_integration_completed' if args.memory_sync else ('rust_real_chat_integration_completed' if args.chat_sync else 'rust_real_account_integration_completed'), 'test result: ok. 1 passed' in result.stdout)
            report = {'source_head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                      'postgres_version': sql('SHOW server_version'), 'pgvector_version': sql("SELECT extversion FROM pg_extension WHERE extname='vector'"),
                      'image': IMAGE, 'elapsed_seconds': round(time.monotonic()-started, 3), 'checks': checks,
                      'source_sha256': fingerprints,
                      'measurements': [json.loads(line.removeprefix('I2_MEASUREMENT ')) for line in result.stdout.splitlines() if line.startswith('I2_MEASUREMENT ')] + ([measured] if measured else []),
                      'summary': {'passed': len(checks), 'failed': 0},
                      'limits': (['synthetic accounts verified at the trusted backend', 'isolated Chat/Memory cache instances in one Rust test process', 'committed response replaced with HTTP503 or delayed by test middleware', 'fixture rate windows reset between scenarios; production limiter unchanged', 'no native GUI/CLI/production capacity or deployment'] if sync else ['synthetic accounts', 'process-private sandbox mail', 'software Passkey authenticator', 'no native GUI/CLI/production deployment'])}
            print(json.dumps(report, ensure_ascii=False), flush=True)
            if args.output:
                args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')
        finally:
            # The image changes this private bind mount to a postgres-owned sticky directory.
            subprocess.run(docker + ['exec', '--user', '0', name, 'chmod', '0777', '/var/run/postgresql'],
                           capture_output=True, env=env, timeout=30)
            subprocess.run(docker + ['rm', '-f', name], capture_output=True, env=env, timeout=30)


if __name__ == '__main__':
    main()
