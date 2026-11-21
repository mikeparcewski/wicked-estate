#!/usr/bin/env python3
"""Bump the workspace version in the root Cargo.toml (used by release.yml, wicked-estate#479).

Rewrites exactly two things: `version` under `[workspace.package]` and the `version` pin of each
internal `wicked-estate-*` path dependency. Third-party pins (`zip = { version = "9.0.5" }`) and the
independently versioned vendored grammars are never touched.

Usage: bump_workspace_version.py <X.Y.Z> [path/to/Cargo.toml]
"""
import re
import sys
from pathlib import Path

SEMVER = r"\d+\.\d+\.\d+"
SECTION = re.compile(r"^\[([^\]]+)\]\s*(?:#.*)?$")
WORKSPACE_VERSION = re.compile(rf'^(version\s*=\s*)"{SEMVER}"')
INTERNAL_PIN = re.compile(
    rf'^(wicked-estate-(?!tree-sitter-)[\w-]+\s*=\s*\{{(?=[^}}\n]*\bpath\s*=)[^}}\n]*\bversion\s*=\s*)"{SEMVER}"'
)


def bump(text: str, new_ver: str) -> str:
    section = None
    out = []
    for line in text.splitlines(keepends=True):
        m = SECTION.match(line.strip())
        if m:
            section = m.group(1)
        elif section == "workspace.package":
            line = WORKSPACE_VERSION.sub(lambda m: f'{m.group(1)}"{new_ver}"', line)
        elif section is not None and section.endswith("dependencies"):
            line = INTERNAL_PIN.sub(lambda m: f'{m.group(1)}"{new_ver}"', line)
        out.append(line)
    return "".join(out)


def main() -> int:
    if len(sys.argv) not in (2, 3) or not re.fullmatch(SEMVER, sys.argv[1]):
        print(__doc__, file=sys.stderr)
        return 2
    p = Path(sys.argv[2] if len(sys.argv) == 3 else "Cargo.toml")
    text = p.read_text(encoding="utf-8")
    updated = bump(text, sys.argv[1])
    if updated == text:
        print(f"WARNING: nothing to change in {p} (already {sys.argv[1]}?)")
    p.write_text(updated, encoding="utf-8")
    print(f"Bumped workspace version and internal pins to {sys.argv[1]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
