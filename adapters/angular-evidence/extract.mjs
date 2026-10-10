#!/usr/bin/env node
// Usage: extract.mjs --project <tsconfig.json> [--root <repo root>] [--snapshot <id>] [--out <file>]
// Writes one SemanticEvidence v1 JSON document (stdout or --out); a summary goes to stderr.
// Exit 2: unsupported or unreadable Angular version (nothing written). Exit 1: usage/other error.
import fs from 'node:fs';
import path from 'node:path';
import { extractEvidence, UnsupportedVersion, EXIT_UNSUPPORTED } from './lib.mjs';

function parse(argv) {
  const out = {};
  const known = new Set(['--project', '--root', '--snapshot', '--out']);
  for (let i = 0; i < argv.length; i += 2) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (!known.has(flag) || value === undefined || value.startsWith('--')) {
      throw new Error(`usage: extract.mjs --project <tsconfig.json> [--root <dir>] [--snapshot <id>] [--out <file>] (bad argument ${JSON.stringify(flag)})`);
    }
    out[flag.slice(2)] = value;
  }
  if (!out.project) throw new Error('usage: --project <tsconfig.json> is required');
  return out;
}

try {
  const args = parse(process.argv.slice(2));
  const root = path.resolve(args.root ?? path.dirname(args.project));
  const { envelope, stats } = await extractEvidence({ tsconfig: args.project, root, snapshot: args.snapshot });
  const json = JSON.stringify(envelope, null, 2) + '\n';
  if (args.out) fs.writeFileSync(args.out, json);
  else process.stdout.write(json);
  process.stderr.write(`angular-evidence: ${JSON.stringify(stats)}\n`);
} catch (e) {
  process.stderr.write(`angular-evidence: ${e.message}\n`);
  process.exit(e instanceof UnsupportedVersion ? EXIT_UNSUPPORTED : 1);
}
