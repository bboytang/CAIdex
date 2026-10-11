#!/usr/bin/env python3
"""Finite local PostgreSQL backups. Restore only creates a quarantined database."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import resource
import stat
import subprocess
import time
import uuid

ROOT = Path(__file__).resolve().parents[3]
SCHEMA = 5  # Updated with the supported forward migrations, never guessed from an archive.
MAX_BYTES = 5_000_000_000
MAX_COUNT = 7
MAX_AGE = 7 * 86400
RESERVE_BYTES = 256 * 1024**2


def private_directory(path):
    path = Path(path)
    if not path.is_absolute() or any(p.joinpath('.git').exists() for p in [path, *path.parents]):
        raise ValueError('backup path must be absolute and outside Git')
    path.mkdir(mode=0o700, parents=False, exist_ok=True)
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o077:
        raise ValueError('backup directory must be owner-only and not a symlink')
    # Resolve parents too: a private leaf must not redirect through a symlink or Git checkout.
    if path.resolve() != path or any(p.joinpath('.git').exists() for p in path.resolve().parents):
        raise ValueError('backup path is not canonical')
    return path


def regular(path):
    info = Path(path).lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_nlink != 1 or info.st_mode & 0o077:
        raise ValueError('backup file ownership, permissions or links refused')
    return info


def digest(path):
    regular(path)
    sha = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for block in iter(lambda: handle.read(1024**2), b''):
            sha.update(block)
    return sha.hexdigest()


def write_json(path, value):
    path = Path(path)
    if path.exists() or path.is_symlink():
        raise ValueError('output already exists')
    partial = path.with_name(path.name + '.partial')
    fd = os.open(partial, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        with os.fdopen(fd, 'w') as handle:
            json.dump(value, handle, ensure_ascii=False, indent=2)
            handle.write('\n')
            handle.flush()
            os.fsync(handle.fileno())
        partial.rename(path)
        directory_fd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    except Exception:
        partial.unlink(missing_ok=True)
        raise


class Database:
    def __init__(self, socket, database, prefix=()):
        if not str(socket).startswith('/') or not re.fullmatch(r'[a-z][a-z0-9_]{0,62}', database):
            raise ValueError('local Unix socket and simple database name required')
        self.socket, self.name, self.prefix = str(socket), database, list(prefix)

    def command(self, program, *args):
        return self.prefix + [program, '-h', self.socket, '-U', 'postgres', '-d', self.name, *args]

    def run(self, program, *args, stdin=None, stdout=None, file_limit=None):
        result = subprocess.run(self.command(program, *args), input=stdin, stdout=stdout or subprocess.PIPE,
                                stderr=subprocess.PIPE, timeout=300,
                                preexec_fn=(lambda: resource.setrlimit(resource.RLIMIT_FSIZE, (file_limit, file_limit))) if file_limit is not None else None)
        if result.returncode:
            # PG errors may contain user payload or credentials; operator sees only a safe category.
            code = re.search(rb'(?:ERROR|FATAL|PANIC):\s+([A-Z0-9]{5})', result.stderr)
            category = ' SQLSTATE ' + code[1].decode() if code else ''
            raise RuntimeError(f'{program} failed{category}; database or backup unchanged/quarantined')
        return result.stdout

    def sql(self, query):
        return self.run('psql', '-X', '-qAt', '-v', 'ON_ERROR_STOP=1', '-v', 'VERBOSITY=sqlstate', stdin=query.encode()).decode().strip()

    def metadata(self):
        return json.loads(self.sql("SELECT jsonb_build_object('schema', (SELECT max(version) FROM caidex.schema_migrations), 'postgres',current_setting('server_version'), 'vector',(SELECT extversion FROM pg_extension WHERE extname='vector'),'cluster',(SELECT system_identifier::text FROM pg_control_system()),'database',current_database(),'captured_at',extract(epoch FROM clock_timestamp()))"))


def verify(manifest):
    regular(manifest)
    if regular(manifest).st_size > 16384:
        raise ValueError('backup manifest too large')
    value = json.loads(Path(manifest).read_text())
    if value.get('format') != 'caidex-local-backup-v1' or value.get('schema') not in (4, SCHEMA):
        raise ValueError('unsupported backup manifest/schema')
    name = value.get('archive', '')
    if not re.fullmatch(r'snapshot-[0-9]+-[0-9a-f]{32}\.dump', name):
        raise ValueError('invalid archive name')
    if name != Path(manifest).with_suffix('.dump').name or value.get('protocol') != 1:
        raise ValueError('backup manifest/archive association refused')
    archive = Path(manifest).parent / name
    if regular(archive).st_size != value['bytes'] or digest(archive) != value['sha256']:
        raise ValueError('backup integrity check failed')
    return value, archive


def retained(directory, now):
    records = []
    for manifest in sorted(directory.glob('snapshot-*.json')):
        value, archive = verify(manifest)
        if value['captured_at'] > now + 60:
            raise ValueError('backup creation time is in the future')
        records.append((value['captured_at'], manifest, archive))
    return sorted(records)


def prune(directory, now=None, keep=MAX_COUNT):
    now = time.time() if now is None else now
    removed = []
    # snapshot-* is the tool's reserved namespace. Expire interrupted/orphaned files too;
    # a killed pg_dump can contain deleted bodies without having a completed manifest.
    groups = {}
    for path in directory.iterdir():
        match = re.fullmatch(r'(snapshot-([0-9]+)-[0-9a-f]{32})\.(dump|json|partial|json\.partial)', path.name)
        if match and int(match[2]) <= now-MAX_AGE:
            regular(path)
            groups.setdefault(match[1], []).append(path)
    for stem, paths in groups.items():
        for path in paths:
            path.unlink()
        removed.append(stem+'.json' if any(p.suffix=='.json' for p in paths) else paths[0].name)
    records = retained(directory, now)
    for _, manifest, archive in records[:max(0,len(records)-keep)]:
        archive.unlink()
        manifest.unlink()
        removed.append(manifest.name)
    return removed


def backup(db, directory, max_bytes=MAX_BYTES, reserve_bytes=RESERVE_BYTES):
    directory = private_directory(directory)
    lock_path = directory / 'backup.lock'
    fd = os.open(lock_path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'rb') as lock:
        regular(lock_path)
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        prune(directory)
        meta = db.metadata()
        if meta['schema'] not in (4, SCHEMA):
            raise ValueError('source schema unsupported')
        retained(directory, time.time())
        used = sum(regular(p).st_size for p in directory.iterdir())
        # pg_dump may compress, but budgeting the whole database avoids relying on a favorable ratio.
        required = int(db.sql('SELECT pg_database_size(current_database())'))
        free = os.statvfs(directory).f_bavail * os.statvfs(directory).f_frsize
        if used + required > max_bytes or free < required + reserve_bytes:
            raise RuntimeError('backup disk budget exhausted; no new backup started')
        name = f'snapshot-{int(meta["captured_at"])}-{uuid.uuid4().hex}'
        archive = directory / (name + '.dump')
        partial = directory / (name + '.partial')
        started = time.monotonic()
        try:
            handle = os.open(partial, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            with os.fdopen(handle, 'wb') as output:
                db.run('pg_dump', '--format=custom', '--schema=caidex', '--no-comments', stdout=output, file_limit=max_bytes-used-16384)
                output.flush()
                os.fsync(output.fileno())
            if regular(partial).st_size + used > max_bytes:
                raise RuntimeError('backup exceeded finite disk budget')
            if db.metadata()['schema'] != meta['schema']:
                raise RuntimeError('schema changed during backup; do not publish archive')
            partial.rename(archive)
            meta.update({'format': 'caidex-local-backup-v1', 'archive': archive.name,
                         'bytes': regular(archive).st_size, 'sha256': digest(archive),
                         'source_head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                         'protocol': 1, 'completed_at': time.time(), 'dump_seconds': round(time.monotonic() - started, 3),
                         'retention_days': 7, 'max_count': 7, 'max_bytes': MAX_BYTES,
                         'contains_deleted_data_until_expiry': True})
            manifest = directory / (name + '.json')
            write_json(manifest, meta)
            prune(directory)
            return manifest
        except Exception:
            partial.unlink(missing_ok=True)
            # Never advertise an incomplete/unregistered archive as a successful backup.
            if archive.exists() and not directory.joinpath(name + '.json').exists():
                archive.unlink()
            raise


# One read snapshot, minimal control/identity/revocation metadata; no Chat/Memory or verification secrets.
CHECKPOINT = """
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SELECT jsonb_build_object(
 'format','caidex-safety-checkpoint-v1','schema',(SELECT max(version) FROM caidex.schema_migrations),
 'cluster',(SELECT system_identifier::text FROM pg_control_system()),'database',current_database(),
 'captured_at',extract(epoch FROM transaction_timestamp()),
 'users',coalesce((SELECT jsonb_agg(jsonb_build_object('user_id',user_id,'state',state,'memory_enabled',memory_enabled,
   'chat_sync_enabled',chat_sync_enabled,'settings_version',settings_version,'chat_settings_version',chat_settings_version,
   'cloud_epoch',cloud_epoch,'scope_version',scope_version,'allowed_scopes',allowed_scopes,'memory_delete_pending',memory_delete_pending)) FROM caidex.users),'[]'),
 'identities',coalesce((SELECT jsonb_agg(to_jsonb(i)) FROM caidex.user_identities i),'[]'),
 'credentials',coalesce((SELECT jsonb_agg(jsonb_build_object('credential_id',credential_id,'fingerprint',encode(digest(to_jsonb(c)::text,'sha256'),'hex'))) FROM caidex.auth_credentials c),'[]'),
 'projects',coalesce((SELECT jsonb_agg(jsonb_build_object('project_id',project_id,'acl_version',acl_version)) FROM caidex.projects),'[]'),
 'members',coalesce((SELECT jsonb_agg(to_jsonb(m)) FROM caidex.project_members m),'[]'),
 'memories',coalesce((SELECT jsonb_agg(jsonb_build_object('memory_id',memory_id,'current_revision_id',current_revision_id,'state',state,'sync_generation',sync_generation)) FROM caidex.memories),'[]'),
 'tombstones',coalesce((SELECT jsonb_agg(to_jsonb(t)) FROM caidex.deletion_tombstones t),'[]'));
COMMIT;
"""


def checkpoint(db, output):
    output = Path(output)
    private_directory(output.parent)
    value = json.loads(db.sql(CHECKPOINT))
    if value['schema'] != SCHEMA:
        raise ValueError('checkpoint schema unsupported')
    write_json(output, value)
    return value


def restore(db, manifest, target, checkpoint_file=None):
    if not re.fullmatch(r'caidex_restore_[a-z0-9_]{1,40}', target):
        raise ValueError('new caidex_restore_* quarantine database required')
    private_directory(Path(manifest).parent)
    meta, archive = verify(Path(manifest))
    latest = None
    if checkpoint_file:
        checkpoint_file = Path(checkpoint_file)
        private_directory(checkpoint_file.parent)
        if regular(checkpoint_file).st_size > 64*1024**2:
            raise ValueError('checkpoint exceeds finite metadata limit')
        latest = json.loads(checkpoint_file.read_text())
        if (latest.get('format') != 'caidex-safety-checkpoint-v1' or latest['schema'] != SCHEMA
                or latest['cluster'] != meta['cluster'] or latest['database'] != meta['database']
                or latest['captured_at'] < meta['completed_at']):
            raise ValueError('checkpoint is unsupported, unrelated or older than completed backup')
    roles = db.sql("SELECT count(*)=4 AND bool_and(NOT(rolsuper OR rolbypassrls OR rolcanlogin OR rolcreatedb OR rolcreaterole)) FROM pg_roles WHERE rolname IN('caidex_owner','caidex_guard','caidex_app','caidex_auth')")
    if roles != 't':
        raise ValueError('dedicated non-login non-owner runtime roles required')
    # No --clean, no drop/overwrite, no connection to the source data after creation.
    db.sql(f'CREATE DATABASE {target} TEMPLATE template0;')
    # Defaults grant CONNECT to PUBLIC. Revoke before any data is restored; no app can attach.
    db.sql(f'REVOKE ALL ON DATABASE {target} FROM PUBLIC, caidex_app, caidex_auth;')
    isolated = Database(db.socket, target, db.prefix)
    started = time.monotonic()
    try:
        isolated.sql("REVOKE CREATE ON SCHEMA public FROM PUBLIC; CREATE EXTENSION vector VERSION '0.8.2'; CREATE EXTENSION pgcrypto;")
        with archive.open('rb') as handle:
            result = subprocess.run(isolated.command('pg_restore', '--single-transaction', '--exit-on-error'),
                                    stdin=handle, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=300)
        if result.returncode:
            raise RuntimeError('archive restore failed; target remains quarantined')
        if meta['schema']==4:
            isolated.sql((ROOT / 'cloud/postgres/migrations/0005_operations.sql').read_text())
        sql = (ROOT / 'cloud/postgres/operations/quarantine.sql').read_text()
        payload = json.dumps(latest or {}, ensure_ascii=True).replace("'", "''")
        isolated.sql("BEGIN; CREATE TEMP TABLE safety(value jsonb); INSERT INTO safety VALUES ('" + payload + "');\n" + sql + '\nCOMMIT;')
        actual = isolated.metadata()
        if actual['schema'] != SCHEMA:
            raise RuntimeError('restored schema refused; target remains quarantined')
        rls = isolated.sql("SELECT count(*) FILTER(WHERE relrowsecurity AND relforcerowsecurity)=count(*)-1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='caidex' AND c.relkind='r'")
        # schema_migrations is intentionally public metadata; every private table must force RLS.
        if rls != 't':
            raise RuntimeError('restored RLS refused; target remains quarantined')
        return {'database': target, 'quarantined': True, 'automatic_publication': False,
                'backup_point': meta['captured_at'], 'backup_window':[meta['captured_at'],meta['completed_at']], 'checkpoint_point': latest['captured_at'] if latest else None,
                'later_operations_known': False, 'latest_checkpoint_applied': latest is not None,
                'old_sessions_valid': False, 'sync_enabled': False,
                'restore_seconds': round(time.monotonic() - started, 3), 'schema': SCHEMA}
    except Exception:
        # The admin can inspect a failed database. No failure path releases CONNECT or source traffic.
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--socket', required=True, type=Path)
    parser.add_argument('--database', required=True)
    commands = parser.add_subparsers(dest='action', required=True)
    commands.add_parser('backup').add_argument('directory', type=Path)
    commands.add_parser('checkpoint').add_argument('output', type=Path)
    prune_args = commands.add_parser('prune')
    prune_args.add_argument('directory', type=Path)
    restore_args = commands.add_parser('restore')
    restore_args.add_argument('manifest', type=Path)
    restore_args.add_argument('--target', required=True)
    restore_args.add_argument('--checkpoint', type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    db = Database(args.socket, args.database)
    try:
        if args.action == 'backup':
            result = {'manifest': str(backup(db, args.directory))}
        elif args.action == 'checkpoint':
            value = checkpoint(db, args.output)
            result = {'checkpoint': str(args.output), 'captured_at': value['captured_at']}
        elif args.action == 'prune':
            result = {'removed': prune(private_directory(args.directory))}
        else:
            result = restore(db, args.manifest, args.target, args.checkpoint)
        print(json.dumps(result))
    except Exception as error:
        print(json.dumps({'error': str(error), 'publication_allowed': False}))
        raise SystemExit(1) from None


if __name__ == '__main__':
    main()
