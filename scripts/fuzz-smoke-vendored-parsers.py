#!/usr/bin/env python3
"""#322 step 5: a short, deterministic fuzz smoke over every vendored parser and its scanner.

Each grammar is compiled by the pinned tree-sitter CLI (scripts/regen-vendored-parsers.py PIN) and
fed generated inputs: arbitrary bytes, random sequences of the grammar's own tokens (every string
in its grammar.json), mutations of the repository's fixtures for that language, and adversarial
shapes (deep nesting, NUL bytes, invalid UTF-8, a very long line, each token followed by a string
or call the file never closes). Parse
errors are expected; a crash (the CLI dies on a signal) or a parse that runs past the per-input
timeout fails the run and names the input, which is kept for reproduction.

This is the per-PR smoke (#322 decision B). It is not coverage-guided; a nightly coverage-guided
run is tracked separately.

    python3 scripts/fuzz-smoke-vendored-parsers.py [--inputs N] [--seed S] [--keep DIR]
    TREE_SITTER_CLI=path/to/tree-sitter python3 scripts/fuzz-smoke-vendored-parsers.py
"""

import argparse
import importlib.util
import json
import os
from pathlib import Path
import random
import shutil
import signal
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "crates/wicked-estate-extract/tests/fixtures"
TIMEOUT_US = 3_000_000  # per input; inputs are at most a few hundred KB
# A scanner stuck in a loop never returns to the parser, so the CLI's --timeout cannot stop it
# (nor can SIGTERM: the CLI turns it into a cancel flag the stuck loop never reads). Each batch
# therefore has a hard deadline, after which the CLI is killed and inputs are retried one by one.
BATCH_DEADLINE_S = 600
SINGLE_DEADLINE_S = 20
MAX_SEED_BYTES = 64 * 1024
# Known super-linear shapes, each with its open issue. A timeout on one is reported, not failed;
# fixing the issue deletes the entry. Anything else that times out fails the run.
KNOWN_SLOW: dict = {}


def regen():
    spec = importlib.util.spec_from_file_location("regen", ROOT / "scripts/regen-vendored-parsers.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def grammar_tokens(grammar_dir):
    """Every literal string the grammar matches: its keywords and punctuation."""
    tokens = set()

    def walk(node):
        if isinstance(node, dict):
            if node.get("type") == "STRING" and node.get("value"):
                tokens.add(node["value"])
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for value in node:
                walk(value)

    grammar = json.loads((grammar_dir / "src/grammar.json").read_text())
    walk(grammar.get("rules", {}))
    walk(grammar.get("externals", []))
    return grammar["name"], sorted(tokens)


def seed_files(grammar_dir, name):
    """The repository's sources for this language: its fixtures and the tree's own test corpus."""
    seeds = []
    for base in (FIXTURES / name, FIXTURES / name.replace("_", "-"), grammar_dir / "test", grammar_dir / "corpus",
                 grammar_dir.parent / "test", grammar_dir.parent / "corpus"):
        if base.is_dir():
            seeds += [p for p in sorted(base.rglob("*")) if p.is_file() and p.stat().st_size <= MAX_SEED_BYTES]
    return [p.read_bytes() for p in seeds]


def inputs_for(grammar_dir, count, rng):
    name, tokens = grammar_tokens(grammar_dir)
    seeds = seed_files(grammar_dir, name) or [b""]
    words = [t.encode() for t in tokens] + [b"x", b"a1", b"_", b"0", b"1.5e9", b"\"s\"", b"'s'"]
    glue = [b" ", b"\n", b"\r\n", b"\t", b"", b"  "]
    specials = [b"\x00", b"\xff", b"\xc3", b"\xe2\x80", b"\r", b"\"", b"'", b"#", b"<", b">", b"(", b")", b"{", b"}",
                b"[", b"]", b"\\", b"/*", b"*/", b"<!--", b"-->", b"&", b";", b":", b"="]
    out = []
    # Adversarial shapes, the same for every grammar.
    for unit in (b"(", b"{", b"[", b"<", b"\"", b"'", b"/*", b"<!--", b"#"):
        out.append((f"repeat:{unit.decode()}", unit * 20000))
    out.append(("long-line", b"x" * 300_000))
    out.append(("nul", b"\x00" * 4096))
    out.append(("all-bytes", bytes(range(256)) * 64))
    for token in words[:200]:
        out.append((f"token-repeat:{token[:20]!r}", token * 2000))
    # Each token, then an opener whose closer never comes: the file ends inside the string or the
    # call. A scanner that waits for the closer must still stop at the end of the input.
    for token in words[:400]:
        for opener in (b'("x', b"(x", b'"x', b"'x"):
            out.append((f"token-open-at-eof:{token[:20]!r}", token + opener))
    # Then `count` generated inputs, on top of the fixed shapes above.
    target = len(out) + count
    while len(out) < target:
        kind = rng.random()
        if kind < 0.1:
            # The file ends right after an opener: the shape that leaves a scanner looking for a
            # closer that never comes.
            seed = rng.choice(seeds)
            data = seed[: rng.randint(0, len(seed))] + b" " + rng.choice(words) + rng.choice([b"", b"\"", b"'", b"(", b"\n"])
            out.append(("opener-at-eof", data))
        elif kind < 0.2:
            out.append(("random-bytes", rng.randbytes(rng.randint(0, 4096))))
        elif kind < 0.55:
            parts = []
            for _ in range(rng.randint(1, 400)):
                parts.append(rng.choice(words) if rng.random() < 0.8 else rng.choice(specials))
                parts.append(rng.choice(glue))
            out.append(("token-sequence", b"".join(parts)))
        else:
            data = bytearray(rng.choice(seeds))
            for _ in range(rng.randint(1, 12)):
                op = rng.random()
                at = rng.randint(0, len(data))
                if op < 0.3:
                    data[at:at] = rng.choice(words + specials)
                elif op < 0.5 and data:
                    del data[at:at + rng.randint(1, 64)]
                elif op < 0.7 and data:
                    chunk = data[at:at + rng.randint(1, 256)]
                    data[at:at] = chunk * rng.randint(2, 50)
                elif op < 0.85 and data:
                    data[min(at, len(data) - 1)] = rng.randint(0, 255)
                else:
                    data[at:at] = rng.choice(seeds)[: rng.randint(0, 512)]
            out.append(("mutated-seed", bytes(data)))
    return name, out


def run(cli, grammar_dir, paths, env, deadline=BATCH_DEADLINE_S):
    """(failed, timed-out inputs). The CLI exits 1 on syntax errors; a signal (a crash) or the
    hard deadline (a scanner that never returns) is a failure."""
    listing = paths[0].parent / "paths.txt"
    listing.write_text("\n".join(str(p) for p in paths) + "\n")
    # The CLI runs in its own process group: `npx` and the package's launcher start the native
    # parser as a child, so the deadline kills the whole group, not only the launcher.
    proc = subprocess.Popen(cli + ["parse", "--json", "--timeout", str(TIMEOUT_US), "--paths", str(listing)],
                            cwd=grammar_dir, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            text=True, start_new_session=True)
    try:
        stdout, _ = proc.communicate(timeout=deadline)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        proc.communicate()
        return True, []
    if proc.returncode not in (0, 1):
        return True, []
    # Inputs with syntax errors get a summary line each, ahead of the JSON report.
    start = stdout.find('{\n  "parse_summaries"')
    summaries = json.loads(stdout[start:]).get("parse_summaries", []) if start >= 0 else []
    if len(summaries) != len(paths):
        return True, []
    return False, [s["file"] for s in summaries if not s["successful"] and s["duration"] is None]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--inputs", type=int, default=400, help="generated inputs per grammar, after the fixed shapes")
    ap.add_argument("--seed", default="wicked-estate-322")
    ap.add_argument("--keep", help="directory to keep failing inputs in")
    ap.add_argument("--only", action="append", help="limit to these parsers (vendor-relative, e.g. tree-sitter-vba)")
    args = ap.parse_args()
    r = regen()
    cli = r.cli()
    work = Path(tempfile.mkdtemp(prefix="ts-fuzz-"))
    env = dict(os.environ, TREE_SITTER_DIR=str(work / "config"), TREE_SITTER_LIBDIR=str(work / "lib"))
    failures = []
    for grammar_dir in r.grammar_dirs():
        rel = grammar_dir.relative_to(r.VENDOR)
        if args.only and str(rel) not in args.only:
            continue
        rng = random.Random(f"{args.seed}:{rel}")
        name, inputs = inputs_for(grammar_dir, args.inputs, rng)
        case = work / str(rel).replace("/", "__")
        case.mkdir(parents=True)
        paths = []
        labels = {}
        for i, (label, data) in enumerate(inputs):
            p = case / f"{i:05d}.in"
            p.write_bytes(data)
            paths.append(p)
            labels[str(p)] = label
        crashed, slow = run(cli, grammar_dir, paths, env)
        if crashed:
            # Find the inputs that crash or hang it, one at a time.
            culprits = [p for p in paths if run(cli, grammar_dir, [p], env, SINGLE_DEADLINE_S)[0]]
            failures += [f"{rel}: CRASH or HANG on {labels[str(p)]} input {p}" for p in culprits] \
                or [f"{rel}: the CLI failed on the batch"]
        known = [p for p in slow if (str(rel), labels.get(p)) in KNOWN_SLOW]
        for p in known:
            print(f"{rel}: known slow shape {labels[p]}: {KNOWN_SLOW[(str(rel), labels[p])]}", flush=True)
        failures += [f"{rel}: TIMEOUT (> {TIMEOUT_US // 1000} ms) on {labels.get(p)} input {p}" for p in slow if p not in known]
        print(f"{rel} ({name}): {len(inputs)} inputs, {'CRASH' if crashed else 'no crash'}, "
              f"{len(slow) - len(known)} new timeouts, {len(known)} known", flush=True)
    if failures:
        if args.keep:
            keep = Path(args.keep)
            keep.mkdir(parents=True, exist_ok=True)
            for line in failures:
                src = Path(line.rsplit(" ", 1)[-1])
                if src.is_file():
                    shutil.copy(src, keep / f"{line.split(':')[0].replace('/', '__')}-{src.name}")
        sys.exit("fuzz smoke failed:\n" + "\n".join(failures))
    shutil.rmtree(work, ignore_errors=True)
    print(f"fuzz smoke: {len(r.grammar_dirs())} parsers, no crash, no timeout")


if __name__ == "__main__":
    main()
