// Regenerate (or with --check, verify) every committed fixture envelope from the pinned compiler.
// CI runs --check, which is what makes the fixtures compiler-derived rather than hand-written.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { extractEvidence } from '../lib.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const fixtures = path.join(here, '..', 'fixtures');
const CASES = ['bindings', 'partial'];
const check = process.argv.includes('--check');
let stale = 0;
for (const name of CASES) {
  const dir = path.join(fixtures, name);
  const { envelope } = await extractEvidence({ tsconfig: path.join(dir, 'tsconfig.json'), root: dir, snapshot: `fixtures/${name}` });
  const json = JSON.stringify(envelope, null, 2) + '\n';
  const file = path.join(dir, 'evidence.json');
  const old = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : null;
  if (check) {
    if (old !== json) {
      stale += 1;
      process.stderr.write(`stale: fixtures/${name}/evidence.json differs from the compiler's output\n`);
    }
  } else {
    fs.writeFileSync(file, json);
  }
}
if (stale) process.exit(1);
