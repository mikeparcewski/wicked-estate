#!/usr/bin/env python3
"""ADR-014 measurement procedure: a baseline and a candidate `wicked-estate` binary, one corpus.

    measure-dataflow.py --base BIN --cand BIN --corpus DIR --touch REL --new-constructs a,b
                        [--runs 3] [--out results.json]

Every run is `index <corpus> --db <fresh> --force`, interleaved base/cand, median of `--runs`.
Incremental = re-index (no --force) after appending one comment line to `--touch`.
Edges = the `flows_to` count from `stats --json`; DB size = `db_size_bytes` of a fresh full index.
Lineage = 10 fixed queries (the baseline's 10 largest flows_to producers by out-degree, ties by id),
plus a sweep over every baseline Parameter slot: `lineage <id> --relation flows_to --json` at the
default depth, recording each document's size and `truncated` flag.
Semantics gate = every `flow_support` row in the candidate graph whose construct is one of
`--new-constructs` must be `may_influence`.

Prints a Markdown table and writes the raw numbers as JSON. Exit 0 always: the gates are judged
by a reader of the table, never by this script alone.
"""
import argparse, json, os, shutil, sqlite3, statistics, subprocess, sys, tempfile, time


def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def full_index(binary, corpus, db):
    for suffix in ("", "-wal", "-shm"):
        if os.path.exists(db + suffix):
            os.remove(db + suffix)
    t = time.monotonic()
    r = run([binary, "index", corpus, "--db", db, "--force"])
    dt = time.monotonic() - t
    if r.returncode != 0:
        sys.exit(f"index failed ({binary}): {r.stderr[-2000:]}")
    return dt


def incremental(binary, corpus, db, touch, n):
    with open(os.path.join(corpus, touch), "a") as f:
        f.write(f"\n// measure-dataflow touch {n}\n")
    t = time.monotonic()
    r = run([binary, "index", corpus, "--db", db])
    dt = time.monotonic() - t
    if r.returncode != 0:
        sys.exit(f"incremental index failed ({binary}): {r.stderr[-2000:]}")
    return dt


def stats(binary, db):
    r = run([binary, "stats", "--json", "--db", db])
    d = json.loads(r.stdout)
    flows = sum(v for k, v in d["edges_by_kind"].items() if "flows_to" in k)
    return {"flows_to": flows, "edges": d["edges"], "nodes": d["nodes"],
            "db_size_bytes": d["db_size_bytes"]}


def edges(db):
    c = sqlite3.connect(db)
    out = [json.loads(row[0]) for row in
           c.execute("select data from edges where kind like '%flows_to%'")]
    c.close()
    return out


def lineage_roots(db):
    out_degree = {}
    for e in edges(db):
        out_degree[e["target"]] = out_degree.get(e["target"], 0) + 1  # stored consumer -> producer
    top = sorted(out_degree.items(), key=lambda kv: (-kv[1], kv[0]))[:10]
    c = sqlite3.connect(db)
    params = sorted(json.loads(row[0])["symbol"] for row in
                    c.execute("select data from nodes where kind = '\"parameter\"'"))
    c.close()
    return [s for s, _ in top], params


def lineage(binary, db, symbol):
    r = run([binary, "lineage", symbol, "--relation", "flows_to", "--json", "--db", db])
    if r.returncode != 0:
        return {"error": r.stderr[-300:], "size": 0, "truncated": None, "total": None}
    d = json.loads(r.stdout)
    return {"size": len(r.stdout), "truncated": bool(d.get("truncated")), "total": d.get("total")}


def semantics_gate(db, new):
    by_construct, violations, merged = {}, [], 0
    for e in edges(db):
        rows = e.get("metadata", {}).get("flow_support", [])
        kinds = {row.get("construct") for row in rows}
        if kinds & new and kinds - new:
            merged += 1
        for row in rows:
            key = (row.get("construct"), row.get("semantics"))
            by_construct[key] = by_construct.get(key, 0) + 1
            if row.get("construct") in new and row.get("semantics") != "may_influence":
                violations.append((e["source"], e["target"], key))
    return by_construct, violations, merged


def pct(a, b):
    return 100.0 * (b - a) / a if a else float("nan")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", required=True)
    ap.add_argument("--cand", required=True)
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--touch", required=True)
    ap.add_argument("--new-constructs", required=True)
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--out", default="results.json")
    a = ap.parse_args()
    new = set(filter(None, a.new_constructs.split(",")))
    work = tempfile.mkdtemp(prefix="measure-dataflow-")
    corpus = os.path.join(work, "corpus")
    shutil.copytree(a.corpus, corpus, symlinks=True)
    dbs = {"base": os.path.join(work, "base.db"), "cand": os.path.join(work, "cand.db")}
    bins = {"base": a.base, "cand": a.cand}
    touch_path = os.path.join(corpus, a.touch)
    pristine = open(touch_path).read()
    t_full = {"base": [], "cand": []}
    t_inc = {"base": [], "cand": []}
    for i in range(a.runs):
        for side in ("base", "cand"):
            with open(touch_path, "w") as f:
                f.write(pristine)
            t_full[side].append(full_index(bins[side], corpus, dbs[side]))
            t_inc[side].append(incremental(bins[side], corpus, dbs[side], a.touch, i))
    # The graphs measured are fresh full indexes of the pristine corpus.
    with open(touch_path, "w") as f:
        f.write(pristine)
    for side in ("base", "cand"):
        full_index(bins[side], corpus, dbs[side])
    st = {side: stats(bins[side], dbs[side]) for side in ("base", "cand")}
    fixed, params = lineage_roots(dbs["base"])
    lin_fixed = {s: {side: lineage(bins[side], dbs[side], s) for side in ("base", "cand")}
                 for s in fixed}
    sweep_new_trunc, sweep_base_trunc, sweep_cand_trunc = [], 0, 0
    for s in params:
        b, c = lineage(bins["base"], dbs["base"], s), lineage(bins["cand"], dbs["cand"], s)
        sweep_base_trunc += bool(b["truncated"])
        sweep_cand_trunc += bool(c["truncated"])
        if c["truncated"] and not b["truncated"]:
            sweep_new_trunc.append(s)
    by_construct, violations, merged = semantics_gate(dbs["cand"], new)
    med = lambda xs: statistics.median(xs)
    res = {
        "corpus": a.corpus, "runs": a.runs, "stats": st,
        "full_index_s": t_full, "incremental_s": t_inc,
        "lineage_fixed": lin_fixed,
        "lineage_sweep": {"queries": len(params), "base_truncated": sweep_base_trunc,
                          "cand_truncated": sweep_cand_trunc,
                          "newly_truncated": sweep_new_trunc},
        "new_construct_support": {f"{k[0]}/{k[1]}": v for k, v in sorted(by_construct.items())
                                  if k[0] in new},
        "semantics_violations": violations, "edges_merged_with_existing_construct": merged,
    }
    json.dump(res, open(a.out, "w"), indent=2)
    newly_fixed = [s for s, v in lin_fixed.items()
                   if v["cand"]["truncated"] and not v["base"]["truncated"]]
    fb, fc = med(t_full["base"]), med(t_full["cand"])
    ib, ic = med(t_inc["base"]), med(t_inc["cand"])
    sb, sc = st["base"], st["cand"]
    rows = [
        ("`flows_to` edges", sb["flows_to"], sc["flows_to"], pct(sb["flows_to"], sc["flows_to"]), "≤ +25 %"),
        ("full index (s, median)", round(fb, 2), round(fc, 2), pct(fb, fc), "≤ +15 %"),
        ("incremental re-index (s, median)", round(ib, 2), round(ic, 2), pct(ib, ic), "≤ +15 %"),
        ("DB size (bytes)", sb["db_size_bytes"], sc["db_size_bytes"],
         pct(sb["db_size_bytes"], sc["db_size_bytes"]), "≤ +10 %"),
    ]
    print(f"### {os.path.basename(a.corpus.rstrip('/'))}\n")
    print("| metric | baseline | candidate | change | gate |\n|---|---|---|---|---|")
    for name, b, c, p, g in rows:
        print(f"| {name} | {b} | {c} | {p:+.2f} % | {g} |")
    print(f"| Lineage, 10 fixed queries newly truncated | | | {len(newly_fixed)} | 0 |")
    print(f"| Lineage sweep ({len(params)} parameter roots) newly truncated | "
          f"{sweep_base_trunc} truncated | {sweep_cand_trunc} truncated | "
          f"{len(sweep_new_trunc)} | 0 |")
    print(f"| `value_preserving` rows from a new construct | | | {len(violations)} | 0 |")
    print(f"\nnodes {sb['nodes']} -> {sc['nodes']}, edges {sb['edges']} -> {sc['edges']}; "
          f"new-construct support rows: {res['new_construct_support']}; "
          f"edges where a new construct merged into an existing construct's edge: {merged}")
    print("\nfixed lineage queries (doc bytes base -> cand, truncated):")
    for s, v in lin_fixed.items():
        print(f"- `{s}`: {v['base']['size']} -> {v['cand']['size']}, "
              f"{v['base']['truncated']} -> {v['cand']['truncated']}")
    shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
