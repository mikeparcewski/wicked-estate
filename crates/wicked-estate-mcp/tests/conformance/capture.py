#!/usr/bin/env python3
"""Golden schema + response capture per TEST-001 §3.3.

Stage 1 (--schema-goldens, or no arguments — defaults to Stage 1):
  Drives each v0.12.x MCP binary over stdio: initialize -> tools/list.
  Writes crates/wicked-estate-mcp/tests/conformance/schemas/<tool>.json
  (name, description, inputSchema) and raw tools-list captures for provenance.

Stage 2 (--response-goldens --seeded-db-dir <dir>):
  Requires pre-seeded stores (see §3.3 step 2).
  IMPORTANT: The REPRESENTATIVE_CALLS dict uses placeholder strings
  (SEED_SYMBOL_ID, SEED_NODE_ID, SEED_SYMBOL_NAME, SEED_QUERY) that MUST be
  replaced with real IDs from the seeded stores before running Stage 2.
  Edit the constants at the top of the REPRESENTATIVE_CALLS dict below.
  Issues one representative tools/call per tool against the seeded v0.12.x binaries.
  Records top-level result field names+types and item-level field names+types for
  array-of-objects fields (one nesting level). Writes:
    crates/wicked-estate-mcp/tests/conformance/response-schemas/<tool>.json

Binaries required (build from source BEFORE Wave A):
  wicked-estate-mcp v0.12.0     (wicked-estate workspace)
  wicked-memory-mcp v0.12.1     (wicked-memory workspace)
  wicked-knowledge-mcp v0.12.1  (wicked-knowledge workspace)

SemanticSearch schema golden requires:
  cargo build -p wicked-estate-mcp --features fastembed
then re-run this script pointing ESTATE_BINARY at the fastembed binary:
  ESTATE_BINARY=/path/to/fastembed/wicked-estate-mcp python3 capture.py --schema-goldens

Tier enum (HC-007 frozen) — use these string values in API calls, NOT T-codes:
  T0/working   = "working"
  T1/episodic  = "episodic"
  T2/semantic  = "semantic"   (reflect output; T2 facts)
  T3/procedural= "procedural" (learn output; T3 facts)
  T4/archival  = "archival"   (excluded from standard recall)

memory.learn param name is `symbols` (not `about`; `about` is memory.capture's param).
knowledge.relate params are `src`, `tgt`, `rel` (not src_id/tgt_id/kind).
knowledge.write param is `content` (not label/body).
knowledge.ingest params are `title` and `chunks` (not doc.id/doc.body).
memory.recall pagination uses `token_budget` (not `limit`).
ContextBundle seed uses `symbol` or `query` (not `target`).
ALL estate symbol-lookup tools use `symbol` as the required param (NOT `symbol_id`):
  RetrieveEntity, BlastRadius, TraverseGraph, FetchContent, Lineage — all require `symbol`.
Traversal depth param is `depth` (NOT `max_depth`) for TraverseGraph, BlastRadius, Lineage.
TraverseGraph direction enum is ["dependencies", "dependents", "both"] (NOT "outbound"/"inbound").
memory.learn tier is constrained to ["semantic", "procedural"] only (NOT all 5 tier values).

Stage 2 open_server() sets WICKED_ESTATE_DB (not just WICKED_HOME) because wicked-estate-mcp
v0.12.0 resolves its store via WICKED_ESTATE_DB env var, not WICKED_HOME.
"""
import argparse
import json
import os
import subprocess
import sys

ESTATE = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", ".."))
OUT_SCHEMAS = os.path.join(ESTATE, "crates", "wicked-estate-mcp", "tests", "conformance", "schemas")
OUT_RESPONSE = os.path.join(ESTATE, "crates", "wicked-estate-mcp", "tests", "conformance", "response-schemas")
OUT_RAW = os.path.join(ESTATE, "crates", "wicked-estate-mcp", "tests", "conformance", "raw")

ESTATE_BINARY = os.environ.get("ESTATE_BINARY", os.path.join(ESTATE, "target", "debug", "wicked-estate-mcp"))

SERVERS = [
    {
        "name": "wicked-estate-mcp-0.12.0",
        "cmd": [ESTATE_BINARY],
        "env": {},
    },
    {
        "name": "wicked-memory-mcp-0.12.1",
        "cmd": [os.path.join(os.path.dirname(ESTATE), "wicked-memory", "target", "debug", "wicked-memory-mcp")],
        "env": {},
    },
    {
        "name": "wicked-knowledge-0.12.1",
        "cmd": [os.path.join(os.path.dirname(ESTATE), "wicked-knowledge", "target", "debug", "wicked-knowledge-mcp")],
        "env": {},
    },
]

# Representative params for each tool — Stage 2 response-schema golden capture.
# BEFORE RUNNING STAGE 2: Replace the placeholder values below with real IDs
# from the seeded stores passed via --seeded-db-dir.
SEED_SYMBOL_ID = "ts-rust . . . seed_fixture/seed_fn()."   # stable estate symbol id from seeded estate.db
SEED_NODE_ID   = "kdoc synthetic 019f37fb-0a72-7d03-9aeb-075e49b0f3cb:"      # knowledge node_id from seeded knowledge.db
SEED_SYMBOL_NAME = "seed_fn" # symbol name searchable in FTS
SEED_QUERY     = "seed"                            # FTS query matching seeded content

REPRESENTATIVE_CALLS = {
    # Estate tools (10 standard + 1 fastembed) — param names from HC-007 frozen golden schemas
    # Required field is `symbol` for all symbol-lookup tools (NOT `symbol_id`)
    # Traversal depth param is `depth` (NOT `max_depth`); direction enum is ["dependencies","dependents","both"]
    "SearchEntity":               {"name": SEED_SYMBOL_NAME, "limit": 5},
    "RetrieveEntity":             {"symbol": SEED_SYMBOL_ID},       # param is "symbol", NOT "symbol_id"
    "TraverseGraph":              {"symbol": SEED_SYMBOL_ID, "direction": "dependencies", "depth": 1},  # direction enum NOT "outbound"; depth NOT max_depth
    "BlastRadius":                {"symbol": SEED_SYMBOL_ID, "depth": 1},  # param is "symbol", NOT "symbol_id"; depth NOT max_depth
    "FetchContent":               {"symbol": SEED_SYMBOL_ID},       # param is "symbol", NOT "symbol_id"
    "ContextBundle":              {"symbol": SEED_SYMBOL_ID},       # param is "symbol", NOT "target"
    "RulesInventory":             {},
    "RankHotspots":               {"limit": 5},
    "Communities":                {"limit": 5},
    "Lineage":                    {"symbol": SEED_SYMBOL_ID, "depth": 2},  # param is "symbol", NOT "symbol_id"; depth NOT max_depth
    "SemanticSearch":             {"query": SEED_QUERY},            # fastembed build only
    # Memory tools (6) — HC-007 frozen golden schema param names
    "memory.capture":             {"content": "seed capture", "scope": "test", "tier": "episodic"},  # tier enum NOT "T1"
    "memory.recall":              {"query": SEED_QUERY, "token_budget": 512},  # token_budget NOT limit
    "memory.reflect":             {"scope": "test"},
    "memory.erase":               {"scope_prefix": "test"},
    "memory.learn":               {"content": "seed fact", "symbols": [SEED_SYMBOL_NAME], "tier": "semantic"},  # symbols takes NAME strings (engine.resolve_code), NOT stable IDs; golden: "exact code symbol name(s)"
    "memory.coverage":            {},
    # Knowledge tools (7) — HC-007 frozen golden schema param names
    "knowledge.ingest":           {"title": "seed doc", "chunks": ["seed content chunk"]},  # NOT doc:{id,title,body,class}
    "knowledge.write":            {"content": "seed concept", "class": "concept"},           # content NOT label/body
    "knowledge.relate":           {"src": SEED_NODE_ID, "tgt": SEED_NODE_ID, "rel": "related_to"},  # src/tgt/rel NOT src_id/tgt_id/kind
    "knowledge.recall":           {"query": SEED_QUERY, "token_budget": 512},               # token_budget NOT limit
    "knowledge.coverage":         {},
    "knowledge.relate_code":      {"knowledge_id": SEED_NODE_ID, "code_ids": [SEED_SYMBOL_ID]},  # code_ids NOT symbol_ids
    "knowledge.recall_about_code": {"code_ids": [SEED_SYMBOL_ID]},                          # code_ids NOT symbol_id
}


def rpc(proc, req_id, method, params):
    req = {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params}
    proc.stdin.write((json.dumps(req) + "\n").encode())
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline()
        if not line:
            raise RuntimeError("EOF from server (stderr: {})".format(
                proc.stderr.read().decode()[:500]))
        line = line.strip()
        if not line:
            continue
        msg = json.loads(line)
        if msg.get("id") == req_id:
            return msg


def open_server(server, db_dir):
    env = dict(os.environ)
    env["WICKED_HOME"] = db_dir
    env["WICKED_ESTATE_DB"] = os.path.join(db_dir, "estate.db")
    env["WICKED_MEMORY_DB"] = os.path.join(db_dir, "memory.db")
    env["WICKED_KNOWLEDGE_DB"] = os.path.join(db_dir, "knowledge.db")
    env["WICKED_XEDGE_DB"] = os.path.join(db_dir, "xedge.db")
    env.update(server["env"])
    proc = subprocess.Popen(
        server["cmd"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, env=env,
    )
    rpc(proc, 1, "initialize", {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "golden-capture", "version": "0.0.0"},
    })
    return proc


def capture_schemas(server, tmpdir):
    proc = open_server(server, tmpdir)
    try:
        tools = rpc(proc, 2, "tools/list", {})
    finally:
        proc.stdin.close()
        proc.terminate()
        proc.wait(timeout=5)

    if "error" in tools:
        raise RuntimeError("{} tools/list error: {}".format(server["name"], tools["error"]))
    tool_list = tools["result"]["tools"]
    os.makedirs(OUT_SCHEMAS, exist_ok=True)
    os.makedirs(OUT_RAW, exist_ok=True)
    server_name = server["name"]
    with open(os.path.join(OUT_RAW, server_name + "-tools-list.json"), "w") as f:
        json.dump(tools["result"], f, indent=2, sort_keys=True)
    names = []
    for t in tool_list:
        names.append(t["name"])
        golden = {
            "name": t["name"],
            "description": t.get("description", ""),
            "inputSchema": t.get("inputSchema", {}),
            "captured_from": server_name,
        }
        with open(os.path.join(OUT_SCHEMAS, t["name"] + ".json"), "w") as f:
            json.dump(golden, f, indent=2, sort_keys=True)
    return names


def _json_type(value):
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int):
        return "integer"
    if isinstance(value, float):
        return "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    if isinstance(value, dict):
        return "object"
    return "null"


def capture_response_schema(tool_name, result):
    schema = {"fields": {}, "item_fields": {}}
    if isinstance(result, dict):
        for k, v in result.items():
            schema["fields"][k] = _json_type(v)
            if isinstance(v, list) and v and isinstance(v[0], dict):
                schema["item_fields"][k] = {fk: _json_type(fv) for fk, fv in v[0].items()}
    elif isinstance(result, list):
        schema["fields"]["_root"] = "array"
        if result and isinstance(result[0], dict):
            schema["item_fields"]["_root"] = {fk: _json_type(fv) for fk, fv in result[0].items()}
    return schema


def capture_response_goldens(server, seeded_db_dir):
    proc = open_server(server, seeded_db_dir)
    try:
        tools_resp = rpc(proc, 2, "tools/list", {})
        available = {t["name"] for t in tools_resp["result"]["tools"]}

        os.makedirs(OUT_RESPONSE, exist_ok=True)
        req_id = 3
        captured = []
        skipped = []
        for tool_name, params in REPRESENTATIVE_CALLS.items():
            if tool_name not in available:
                skipped.append(tool_name)
                continue
            resp = rpc(proc, req_id, "tools/call", {"name": tool_name, "arguments": params})
            req_id += 1
            if "error" in resp:
                print("  WARN: {} returned error: {}".format(tool_name, resp["error"]), file=sys.stderr)
                result_shape = {"_error": resp["error"]}
            else:
                result_shape = resp.get("result", {})
            schema = capture_response_schema(tool_name, result_shape)
            golden = {
                "tool": tool_name,
                "captured_from": server["name"],
                "response_schema": schema,
                "representative_params": params,
            }
            with open(os.path.join(OUT_RESPONSE, tool_name + ".json"), "w") as f:
                json.dump(golden, f, indent=2, sort_keys=True)
            captured.append(tool_name)
    finally:
        proc.stdin.close()
        proc.terminate()
        proc.wait(timeout=5)
    return captured, skipped


def main():
    parser = argparse.ArgumentParser(
        description="Golden capture for TEST-001 §3.3. Defaults to Stage 1 (--schema-goldens) if no mode flag given.")
    parser.add_argument("--schema-goldens", action="store_true", default=False,
                        help="Stage 1: capture inputSchema goldens (no seeded stores needed). Default when no flag given.")
    parser.add_argument("--response-goldens", action="store_true", default=False,
                        help="Stage 2: capture response-schema goldens (requires --seeded-db-dir and real IDs in REPRESENTATIVE_CALLS)")
    parser.add_argument("--seeded-db-dir", default=None,
                        help="Path to directory containing pre-seeded .db files for Stage 2")
    args = parser.parse_args()

    if not args.schema_goldens and not args.response_goldens:
        args.schema_goldens = True  # default: Stage 1 only

    import tempfile

    if args.schema_goldens:
        print("=== Stage 1: Schema Goldens ===")
        with tempfile.TemporaryDirectory() as tmpdir:
            results = {}
            for server in SERVERS:
                if not os.path.exists(server["cmd"][0]):
                    print("MISSING BINARY: {}".format(server["cmd"][0]), file=sys.stderr)
                    sys.exit(2)
                names = capture_schemas(server, tmpdir)
                results[server["name"]] = names
                print("  {}: {} tools -> {}".format(server["name"], len(names), sorted(names)))
        total = sum(len(v) for v in results.values())
        print("TOTAL: {} schema golden files in {}".format(total, OUT_SCHEMAS))
        print("NOTE: SemanticSearch requires fastembed build; set ESTATE_BINARY to the fastembed binary.")

    if args.response_goldens:
        print("=== Stage 2: Response Goldens ===")
        if not args.seeded_db_dir:
            print("ERROR: --seeded-db-dir is required for --response-goldens", file=sys.stderr)
            sys.exit(1)
        if SEED_SYMBOL_ID == "REPLACE_WITH_REAL_SYMBOL_ID":
            print("ERROR: Edit SEED_SYMBOL_ID in capture.py before Stage 2", file=sys.stderr)
            sys.exit(1)
        if SEED_NODE_ID == "REPLACE_WITH_REAL_NODE_ID":
            print("ERROR: Edit SEED_NODE_ID in capture.py before Stage 2", file=sys.stderr)
            sys.exit(1)
        if SEED_SYMBOL_NAME == "REPLACE_WITH_REAL_SYMBOL_NAME":
            print("ERROR: Edit SEED_SYMBOL_NAME in capture.py before Stage 2", file=sys.stderr)
            sys.exit(1)
        for server in SERVERS:
            if not os.path.exists(server["cmd"][0]):
                print("MISSING BINARY: {}".format(server["cmd"][0]), file=sys.stderr)
                sys.exit(2)
            captured, skipped = capture_response_goldens(server, args.seeded_db_dir)
            print("  {}: {} captured, {} skipped".format(server["name"], len(captured), len(skipped)))
            if skipped:
                print("    not in tools/list: {}".format(skipped))
        total = sum(1 for f in os.listdir(OUT_RESPONSE) if f.endswith(".json")) if os.path.isdir(OUT_RESPONSE) else 0
        print("TOTAL: {} response golden files in {}".format(total, OUT_RESPONSE))


if __name__ == "__main__":
    main()
