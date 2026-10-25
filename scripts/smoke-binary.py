#!/usr/bin/env python3
"""Install smoke test for a prebuilt wicked-estate archive (wicked-estate#360).

    smoke-binary.py <unpacked-archive-dir> <expected-version> [--exec-prefix "<cmd …>"]

One script for every OS (python3 is on every hosted runner; shell variants would drift).
Against the binaries in <unpacked-archive-dir> it checks:

  1. `wicked-estate --version` prints exactly `wicked-estate <version>` (the #200 contract);
  2. `wicked-estate index` on the committed polyglot fixture (scripts/smoke-fixture: Rust,
     TypeScript, Python, and VBScript from a vendored C grammar) succeeds, and `stats --json`
     reports nodes and `calls` edges — bundled SQLite, the grammars and path handling on this OS;
  3. `blast-radius` of a known symbol returns its known dependent;
  4. `wicked-estate-mcp` answers `initialize` then `tools/list` over stdin with exactly the tool
     count CLAUDE.md states ("N tools across 3 domains") — the one constant this reads.

`--exec-prefix` runs every binary through a wrapper — e.g.
`docker run --rm -i -v <dir>:<dir> -w <dir> rockylinux:8` — so the same checks prove a clean
container with no Rust toolchain can start the binary (the glibc-floor check). The fixture and
work directory are created under <unpacked-archive-dir>, so one bind mount covers both.

Exit status 0 = installable; anything else names the failed step.
"""
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
FIXTURE = HERE / "smoke-fixture"
EXE = ".exe" if os.name == "nt" else ""


def fail(step: str, detail: str) -> None:
    print(f"SMOKE FAIL [{step}]: {detail}", file=sys.stderr)
    sys.exit(1)


def expected_tool_count() -> int:
    text = (REPO / "CLAUDE.md").read_text(encoding="utf-8")
    m = re.search(r"(\d+) tools across 3 domains", text)
    if not m:
        fail("setup", "CLAUDE.md states no 'N tools across 3 domains' count")
    return int(m.group(1))


def main() -> None:
    args = sys.argv[1:]
    prefix: list[str] = []
    if "--exec-prefix" in args:
        i = args.index("--exec-prefix")
        prefix = shlex.split(args[i + 1])
        del args[i : i + 2]
    if len(args) != 2:
        fail("usage", "smoke-binary.py <unpacked-archive-dir> <expected-version> [--exec-prefix CMD]")
    bindir = Path(args[0]).resolve()
    version = args[1].strip().lstrip("v")
    estate = bindir / f"wicked-estate{EXE}"
    mcp = bindir / f"wicked-estate-mcp{EXE}"
    for b in (estate, mcp):
        if not b.is_file():
            fail("archive", f"missing {b.name} in {bindir}")
    for doc in ("LICENSE", "NOTICE", "README.md"):
        if not (bindir / doc).is_file():
            fail("archive", f"missing {doc} in {bindir}")

    work = bindir / "smoke-work"
    shutil.rmtree(work, ignore_errors=True)
    shutil.copytree(FIXTURE, work / "repo")
    home = work / "home"
    home.mkdir(parents=True)
    db = work / "graph.db"
    env = dict(os.environ)
    for k in ("WICKED_ESTATE_DB", "WICKED_STORE_URL", "WICKED_RUNTIME", "WICKED_OTEL_ENDPOINT"):
        env.pop(k, None)
    env["WICKED_HOME"] = str(home)
    env["WICKED_ESTATE_EMIT_PROGRAM"] = "wicked-bus-absent-smoke"
    env["WICKED_ESTATE_EMIT_DEADLETTER"] = str(work / "emit.ndjson")

    def run(binary: Path, argv: list[str], stdin: str | None = None) -> subprocess.CompletedProcess:
        if prefix:
            # A container does not inherit the host environment: `env` (coreutils, present in
            # every base image) hands the binary exactly what it reads.
            passed = [f"{k}={env[k]}" for k in (
                "WICKED_HOME", "WICKED_ESTATE_EMIT_PROGRAM", "WICKED_ESTATE_EMIT_DEADLETTER",
                "WICKED_ESTATE_DB", "WICKED_MEMORY_DB", "WICKED_KNOWLEDGE_DB", "WICKED_XEDGE_DB",
            ) if k in env]
            cmd = prefix + ["env"] + passed + [str(binary)] + argv
        else:
            cmd = [str(binary)] + argv
        return subprocess.run(
            cmd, input=stdin, capture_output=True, text=True, env=env, cwd=work, timeout=600
        )

    # 1. version
    out = run(estate, ["--version"])
    if out.returncode != 0 or out.stdout.strip() != f"wicked-estate {version}":
        fail("version", f"exit {out.returncode}, stdout {out.stdout!r}, stderr {out.stderr[-500:]!r}")
    print(f"ok  version: {out.stdout.strip()}")

    # 2. index + stats
    out = run(estate, ["index", str(work / "repo"), "--db", str(db)])
    if out.returncode != 0:
        fail("index", out.stderr[-2000:])
    out = run(estate, ["stats", "--json", "--db", str(db)])
    if out.returncode != 0:
        fail("stats", out.stderr[-2000:])
    stats = json.loads(out.stdout)
    # `edges_by_kind` keys are the stored (JSON) spelling of the kind: `"calls"` with its quotes.
    calls = sum(v for k, v in (stats.get("edges_by_kind") or {}).items() if k.strip('"') == "calls")
    langs = {k.strip('"') for k in (stats.get("nodes_by_kind") or {})}
    if stats.get("nodes", 0) <= 0 or calls <= 0:
        fail("stats", f"nodes={stats.get('nodes')} calls={calls}: {out.stdout[:2000]}")
    print(f"ok  index: {stats['nodes']} nodes, {calls} calls edges, kinds {sorted(langs)}")
    for ext in (".rs", ".ts", ".py", ".vbs"):
        q = run(estate, ["source", "--file", f"src/sample{ext}", "--json", "--db", str(db)])
        nodes = json.loads(q.stdout).get("nodes", []) if q.returncode == 0 else []
        if not any(str(n.get("kind", "")).lower() != "file" for n in nodes):
            fail("index", f"no symbols from src/sample{ext} (grammar not loaded?): {q.stdout[:500]} {q.stderr[-500:]}")
    print("ok  grammars: rust, typescript, python, vbscript (vendored C) all extracted")

    # 3. blast radius of a known symbol
    out = run(estate, ["blast-radius", "smoke_leaf", "--json", "--db", str(db)])
    if out.returncode != 0:
        fail("blast-radius", out.stderr[-2000:])
    doc = json.loads(out.stdout)
    names = {d.get("name") for d in doc.get("dependents", [])}
    if "smoke_caller" not in names:
        fail("blast-radius", f"smoke_caller is not a dependent of smoke_leaf: {out.stdout[:2000]}")
    print("ok  blast-radius: smoke_leaf <- smoke_caller")

    # 4. MCP over stdio
    env["WICKED_ESTATE_DB"] = str(db)
    for k, f in (("WICKED_MEMORY_DB", "memory.db"), ("WICKED_KNOWLEDGE_DB", "knowledge.db"),
                 ("WICKED_XEDGE_DB", "xedge.db")):
        env[k] = str(home / f)
    # The handshake a real MCP client performs: initialize, the initialized notification, then
    # tools/list. Stdin closes after it; the server must then exit cleanly.
    reqs = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize",
         "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                    "clientInfo": {"name": "smoke-binary", "version": version}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
    ]
    out = run(mcp, [], stdin="".join(json.dumps(r) + "\n" for r in reqs))
    if out.returncode != 0:
        fail("mcp", f"exit {out.returncode} after stdin closed; stderr {out.stderr[-1000:]!r}")
    try:
        lines = [json.loads(l) for l in out.stdout.splitlines() if l.strip()]
    except json.JSONDecodeError as e:
        fail("mcp", f"stdout is not newline-delimited JSON-RPC ({e}): {out.stdout[:1000]!r}")
    init = next((l for l in lines if l.get("id") == 1), None)
    result = (init or {}).get("result") or {}
    if not result.get("protocolVersion") or "tools" not in (result.get("capabilities") or {}):
        fail("mcp", f"initialize did not succeed with a protocol version and a tools capability: {init}")
    listed = next((l for l in lines if l.get("id") == 2), None)
    if listed is None:
        fail("mcp", f"no tools/list response; stdout {out.stdout[:1000]!r} stderr {out.stderr[-1000:]!r}")
    tools = listed.get("result", {}).get("tools", [])
    want = expected_tool_count()
    if len(tools) != want:
        fail("mcp", f"tools/list returned {len(tools)} tools, CLAUDE.md says {want}: "
                    f"{sorted(t.get('name') for t in tools)}")
    print(f"ok  mcp: initialize ({result['protocolVersion']}) + tools/list = {len(tools)} tools, exit 0")

    shutil.rmtree(work, ignore_errors=True)
    print(f"SMOKE PASS: {bindir}")


if __name__ == "__main__":
    main()
