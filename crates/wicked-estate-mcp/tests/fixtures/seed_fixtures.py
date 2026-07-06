#!/usr/bin/env python3
"""
Create v0.12.x fixture DBs for wicked-estate tests/fixtures/ and run Stage 2 response goldens.

Steps:
1. Index a tiny Rust fixture file → estate.db (using wicked-estate CLI)
2. Seed memory.db via wicked-memory-mcp MCP
3. Seed knowledge.db + xedge.db via wicked-knowledge-mcp MCP
4. Copy all 4 files to crates/wicked-estate-mcp/tests/fixtures/
5. Create SEED.md
6. Patch capture.py placeholders with real IDs
7. Run capture.py Stage 2 → response-schemas/
"""
import json
import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile

ESTATE = "/Users/michael.parcewski/Projects/wicked/wicked-estate"
FIXTURES_DIR = os.path.join(ESTATE, "crates", "wicked-estate-mcp", "tests", "fixtures")
CAPTURE_PY = os.path.join(ESTATE, "crates", "wicked-estate-mcp", "tests", "conformance", "capture.py")

ESTATE_BIN    = os.path.join(ESTATE, "target", "debug", "wicked-estate")
ESTATE_MCP    = os.path.join(ESTATE, "target", "debug", "wicked-estate-mcp")
MEMORY_MCP    = os.path.join(ESTATE, "..", "wicked-memory", "target", "debug", "wicked-memory-mcp")
KNOWLEDGE_MCP = os.path.join(ESTATE, "..", "wicked-knowledge", "target", "debug", "wicked-knowledge-mcp")


# ── Tiny fixture Rust source ──────────────────────────────────────────────────

FIXTURE_RS = """\
/// A minimal fixture for wicked-estate pre-build gate tests.
pub fn seed_fn() -> u32 {
    42
}

pub struct SeedStruct {
    pub value: u32,
}

impl SeedStruct {
    pub fn new(v: u32) -> Self {
        SeedStruct { value: v }
    }
}
"""

FIXTURE_FILENAME = "seed_fixture.rs"
SEED_SCOPE = "fixture"
SEED_QUERY = "seed"


# ── JSON-RPC helpers ──────────────────────────────────────────────────────────

def rpc(proc, req_id, method, params):
    req = {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params}
    proc.stdin.write((json.dumps(req) + "\n").encode())
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline()
        if not line:
            raise RuntimeError(
                "EOF from server (stderr: {})".format(proc.stderr.read().decode()[:500])
            )
        line = line.strip()
        if not line:
            continue
        msg = json.loads(line)
        if msg.get("id") == req_id:
            return msg


def open_server(cmd, env_extra, db_dir):
    env = dict(os.environ)
    env["WICKED_HOME"] = db_dir
    env["WICKED_ESTATE_DB"] = os.path.join(db_dir, "estate.db")
    env["WICKED_MEMORY_DB"] = os.path.join(db_dir, "memory.db")
    env["WICKED_KNOWLEDGE_DB"] = os.path.join(db_dir, "knowledge.db")
    env["WICKED_XEDGE_DB"] = os.path.join(db_dir, "xedge.db")
    env.update(env_extra)
    proc = subprocess.Popen(
        cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, env=env,
    )
    resp = rpc(proc, 1, "initialize", {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "fixture-seed", "version": "0.0.0"},
    })
    if "error" in resp:
        raise RuntimeError("initialize error: {}".format(resp["error"]))
    return proc


def close_server(proc):
    try:
        proc.stdin.close()
    except Exception:
        pass
    proc.terminate()
    proc.wait(timeout=5)


def check_result(resp, label):
    if "error" in resp:
        raise RuntimeError("{} failed: {}".format(label, resp["error"]))
    result = resp.get("result", {})
    content = result.get("content", [])
    is_error = result.get("isError", False)
    if is_error:
        raise RuntimeError("{} returned isError=true: {}".format(label, content))
    text = content[0].get("text", "") if content else ""
    print("  {} → {}".format(label, text[:80]))
    return result


# ── Step 1: Index estate fixture ──────────────────────────────────────────────

def seed_estate(workdir):
    fixture_path = os.path.join(workdir, FIXTURE_FILENAME)
    with open(fixture_path, "w") as f:
        f.write(FIXTURE_RS)

    estate_db = os.path.join(workdir, "estate.db")
    print("\n[1] Indexing fixture → estate.db ...")
    result = subprocess.run(
        [ESTATE_BIN, "index", workdir, "--db", estate_db],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        raise RuntimeError("wicked-estate index failed:\n" + result.stderr)
    print("    stdout:", result.stdout.strip()[:120] or "(none)")
    print("    stderr:", result.stderr.strip()[:200] or "(none)")

    # Query symbol ID and epoch from the DB
    con = sqlite3.connect(estate_db)
    rows = con.execute(
        "SELECT s.sym, n.name, s.gen FROM symbols s JOIN nodes n ON n.symbol = s.sid LIMIT 10"
    ).fetchall()
    con.close()
    print("    symbols found:", [(r[0][:60], r[1]) for r in rows])

    if not rows:
        raise RuntimeError("No symbols indexed into estate.db — check fixture file")

    # Pick the function node (seed_fn) as the primary symbol
    fn_row = next((r for r in rows if "seed_fn" in r[0] or "seed_fn" in r[1]), rows[0])
    symbol_id = fn_row[0]
    symbol_name = fn_row[1]
    symbol_epoch = fn_row[2]

    print("    seed symbol_id:   {}".format(symbol_id))
    print("    seed symbol_name: {}".format(symbol_name))
    print("    seed epoch:       {}".format(symbol_epoch))
    return symbol_id, symbol_name, symbol_epoch


# ── Step 2: Seed memory ───────────────────────────────────────────────────────

def seed_memory(workdir):
    print("\n[2] Seeding memory.db via wicked-memory-mcp ...")
    proc = open_server([MEMORY_MCP], {}, workdir)
    try:
        # Capture a node with scope (no about — no code graph in memory-only server)
        r = rpc(proc, 2, "tools/call", {
            "name": "memory.capture",
            "arguments": {
                "content": "seed episodic capture — fixture content for wicked-estate tests",
                "scope": SEED_SCOPE,
                "tier": "episodic",
            }
        })
        check_result(r, "memory.capture episodic")

        # Capture a second node with a different tier (semantic)
        r2 = rpc(proc, 3, "tools/call", {
            "name": "memory.capture",
            "arguments": {
                "content": "seed semantic capture — second fixture node",
                "scope": SEED_SCOPE,
                "tier": "semantic",
            }
        })
        check_result(r2, "memory.capture semantic")

        # Do a reflect to seed the sidecar
        r3 = rpc(proc, 4, "tools/call", {
            "name": "memory.reflect",
            "arguments": {"scope": SEED_SCOPE}
        })
        check_result(r3, "memory.reflect")

    finally:
        close_server(proc)
    print("    memory.db seeded")


# ── Step 3: Seed knowledge + xedge ───────────────────────────────────────────

def seed_knowledge(workdir, symbol_id):
    print("\n[3] Seeding knowledge.db + xedge.db via wicked-knowledge-mcp ...")
    proc = open_server([KNOWLEDGE_MCP], {}, workdir)
    try:
        # Ingest a doc
        r = rpc(proc, 2, "tools/call", {
            "name": "knowledge.ingest",
            "arguments": {
                "title": "seed doc",
                "chunks": [
                    "seed content chunk — fixture content for wicked-estate pre-build gate tests"
                ],
            }
        })
        res = check_result(r, "knowledge.ingest")
        # Extract node_id from the response text.
        # Response format: "ingested doc <full_symbol_id> + N chunk(s)"
        # full_symbol_id is the SymbolId string, e.g. "kdoc synthetic 019f37f9-...: "
        import re
        text = res.get("content", [{}])[0].get("text", "")
        node_id = None
        ingest_match = re.match(r"ingested doc (.+) \+ \d+ chunk", text)
        if ingest_match:
            node_id = ingest_match.group(1).strip()

        # Also write a concept node
        r2 = rpc(proc, 3, "tools/call", {
            "name": "knowledge.write",
            "arguments": {
                "content": "seed concept — fixture knowledge node",
                "class": "concept",
            }
        })
        res2 = check_result(r2, "knowledge.write")
        text2 = res2.get("content", [{}])[0].get("text", "")
        # Response format: "wrote <full_symbol_id>"
        if node_id is None:
            write_match = re.match(r"wrote (.+)", text2)
            if write_match:
                node_id = write_match.group(1).strip()

        if node_id is None:
            raise RuntimeError(
                "Could not determine knowledge node_id; ingest text: '{}'; write text: '{}'".format(text, text2)
            )

        print("    seed node_id: {}".format(node_id))

        # Link knowledge node to estate symbol via knowledge.relate_code → writes xedge
        r3 = rpc(proc, 4, "tools/call", {
            "name": "knowledge.relate_code",
            "arguments": {
                "knowledge_id": node_id,
                "code_ids": [symbol_id],
            }
        })
        check_result(r3, "knowledge.relate_code")

        # Verify xedge was written
        xedge_path = os.path.join(workdir, "xedge.db")
        if os.path.exists(xedge_path):
            con = sqlite3.connect(xedge_path)
            cnt = con.execute("SELECT COUNT(*) FROM xedges").fetchone()[0]
            con.close()
            print("    xedge.db xedge count: {}".format(cnt))
        else:
            print("    WARNING: xedge.db not created by knowledge.relate_code")

    finally:
        close_server(proc)

    return node_id


# ── Step 4: Verify all 4 DBs exist ───────────────────────────────────────────

def verify_dbs(workdir):
    print("\n[4] Verifying DB files ...")
    names = ["estate.db", "memory.db", "knowledge.db", "xedge.db"]
    for name in names:
        p = os.path.join(workdir, name)
        if not os.path.exists(p):
            raise RuntimeError("Missing expected DB: {}".format(p))
        size = os.path.getsize(p)
        print("    {} — {} bytes".format(name, size))


# ── Step 5: Copy to tests/fixtures/ ──────────────────────────────────────────

def copy_to_fixtures(workdir):
    print("\n[5] Copying to tests/fixtures/ ...")
    os.makedirs(FIXTURES_DIR, exist_ok=True)
    mapping = {
        "estate.db":    "estate_v0120.db",
        "memory.db":    "memory_v0121.db",
        "knowledge.db": "knowledge_v0121.db",
        "xedge.db":     "xedge_v0121.db",
    }
    for src_name, dst_name in mapping.items():
        src = os.path.join(workdir, src_name)
        dst = os.path.join(FIXTURES_DIR, dst_name)
        shutil.copy2(src, dst)
        print("    {} → {}".format(src_name, dst_name))
    # Also copy sidecar if present
    sidecar = os.path.join(workdir, "memory.db.memext")
    if os.path.exists(sidecar):
        shutil.copy2(sidecar, os.path.join(FIXTURES_DIR, "memory_v0121.db.memext"))
        print("    memory.db.memext → memory_v0121.db.memext")


# ── Step 6: Create SEED.md ────────────────────────────────────────────────────

def write_seed_md(symbol_id, symbol_name, symbol_epoch, node_id):
    print("\n[6] Writing SEED.md ...")
    seed_md = """\
# Fixture Seed Record

Generated by `seed_fixtures.py` from v0.12.x binaries (pre-Wave A, before any v0.13.0 engine changes).
This file is the authoritative record for SC-009, IT-050, IT-051, IT-052, and §1.5 DB compatibility tests.

## Estate Fixture (`estate_v0120.db`)

Source binary: `wicked-estate v0.12.0` (wicked-estate workspace, `lane-a/epoch` branch)
Indexed from: `seed_fixture.rs` (inline Rust source, one function + one struct)

| Field | Value |
|---|---|
| Seed symbol_id | `{symbol_id}` |
| Seed symbol_name | `{symbol_name}` |
| Seed symbol_epoch | `{symbol_epoch}` |
| Search query (SearchEntity) | `seed_fn` |

**§1.5 test instructions:** Call `SearchEntity(name="seed_fn")` against estate_v0120.db opened in the v0.13.0 engine; assert non-empty results.

## Memory Fixture (`memory_v0121.db`)

Source binary: `wicked-memory-mcp v0.12.1` (wicked-memory workspace)
Seeded with: 2 `memory.capture` calls (one episodic, one semantic) + `memory.reflect`

| Field | Value |
|---|---|
| Seed scope | `fixture` |
| Seed content query | `seed` |
| Sidecar | `memory_v0121.db.memext` (if present) |

**§1.5 test instructions:** Call `memory.recall(query="seed", token_budget=512)` against memory_v0121.db; assert non-empty results.

## Knowledge Fixture (`knowledge_v0121.db`)

Source binary: `wicked-knowledge-mcp v0.12.1` (wicked-knowledge workspace)
Seeded with: `knowledge.ingest` (1 doc, 1 chunk) + `knowledge.write` (1 concept)

| Field | Value |
|---|---|
| Seed node_id | `{node_id}` |
| Seed query | `seed` |

**§1.5 test instructions:** Call `knowledge.recall(query="seed", token_budget=512)` against knowledge_v0121.db; assert non-empty results.

## XEdge Fixture (`xedge_v0121.db`)

Source binary: `wicked-knowledge-mcp v0.12.1` (XedgeStore writer)
Seeded with: `knowledge.relate_code(knowledge_id="{node_id}", code_ids=["{symbol_id}"])` → 1 about-edge

| Field | Value |
|---|---|
| Xedge src_engine | `knowledge` |
| Xedge src_id | `{node_id}` |
| Xedge tgt_engine | `estate` |
| Xedge tgt_id | `{symbol_id}` |
| Xedge tgt_epoch | `0` (hardcoded by `XEdge::about`) |
| Estate symbol current epoch | `{symbol_epoch}` |

**Epoch resolution:** The seeded xedge has `tgt_epoch=0` (XEdge::about hardcodes epoch=0). The estate symbol's epoch at seeding time was `{symbol_epoch}`. Since the fixture was created from a freshly-indexed store, the symbol epoch is 0. Therefore the about-edge IS current (epoch 0 == epoch 0) — the expected outcome for `knowledge.recall_about_code(code_ids=["{symbol_id}"])` in SC-009/§1.5 is a non-empty result (edge resolves, not epoch-dropped).

**§1.5 test instructions:** Call `knowledge.recall_about_code(code_ids=["{symbol_id}"])` against knowledge_v0121.db + xedge_v0121.db; assert non-empty results (about-edge resolves at epoch 0).

## Generation Script

`crates/wicked-estate-mcp/tests/fixtures/seed_fixtures.py` (this script was used to generate these fixtures)
""".format(
        symbol_id=symbol_id,
        symbol_name=symbol_name,
        symbol_epoch=symbol_epoch,
        node_id=node_id,
    )
    seed_path = os.path.join(FIXTURES_DIR, "SEED.md")
    with open(seed_path, "w") as f:
        f.write(seed_md)
    print("    Written: SEED.md")


# ── Step 7: Patch capture.py and run Stage 2 ─────────────────────────────────

def patch_and_run_stage2(workdir, symbol_id, symbol_name, node_id):
    print("\n[7] Patching capture.py with real IDs and running Stage 2 ...")
    with open(CAPTURE_PY, "r") as f:
        src = f.read()

    orig_sid  = 'SEED_SYMBOL_ID = "REPLACE_WITH_REAL_SYMBOL_ID"'
    orig_nid  = 'SEED_NODE_ID   = "REPLACE_WITH_REAL_NODE_ID"'
    orig_name = 'SEED_SYMBOL_NAME = "REPLACE_WITH_REAL_SYMBOL_NAME"'

    new_sid  = 'SEED_SYMBOL_ID = "{}"'.format(symbol_id)
    new_nid  = 'SEED_NODE_ID   = "{}"'.format(node_id)
    new_name = 'SEED_SYMBOL_NAME = "{}"'.format(symbol_name)

    if orig_sid not in src:
        print("    WARNING: SEED_SYMBOL_ID placeholder already replaced in capture.py — skipping patch")
        already_patched = True
    else:
        already_patched = False
        src = src.replace(orig_sid, new_sid)
        src = src.replace(orig_nid, new_nid)
        src = src.replace(orig_name, new_name)
        with open(CAPTURE_PY, "w") as f:
            f.write(src)
        print("    Patched capture.py with real IDs")

    # Run Stage 2
    env = dict(os.environ)
    env["ESTATE_BINARY"] = ESTATE_MCP  # override the broken path in capture.py
    result = subprocess.run(
        [sys.executable, CAPTURE_PY, "--response-goldens", "--seeded-db-dir", workdir],
        capture_output=True, text=True, env=env,
    )
    print("    Stage 2 stdout:", result.stdout.strip()[:500] or "(none)")
    print("    Stage 2 stderr:", result.stderr.strip()[:800] or "(none)")
    if result.returncode != 0:
        print("    WARNING: Stage 2 exited with code {}".format(result.returncode))
    else:
        print("    Stage 2 complete")


# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    print("=== wicked-estate pre-build gate: fixture seeding + Stage 2 golden capture ===\n")

    with tempfile.TemporaryDirectory(prefix="wicked-estate-fixtures-") as workdir:
        print("Working dir: {}".format(workdir))

        symbol_id, symbol_name, symbol_epoch = seed_estate(workdir)
        seed_memory(workdir)
        node_id = seed_knowledge(workdir, symbol_id)
        verify_dbs(workdir)
        copy_to_fixtures(workdir)
        write_seed_md(symbol_id, symbol_name, symbol_epoch, node_id)
        patch_and_run_stage2(workdir, symbol_id, symbol_name, node_id)

    print("\n=== Done ===")
    print("Fixtures in:        {}".format(FIXTURES_DIR))
    print("Response goldens in: crates/wicked-estate-mcp/tests/conformance/response-schemas/")


if __name__ == "__main__":
    main()
