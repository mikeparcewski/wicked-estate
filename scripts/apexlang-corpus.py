#!/usr/bin/env python3
"""Pinned APEXlang corpus (estate#310): fetch it, verify it, or rebuild its manifest.

The full corpus is Oracle's generated APEXlang applications, UPL-1.0, at pinned commits:
  - oracle/apex       fbe095e4c6e783b3fa930ffae927e4826e549190  every *.apx (2,961 files)
  - oracle/skills     014cdb29f12dfa6a689bbe8a80ee1c5f0bf15be1  the example applications (199 files)
It is too large to vendor, so `oracle/apexlang/corpus.tsv` lists every file with its sha256 and
this script fetches the files at those commits and checks each one. A missing file, an extra file
or a sha256 mismatch fails loudly (exit 1).

  apexlang-corpus.py fetch <dest> [--with-sql-exports]
                                           sparse-clone both repos into <dest> and verify
  apexlang-corpus.py verify <dest>         verify an existing <dest>
  apexlang-corpus.py manifest <dest>       rewrite corpus.tsv and apps.tsv from <dest> (maintainers)

Needs git and python3 only. `<dest>/<repo>/...` mirrors each repository's own paths.
"""
import hashlib
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ORACLE = os.path.join(HERE, "..", "oracle", "apexlang")
MANIFEST = os.path.join(ORACLE, "corpus.tsv")
APPS = os.path.join(ORACLE, "apps.tsv")

SOURCES = {
    "oracle-apex": {
        "url": "https://github.com/oracle/apex.git",
        "commit": "fbe095e4c6e783b3fa930ffae927e4826e549190",
        "patterns": ["*.apx", "**/.apex/apexlang.json", "/LICENSE.txt"],
        # `--with-sql-exports`: Oracle's legacy SQL exports of the same applications, for the
        # precision oracle (oracle/apexlang/label.py). Not part of the parse corpus or its manifest.
        "sql_patterns": [
            "/sample-apps/*/sql/*.sql",
            "/starter-apps/*/sql/*.sql",
            "/utility-apps/*/sql/*.sql",
        ],
        "prefix": "",
    },
    "oracle-skills": {
        "url": "https://github.com/oracle/skills.git",
        "commit": "014cdb29f12dfa6a689bbe8a80ee1c5f0bf15be1",
        "patterns": [
            "/apex/apexlang/26.1/apexlang-example-applications/assets/**",
            "/LICENSE.txt",
        ],
        "prefix": "apex/apexlang/26.1/apexlang-example-applications/assets/",
    },
}


def git(*args, cwd=None):
    env = dict(os.environ, GIT_TERMINAL_PROMPT="0")
    subprocess.run(["git", "-c", "core.fsmonitor=false", *args], cwd=cwd, check=True, env=env)


def fetch(dest, with_sql=False):
    os.makedirs(dest, exist_ok=True)
    for name, src in SOURCES.items():
        repo = os.path.join(dest, name)
        if not os.path.isdir(os.path.join(repo, ".git")):
            git("clone", "--quiet", "--filter=blob:none", "--no-checkout", src["url"], repo)
        patterns = src["patterns"] + (src.get("sql_patterns", []) if with_sql else [])
        git("sparse-checkout", "set", "--no-cone", *patterns, cwd=repo)
        git("checkout", "--quiet", "--detach", src["commit"], cwd=repo)


def corpus_files(dest):
    """(repo, path) of every pinned file under dest: .apx, apexlang.json and the licence."""
    out = []
    for name, src in SOURCES.items():
        root = os.path.join(dest, name)
        for d, dirs, files in os.walk(root):
            dirs[:] = [x for x in dirs if x != ".git"]
            for f in files:
                rel = os.path.relpath(os.path.join(d, f), root).replace(os.sep, "/")
                keep = rel == "LICENSE.txt" or (
                    rel.startswith(src["prefix"])
                    and (rel.endswith(".apx") or rel.endswith("/.apex/apexlang.json"))
                )
                if keep:
                    out.append((name, rel))
    return sorted(out)


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 16), b""):
            h.update(chunk)
    return h.hexdigest()


def read_manifest():
    rows = {}
    with open(MANIFEST, encoding="utf-8") as fh:
        for line in fh:
            if line.startswith("#") or not line.strip():
                continue
            repo, commit, path, digest, size = line.rstrip("\n").split("\t")
            if SOURCES[repo]["commit"] != commit:
                sys.exit(f"manifest pins {repo}@{commit}, script pins {SOURCES[repo]['commit']}")
            rows[(repo, path)] = (digest, int(size))
    return rows


def verify(dest):
    want = read_manifest()
    have = set(corpus_files(dest))
    bad = []
    for key in sorted(set(want) - have):
        bad.append(f"MISSING {key[0]}/{key[1]}")
    for key in sorted(have - set(want)):
        bad.append(f"EXTRA {key[0]}/{key[1]}")
    for key in sorted(have & set(want)):
        path = os.path.join(dest, key[0], key[1])
        digest, size = want[key]
        got = sha256(path)
        if got != digest or os.path.getsize(path) != size:
            bad.append(f"SHA256 MISMATCH {key[0]}/{key[1]}: want {digest}, got {got}")
    apx = sum(1 for k in want if k[1].endswith(".apx"))
    if bad:
        for b in bad:
            print(b, file=sys.stderr)
        sys.exit(f"apexlang corpus: {len(bad)} problem(s) against {len(want)} pinned files")
    print(f"apexlang corpus OK: {len(want)} files ({apx} .apx) match corpus.tsv byte for byte")


def manifest(dest):
    rows = corpus_files(dest)
    with open(MANIFEST, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("# APEXlang corpus pin (estate#310). UPL-1.0, (c) Oracle and/or its affiliates.\n")
        fh.write("# repo\tcommit\tpath\tsha256\tbytes\n")
        for repo, path in rows:
            full = os.path.join(dest, repo, path)
            fh.write(f"{repo}\t{SOURCES[repo]['commit']}\t{path}\t{sha256(full)}\t{os.path.getsize(full)}\n")
    with open(APPS, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("# One row per application directory: repo, path, .apx files, mmdVersion.\n")
        fh.write("# repo\tapp_dir\tapx_files\tmmd_version\n")
        for repo, path in rows:
            if path.endswith("/application.apx") or path == "application.apx":
                app = path[: -len("application.apx")].rstrip("/")
                meta = os.path.join(dest, repo, app, ".apex", "apexlang.json")
                try:
                    with open(meta, encoding="utf-8") as m:
                        ver = json.load(m).get("mmdVersion", "unknown")
                except OSError:
                    ver = "unknown"
                n = sum(1 for r, p in rows if r == repo and p.startswith(app + "/") and p.endswith(".apx"))
                fh.write(f"{repo}\t{app}\t{n}\t{ver}\n")
    print(f"wrote {len(rows)} rows to corpus.tsv")


def main():
    args = [a for a in sys.argv[1:] if a != "--with-sql-exports"]
    with_sql = len(args) != len(sys.argv) - 1
    if len(args) != 2 or args[0] not in {"fetch", "verify", "manifest"}:
        sys.exit(__doc__)
    cmd, dest = args[0], os.path.abspath(args[1])
    if cmd == "fetch":
        fetch(dest, with_sql)
        verify(dest)
    elif cmd == "verify":
        verify(dest)
    else:
        manifest(dest)


if __name__ == "__main__":
    main()
