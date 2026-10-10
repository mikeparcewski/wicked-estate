import { test } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { toDocPath, EXIT_UNSUPPORTED } from '../lib.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const cli = path.join(here, '..', 'extract.mjs');

test('Windows paths become repository-relative with / separators', () => {
  assert.equal(toDocPath('C:\\repo\\src\\app\\a.ts', 'C:\\repo', path.win32), 'src/app/a.ts');
  assert.equal(toDocPath('C:\\other\\a.ts', 'C:\\repo', path.win32), null);
  assert.equal(toDocPath('/repo/src/a.ts', '/repo', path.posix), 'src/a.ts');
  assert.equal(toDocPath('/elsewhere/a.ts', '/repo', path.posix), null);
});

for (const [name, why] of [['unsupported', 'not supported'], ['malformed', 'malformed']]) {
  test(`an ${name} Angular version is refused with exit ${EXIT_UNSUPPORTED} and no envelope`, () => {
    const r = spawnSync(process.execPath, [cli, '--project', path.join(here, '..', 'fixtures', name, 'tsconfig.json')], { encoding: 'utf8' });
    assert.equal(r.status, EXIT_UNSUPPORTED, r.stderr);
    assert.equal(r.stdout, '');
    assert.match(r.stderr, new RegExp(why));
  });
}

test('bad arguments fail with usage before compiling anything', () => {
  const r = spawnSync(process.execPath, [cli, '--bogus', 'x'], { encoding: 'utf8' });
  assert.equal(r.status, 1);
  assert.match(r.stderr, /usage/);
});
