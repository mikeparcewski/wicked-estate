#!/usr/bin/env python3
"""#322: regenerate every vendored tree-sitter parser with the one pinned CLI.

Each parser is generated from its `src/grammar.json`, which is data: no JavaScript runs (the CLI's
JavaScript runtime is set to a name that does not exist, so a `.js` grammar could not be loaded
even by mistake). An in-house tree, one whose PROVENANCE row says "In-house", is first generated
from its own `grammar.js`, so `grammar.json` is checked against the source we author. Upstream
`grammar.js` files (tree-sitter-cfml) are never run.

CI runs this and then `git diff --exit-code`: a committed parser that is not exactly what its
grammar generates fails the build.

    python3 scripts/regen-vendored-parsers.py            # CLI via `npx --yes tree-sitter-cli@PIN`
    TREE_SITTER_CLI=path/to/tree-sitter python3 scripts/regen-vendored-parsers.py
"""

import os
from pathlib import Path
import shlex
import subprocess
import sys

# The one pinned CLI. 0.25.x emits ABI 15, which the workspace runtime (`tree-sitter = "0.25"`)
# loads. Change it only in a PR that regenerates every parser (PROVENANCE.md, "Updating").
PIN = "0.25.10"
ROOT = Path(__file__).resolve().parents[1]
VENDOR = ROOT / "crates/wicked-estate-extract/vendor"
NO_JS_RUNTIME = "wicked-no-javascript-runtime"
# Trees whose committed parser is still upstream's, because regenerating it changes the parse
# tables; each is its own PR, gated on the extraction output, which deletes its entry. Nothing else
# may be added here (scripts/test_parser_regeneration.py).
NOT_YET_REGENERATED = {
    "tree-sitter-vbscript": "upstream's parser.c is from an older CLI; regenerating it changes its parse tables (#322)",
}


def grammar_dirs(vendor=VENDOR):
    """Every directory holding a generated grammar (`src/grammar.json`), sorted."""
    return sorted(p.parent.parent for p in vendor.glob("**/src/grammar.json"))


def regenerated_dirs(vendor=VENDOR):
    """The grammars this script regenerates: all but NOT_YET_REGENERATED."""
    return [d for d in grammar_dirs(vendor) if d.relative_to(vendor).parts[0] not in NOT_YET_REGENERATED]


def orphan_parsers(vendor=VENDOR):
    """Committed parsers this script would not regenerate: every `parser.c` must be
    `<grammar>/src/parser.c` beside a `src/grammar.json`, or CI could not prove its bytes."""
    return sorted(str(p.relative_to(vendor)) for p in vendor.glob("**/parser.c")
                  if not (p.parent.name == "src" and (p.parent / "grammar.json").is_file()))


def in_house_trees(vendor=VENDOR):
    """Trees whose PROVENANCE row says they were authored here: their grammar.js is ours to run."""
    trees = set()
    for line in (vendor / "PROVENANCE.md").read_text().splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) == 7 and cells[1].startswith("`tree-sitter-") and cells[2].startswith("In-house"):
            trees.add(cells[1].strip("`/"))
    return trees


def has_config(grammar_dir, vendor=VENDOR):
    """The CLI needs a tree-sitter.json (here or in the tree's root) to emit ABI 15 and metadata;
    without one it silently falls back to ABI 14."""
    tree = vendor / grammar_dir.relative_to(vendor).parts[0]
    return (grammar_dir / "tree-sitter.json").is_file() or (tree / "tree-sitter.json").is_file()


def cli():
    override = os.environ.get("TREE_SITTER_CLI")
    return shlex.split(override) if override else ["npx", "--yes", f"tree-sitter-cli@{PIN}"]


def run(cmd, cwd):
    print(f"$ (cd {cwd.relative_to(ROOT)} && {shlex.join(cmd)})", flush=True)
    subprocess.run(cmd, cwd=cwd, check=True)


def main():
    ts = cli()
    version = subprocess.run(ts + ["--version"], capture_output=True, text=True, check=True).stdout.split()
    if version[:2] != ["tree-sitter", PIN]:
        sys.exit(f"tree-sitter CLI is {' '.join(version[:2])!r}; this repository pins {PIN}")
    orphans = orphan_parsers()
    if orphans:
        sys.exit(f"committed parsers with no src/grammar.json to regenerate them from: {orphans}")
    in_house = in_house_trees()
    dirs = regenerated_dirs()
    missing = [str(d.relative_to(ROOT)) for d in dirs if not has_config(d)]
    if missing:
        sys.exit(f"no tree-sitter.json for {missing}: the CLI would fall back to ABI 14")
    for d in dirs:
        tree = d.relative_to(VENDOR).parts[0]
        if tree in in_house and (d / "grammar.js").is_file():
            run(ts + ["generate"], d)  # our grammar.js -> grammar.json (+ parser.c)
        run(ts + ["generate", "--js-runtime", NO_JS_RUNTIME, "src/grammar.json"], d)
    print(f"regenerated {len(dirs)} parsers with tree-sitter {PIN}; not yet: {sorted(NOT_YET_REGENERATED)}")


if __name__ == "__main__":
    main()
