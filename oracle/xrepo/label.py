#!/usr/bin/env python3
"""Label the project linker's predicted package links with the package managers' OWN resolution
(PLAN §6.3, slice X1): the truth never reuses the linker's code or its reading of manifests.

usage: label.py <project.db> <case> <label>=<checkout> [<label>=<checkout> ...] > labels.json

For every link edge in the db (`resolved_by = wicked-link`):
- npm: Node resolves the specifier from the importing file's directory (`require.resolve`, then
  `realpath`, so workspace symlinks land on the workspace dir). The link is TRUE when the resolved
  package directory is the provider's package directory and the linked file is Node's entry, or the
  package's declared `types` file when Node cannot resolve a types-only package. An installed
  registry copy (under node_modules) counts as the provider when its `repository` and `directory`
  name the provider checkout's package and its name matches.
- cargo: `cargo metadata` (Cargo's own resolution with `--all-features`, `[patch]` applied; nothing is compiled; an optional dependency counts). The
  link is TRUE when Cargo resolves the importing file's package to the LOCAL provider package and
  the linked file is that package's lib target `src_path`.
- other ecosystems: `unjudged` (excluded from the score).

Output rows match score.py: case, from_file, to_file, identity, label ("true"/"false"), reason.
"""
import json
import os
import sqlite3
import subprocess
import sys


def edges(db):
    con = sqlite3.connect(db)
    out = []
    for (data,) in con.execute("SELECT data FROM edges"):
        e = json.loads(data)
        if e.get("resolved_by") != "wicked-link":
            continue
        out.append(e)
    return out


def file_of(sym):
    # A File node's SymbolId is `file . <store path>:`.
    s = sym if isinstance(sym, str) else sym.get("0", "")
    if s.startswith("file . ") and s.endswith(":"):
        return s[len("file . "):-1]
    return s


def split(store_path, roots):
    label, _, rel = store_path.partition("/")
    return label, rel, roots.get(label)


def node_resolve(spec, from_dir):
    js = (
        "const p=require('path'),fs=require('fs');"
        "try{const r=require.resolve(process.argv[1],{paths:[process.argv[2]]});"
        "console.log(fs.realpathSync(r))}catch(e){console.log('')}"
    )
    r = subprocess.run(["node", "-e", js, spec, from_dir], capture_output=True, text=True, timeout=30)
    return r.stdout.strip()


def pkg_dir_of(path):
    d = os.path.dirname(path)
    while d and d != "/":
        pj = os.path.join(d, "package.json")
        if os.path.isfile(pj):
            try:
                if "name" in json.load(open(pj)):
                    return d
            except Exception:
                pass
        d = os.path.dirname(d)
    return ""


def installed_pkg(pkg, from_dir):
    d = from_dir
    while d and d != "/":
        cand = os.path.join(d, "node_modules", pkg)
        if os.path.isdir(cand):
            return os.path.realpath(cand)
        d = os.path.dirname(d)
    return ""


def git_remote(root):
    r = subprocess.run(["git", "-C", root, "remote", "get-url", "origin"], capture_output=True, text=True)
    return r.stdout.strip()


def norm_repo(u):
    u = (u or "").strip()
    for p in ("git+", ):
        if u.startswith(p):
            u = u[len(p):]
    u = u.replace("git@github.com:", "https://github.com/").removesuffix(".git").rstrip("/")
    return u.split("://", 1)[-1].lower()


def judge_npm(e, roots):
    md = e.get("metadata", {})
    pkg = md.get("link.package", "")
    frm, to = file_of(e["source"]), file_of(e["target"])
    fl, frel, froot = split(frm, roots)
    tl, trel, troot = split(to, roots)
    if not froot or not troot:
        return "false", "checkout not given"
    from_abs = os.path.realpath(os.path.join(froot, frel))
    to_abs = os.path.realpath(os.path.join(troot, trel))
    prov_dir = pkg_dir_of(to_abs)
    inst = installed_pkg(pkg, os.path.dirname(from_abs))
    if not inst:
        return "false", f"node finds no installed {pkg} from the importing file"
    if inst != prov_dir:
        # A registry copy: it is the provider when its manifest names the provider's package.
        if "node_modules" not in inst:
            return "false", f"node resolves {pkg} to {inst}, not {prov_dir}"
        m = json.load(open(os.path.join(inst, "package.json")))
        repo = m.get("repository") or {}
        url = repo if isinstance(repo, str) else repo.get("url", "")
        dirn = "" if isinstance(repo, str) else (repo.get("directory") or "")
        prov_rel = os.path.relpath(prov_dir, troot).replace(os.sep, "/")
        if prov_rel == ".":
            prov_rel = ""
        same_repo = norm_repo(url) == norm_repo(git_remote(troot))
        same_dir = dirn.strip("/") in ("", prov_rel)
        prov_name = json.load(open(os.path.join(prov_dir, "package.json"))).get("name")
        if not (same_repo and same_dir and m.get("name") == prov_name == pkg):
            return "false", f"installed copy {inst} is not the provider package"
        rel_in_pkg = os.path.relpath(to_abs, prov_dir)
        if not os.path.exists(os.path.join(inst, rel_in_pkg)) and not rel_in_pkg.startswith("src"):
            return "false", f"linked file {rel_in_pkg} is not in the installed package"
        return "true", "registry copy of the provider package"
    entry = node_resolve(pkg, os.path.dirname(from_abs))
    if entry and entry == to_abs:
        return "true", "node entry"
    m = json.load(open(os.path.join(prov_dir, "package.json")))
    types = m.get("types") or m.get("typings")
    if types and os.path.realpath(os.path.join(prov_dir, types)) == to_abs:
        return "true", "declared types entry"
    if entry and entry.startswith(prov_dir):
        # Same package, different file: a dist->src mapping (tsconfig-outdir) is judged by stem.
        stem = lambda p: os.path.splitext(os.path.basename(p).replace(".d.ts", ".ts"))[0]
        if stem(entry) == stem(to_abs):
            return "true", "package entry mapped to its source"
        return "false", f"node entry is {os.path.relpath(entry, prov_dir)}"
    return "true" if prov_dir and to_abs.startswith(prov_dir) else "false", "package dir matches; entry not resolvable by node"


_meta = {}


def cargo_meta(root):
    """Cargo's full resolution (`cargo metadata`, with `[patch]` applied; nothing is compiled)."""
    if root not in _meta:
        r = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--all-features",
             "--manifest-path", os.path.join(root, "Cargo.toml")],
            capture_output=True, text=True, timeout=600,
        )
        _meta[root] = json.loads(r.stdout) if r.returncode == 0 else {"packages": [], "resolve": None}
    return _meta[root]


def judge_cargo(e, roots):
    frm, to = file_of(e["source"]), file_of(e["target"])
    fl, frel, froot = split(frm, roots)
    tl, trel, troot = split(to, roots)
    from_abs = os.path.realpath(os.path.join(froot, frel))
    to_abs = os.path.realpath(os.path.join(troot, trel))
    meta = cargo_meta(froot)
    pkgs = [p for p in meta["packages"] if p.get("source") is None]  # local packages only
    def manifest_dir(p):
        return os.path.realpath(os.path.dirname(p["manifest_path"]))
    owner = max(
        (p for p in pkgs if from_abs.startswith(manifest_dir(p) + os.sep)),
        key=lambda p: len(manifest_dir(p)), default=None,
    )
    prov = next((p for p in pkgs if any(
        "lib" in t.get("kind", []) and os.path.realpath(t["src_path"]) == to_abs
        for t in p["targets"])), None)
    if owner is None or prov is None:
        return "false", "cargo metadata places no local package at one end"
    node = next((n for n in (meta.get("resolve") or {}).get("nodes", []) if n["id"] == owner["id"]), None)
    if node is None:
        return "false", "cargo metadata has no resolution for the importing package"
    if prov["id"] in node.get("dependencies", []):
        return "true", f"cargo resolves {owner['name']} -> {prov['name']} (local)"
    return "false", f"cargo does not resolve {owner['name']} to the local {prov['name']}"


def main():
    db, case = sys.argv[1], sys.argv[2]
    roots = dict(a.split("=", 1) for a in sys.argv[3:])
    rows = []
    for e in edges(db):
        md = e.get("metadata", {})
        eco = md.get("link.ecosystem")
        if eco == "npm":
            lab, why = judge_npm(e, roots)
        elif eco == "cargo":
            lab, why = judge_cargo(e, roots)
        else:
            lab, why = "unjudged", f"no oracle for {eco}"
        if lab == "unjudged":
            continue
        rows.append({
            "case": case, "from_file": file_of(e["source"]), "to_file": file_of(e["target"]),
            "identity": md.get("link.identity"), "package": md.get("link.package"),
            "label": lab, "reason": why,
        })
    json.dump(rows, sys.stdout, indent=1)


if __name__ == "__main__":
    main()
