import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const lock = JSON.parse(readFileSync(join(root, 'upstream/codex/lock.json'), 'utf8'));
const binary = process.env.CAIDEX_CODEX_BIN || 'codex';
const directory = mkdtempSync(join(tmpdir(), 'caidex-schema-'));
const env = { CODEX_HOME: directory };
for (const key of ['PATH', 'HOME', 'SystemRoot', 'USERPROFILE', 'TEMP', 'TMP']) {
  if (process.env[key] !== undefined) env[key] = process.env[key];
}
const run = (args) => {
  const result = spawnSync(binary, args, { env, encoding: 'utf8', timeout: 30000 });
  if (result.error || result.status !== 0) {
    throw new Error(`Pinned schema command failed: ${result.error?.message || result.status}`);
  }
  return result.stdout.trim();
};
const fingerprint = (bytes) => createHash('sha256').update(bytes).digest('hex');

try {
  if (run(['--version']) !== `codex-cli ${lock.cliVersion}`) {
    throw new Error('Codex version does not match lock.json');
  }
  for (const [name, key] of [
    ['stable', 'schemaBundleSha256'],
    ['experimental', 'experimentalSchemaBundleSha256'],
  ]) {
    const output = join(directory, name);
    run(['app-server', 'generate-json-schema', '--out', output,
      ...(name === 'experimental' ? ['--experimental'] : [])]);
    const generated = readFileSync(join(output, 'codex_app_server_protocol.schemas.json'));
    const archived = readFileSync(join(root, `upstream/codex/schemas/protocol.${name}.json`));
    if (fingerprint(generated) !== lock[key] || fingerprint(archived) !== lock[key]) {
      throw new Error(`${name} schema differs from the pinned fingerprint`);
    }
    console.log(`${name} schema matches Codex ${lock.cliVersion}`);
  }
} finally {
  rmSync(directory, { recursive: true, force: true });
}
