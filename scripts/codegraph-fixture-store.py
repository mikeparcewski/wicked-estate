#!/usr/bin/env python3
"""The codegraph gate store (estate#595, HOLD gate b of #584).

    codegraph-fixture-store.py grow <path>
        Write the gate fixture's oversized TS file (past codegraph's 1 MiB source cap) to <path>.
        regen.sh records it, and the Rust tests write the same bytes (codegraph::coverage_tests).

    codegraph-fixture-store.py store <wicked-estate binary> <out dir>
        Index a copy of tests/fixtures/codegraph/coverage/repo with `--codegraph`, the recorded
        codegraph database standing in for the run, into <out dir>/codegraph-gate.db. The
        recording is followed by synthetic-kinds.sql (the edge kinds codegraph 1.6.4 never emits,
        labelled `resolvedBy: synthetic`), so the store holds every kind estate maps. No ambient
        extraction plugin takes part (an empty plugin dir, no override). The binary
        must be a test build (`--features codegraph-fixture`); a release binary has no hook and
        the run fails here. Consumers' contract tests read the store through
        WICKED_CODEGRAPH_FIXTURE_STORE.

One script for every OS (python3 on every runner).
"""
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
FIXTURE = REPO / "crates/wicked-estate/tests/fixtures/codegraph/coverage"
BIG = "src/ts/big.ts"


def big_text() -> str:
    line = "// codegraph caps a source file at 1 MiB; this one is bigger.\n"
    n = (1024 * 1024) // len(line) + 1
    return "export const big = 0;\n" + line * n


def grow(path: Path) -> None:
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(big_text())


def store(binary: Path, out: Path) -> Path:
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    tree = out / "repo"
    shutil.copytree(FIXTURE / "repo", tree)
    grow(tree / BIG)
    home = out / "home"
    home.mkdir()
    db = out / "codegraph-gate.db"
    plugins = out / "no-plugins"
    plugins.mkdir()
    recorded = out / "recorded.sql"
    recorded.write_text(
        (FIXTURE / "codegraph-1.6.4.sql").read_text(encoding="utf-8") + "\n"
        + (FIXTURE / "synthetic-kinds.sql").read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    env = {k: v for k, v in os.environ.items()
           if k not in ("WICKED_ESTATE_DB", "WICKED_STORE_URL", "WICKED_RUNTIME", "WICKED_OTEL_ENDPOINT",
                        "WICKED_ESTATE_PLUGIN_OVERRIDE")}
    env["WICKED_HOME"] = str(home)
    env["WICKED_ESTATE_PLUGINS"] = str(plugins)
    env["WICKED_ESTATE_EMIT_PROGRAM"] = "wicked-bus-absent-codegraph-gate"
    env["WICKED_ESTATE_EMIT_DEADLETTER"] = str(out / "emit.ndjson")
    env["WICKED_ESTATE_CODEGRAPH_RECORDED"] = str(recorded)
    run = subprocess.run(
        [str(binary), "index", str(tree), "--db", str(db), "--codegraph", str(out / "no-bundle")],
        capture_output=True, text=True, env=env, timeout=600,
    )
    sys.stderr.write(run.stderr)
    if run.returncode != 0:
        sys.exit(f"codegraph gate store: index exited {run.returncode}")
    if "CODEGRAPH-FIXTURE:" not in run.stderr or "CODEGRAPH-FALLBACK" in run.stderr:
        sys.exit("codegraph gate store: the run did not read the recorded database "
                 "(a release binary, or the fixture fell back)")
    return db


def main() -> None:
    args = sys.argv[1:]
    if len(args) == 2 and args[0] == "grow":
        grow(Path(args[1]))
    elif len(args) == 3 and args[0] == "store":
        print(store(Path(args[1]), Path(args[2])))
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
