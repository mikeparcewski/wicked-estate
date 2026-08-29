#!/usr/bin/env python3
"""W8.3 — Generate docs/language-coverage-matrix.md from code-driven sources.

Reads two authoritative sources and cross-references them:

1. `crates/wicked-estate-extract/languages.toml`  — the 73-language aspirational manifest
   (tier, caps, extensions). This is the data row added for every language;
   it is the code-driven capability matrix prior art issue asked for but
   never built (they maintained it by hand; we generate it from data).

2. `crates/wicked-estate-extract/src/treesitter.rs` — the wired LANG_TABLE. A language
   is "wired" only when its grammar crate is compiled in and its .scm file is
   embedded. The IaCExtractor handles `cloudformation` and `kubernetes` as a
   special case (YAML grammar, tree-walk logic, no .scm file).

Outputs:
    docs/language-coverage-matrix.md

Usage:
    python3 scripts/gen-coverage-matrix.py
    python3 scripts/gen-coverage-matrix.py --check   # exit 1 if output is stale
"""

import os
import re
import sys
import tomllib  # stdlib in Python ≥ 3.11; fallback handled below

REPO_ROOT = os.path.normpath(os.path.join(os.path.dirname(__file__), ".."))
TOML_PATH = os.path.join(REPO_ROOT, "crates", "wicked-estate-extract", "languages.toml")
TS_RS_PATH = os.path.join(REPO_ROOT, "crates", "wicked-estate-extract", "src", "treesitter.rs")
OUT_PATH = os.path.join(REPO_ROOT, "docs", "language-coverage-matrix.md")


# ── TOML loader fallback for Python < 3.11 ───────────────────────────────────

def load_toml(path):
    try:
        import tomllib as _tomllib
        with open(path, "rb") as f:
            return _tomllib.load(f)
    except ImportError:
        pass
    try:
        import tomli as _tomllib  # optional third-party fallback
        with open(path, "rb") as f:
            return _tomllib.load(f)
    except ImportError:
        pass
    # Manual minimal parser — only handles the simple table-array TOML we have.
    return _parse_toml_minimal(path)


def _parse_toml_minimal(path):
    """Parse the specific languages.toml format without a TOML library."""
    languages = []
    current = None
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line == "[[language]]":
                if current is not None:
                    languages.append(current)
                current = {}
            elif current is not None and "=" in line and not line.startswith("#"):
                key, _, val = line.partition("=")
                key = key.strip()
                val = val.strip()
                if val.startswith('"') and val.endswith('"'):
                    current[key] = val[1:-1]
                elif val.startswith("[") and val.endswith("]"):
                    inner = val[1:-1].strip()
                    if inner:
                        current[key] = [
                            item.strip().strip('"')
                            for item in inner.split(",")
                            if item.strip()
                        ]
                    else:
                        current[key] = []
        if current is not None:
            languages.append(current)
    return {"language": languages}


# ── Wired language detection from treesitter.rs ──────────────────────────────

def extract_wired_languages(rs_path):
    """Return the set of language names in LANG_TABLE in treesitter.rs.

    Parses the static LANG_TABLE array by looking for `name: "<lang>"` entries
    between the `static LANG_TABLE` declaration and the next top-level item.
    Also adds cloudformation and kubernetes from IaCExtractor (hard-wired, no
    LANG_TABLE entry).
    """
    with open(rs_path, encoding="utf-8") as f:
        src = f.read()

    # Find LANG_TABLE block
    table_start = src.find("static LANG_TABLE:")
    if table_start == -1:
        # Fallback: search for LangEntry patterns anywhere
        table_start = 0

    # Find the end: next `static` or `pub struct` or `impl ` at column 0
    # after LANG_TABLE starts — look for the closing `];`
    table_end = src.find("];", table_start)
    if table_end == -1:
        table_end = len(src)
    else:
        table_end += 2  # include the ];

    table_block = src[table_start:table_end]

    # Extract name: "<lang>" values
    names = re.findall(r'name:\s*"([^"]+)"', table_block)
    wired = set(names)

    # IaCExtractor: handles cloudformation and kubernetes via tree-walk (no LANG_TABLE)
    # Verified from the IaCExtractor::for_language match arm in treesitter.rs
    if 'IaCExtractor' in src or 'cloudformation' in src:
        wired.add("cloudformation")
        wired.add("kubernetes")

    return wired


# ── Tier ordering and display ─────────────────────────────────────────────────

TIER_ORDER = {"precise": 0, "structural": 1, "tags": 2, "detected": 3, "document": 4}

TIER_DESC = {
    "precise":    "precise",
    "structural": "structural",
    "tags":       "tags",
    "detected":   "detected",
    "document":   "document",
}

CAP_SYMBOLS = {
    "symbols":    "S",
    "calls":      "C",
    "imports":    "I",
    "extends":    "E",
    "implements": "P",
}


def caps_display(caps):
    """Return a compact capability string like S·C·I·E."""
    parts = [CAP_SYMBOLS[c] for c in ["symbols", "calls", "imports", "extends", "implements"] if c in caps]
    return "·".join(parts) if parts else "—"


# ── Markdown generation ───────────────────────────────────────────────────────

def generate_matrix(languages, wired_names):
    lines = []
    lines.append("<!-- AUTO-GENERATED by scripts/gen-coverage-matrix.py — do not edit by hand -->")
    lines.append("")
    lines.append("# Language Coverage Matrix")
    lines.append("")
    lines.append("Generated from `crates/wicked-estate-extract/languages.toml` (aspirational manifest) cross-referenced")
    lines.append("against `crates/wicked-estate-extract/src/treesitter.rs` `LANG_TABLE` (what is actually wired).")
    lines.append("")
    lines.append("This is the code-driven capability matrix prior art issue asked for but never built")
    lines.append("(they maintained it by hand). Here it is generated from data — a new language is one row")
    lines.append("in `languages.toml` + a `.scm` query file + a `LANG_TABLE` entry; this table regenerates.")
    lines.append("")
    lines.append("## Capability key")
    lines.append("")
    lines.append("| Symbol | Meaning |")
    lines.append("|--------|---------|")
    lines.append("| S | symbols (definitions, classes, functions, …) |")
    lines.append("| C | calls (call-graph edges) |")
    lines.append("| I | imports (import/require/use edges) |")
    lines.append("| E | extends (class hierarchy — `extends`) |")
    lines.append("| P | implements (interface implementation) |")
    lines.append("")
    lines.append("## Tier key")
    lines.append("")
    lines.append("| Tier | Meaning |")
    lines.append("|------|---------|")
    lines.append("| `document` | Config/markup — symbols only (YAML, JSON, HTML, HCL, …). |")
    lines.append("| `tags`     | Code — symbol definitions extractable by tree-sitter tags queries. |")
    lines.append("| `structural` | Code — symbols + calls + imports + heritage from `.scm` queries. |")
    lines.append("| `precise`  | Structural + cross-file resolved refs from SCIP / TSG / on-demand LSP. |")
    lines.append("")
    lines.append("Note: `extends`/`implements` in the manifest reflect what the `.scm` query captures")
    lines.append("(tree-sitter, intra-file). Cross-file resolution of heritage requires the `precise` tier")
    lines.append("(SCIP/TSG/LSP — Waves W2.2/W3.2/W3.3).")
    lines.append("")

    # Wired / unwired summary
    wired_count = sum(1 for lang in languages if lang["name"] in wired_names)
    total = len(languages)
    lines.append(f"**{total} languages in manifest · {wired_count} wired (extractor present) · "
                 f"{total - wired_count} aspirational (manifest row, extractor pending)**")
    lines.append("")

    # Separate IaC entries (cloudformation/kubernetes not in languages.toml)
    iac_only = [n for n in wired_names if n not in {lang["name"] for lang in languages}]

    lines.append("## Full matrix")
    lines.append("")
    lines.append("| Language | Wired? | Tier | Capabilities | Extensions |")
    lines.append("|----------|:------:|------|:------------:|------------|")

    # Sort: wired first (by tier order then name), then unwired alphabetically
    def sort_key(lang):
        is_wired = lang["name"] in wired_names
        tier_ord = TIER_ORDER.get(lang.get("tier", "tags"), 99)
        return (0 if is_wired else 1, tier_ord, lang["name"])

    sorted_langs = sorted(languages, key=sort_key)

    for lang in sorted_langs:
        name = lang["name"]
        wired = "yes" if name in wired_names else "no"
        tier = lang.get("tier", "tags")
        caps = lang.get("caps", [])
        exts = lang.get("ext", [])
        ext_str = ", ".join(f"`.{e}`" for e in exts) if exts else "—"
        caps_str = caps_display(caps)
        lines.append(f"| `{name}` | {wired} | `{tier}` | {caps_str} | {ext_str} |")

    # Languages wired but not yet in the TOML manifest
    # cloudformation/kubernetes: IaCExtractor (YAML grammar + tree-walk, no .scm)
    iac_names = {"cloudformation", "kubernetes"}
    manifest_only_extra = [n for n in iac_only if n not in iac_names]
    iac_extra = [n for n in iac_only if n in iac_names]

    if iac_extra:
        lines.append("")
        lines.append("### IaC extractors (wired via `IaCExtractor`, not in `languages.toml`)")
        lines.append("")
        lines.append("These use the tree-sitter-yaml grammar with a dedicated tree-walk (no `.scm` file).")
        lines.append("Resources become `NodeKind::Other(\"resource\")` nodes; `depends_on`/`!Ref` become edges.")
        lines.append("")
        lines.append("| Language | Wired? | Tier | Capabilities | Extensions |")
        lines.append("|----------|:------:|------|:------------:|------------|")
        for name in sorted(iac_extra):
            if name == "cloudformation":
                lines.append("| `cloudformation` | yes | `structural` | S·C | `.yaml`, `.yml`, `.json` |")
            elif name == "kubernetes":
                lines.append("| `kubernetes` | yes | `structural` | S | `.yaml`, `.yml` |")
            else:
                lines.append(f"| `{name}` | yes | `structural` | S | — |")

    if manifest_only_extra:
        lines.append("")
        lines.append("### Wired but missing from `languages.toml`")
        lines.append("")
        lines.append("These languages have a LANG_TABLE entry + `.scm` file but no manifest row yet.")
        lines.append("Add a `[[language]]` row to `languages.toml` to complete the registration.")
        lines.append("")
        lines.append("| Language | Wired? | Tier | Capabilities | Extensions |")
        lines.append("|----------|:------:|------|:------------:|------------|")
        for name in sorted(manifest_only_extra):
            lines.append(f"| `{name}` | yes | `tags` | S | — |")

    # Wired-only summary table
    lines.append("")
    lines.append("## Wired languages summary")
    lines.append("")
    lines.append("Languages with an active extractor (tree-sitter grammar compiled in, `.scm` embedded).")
    lines.append("")
    lines.append("| Language | Tier | Capabilities | Extensions |")
    lines.append("|----------|------|:------------:|------------|")

    wired_langs = [lang for lang in sorted_langs if lang["name"] in wired_names]
    for lang in wired_langs:
        name = lang["name"]
        tier = lang.get("tier", "tags")
        caps = lang.get("caps", [])
        exts = lang.get("ext", [])
        ext_str = ", ".join(f"`.{e}`" for e in exts) if exts else "—"
        caps_str = caps_display(caps)
        lines.append(f"| `{name}` | `{tier}` | {caps_str} | {ext_str} |")

    for name in sorted(iac_only):
        if name == "cloudformation":
            lines.append("| `cloudformation` | `structural` | S·C | `.yaml`, `.yml`, `.json` |")
        elif name == "kubernetes":
            lines.append("| `kubernetes` | `structural` | S | `.yaml`, `.yml` |")

    lines.append("")
    lines.append("## ABI note")
    lines.append("")
    lines.append("All wired grammars use **ABI 14** (tree-sitter 0.24 supports ABI 13–14).")
    lines.append("Grammars at ABI 15 (`tree-sitter-go` ≥0.25, `tree-sitter-bash` ≥0.25,")
    lines.append("`tree-sitter-javascript` ≥0.25, `tree-sitter-c` 0.24.x,")
    lines.append("`tree-sitter-c-sharp` ≥0.23.x, `tree-sitter-hcl` any version) were dropped.")
    lines.append("The `≥73` parity test gates regressions. Upgrade these entries when tree-sitter 0.25")
    lines.append("is adopted workspace-wide.")
    lines.append("")

    return "\n".join(lines) + "\n"


# ── CLI ───────────────────────────────────────────────────────────────────────

def main():
    check_mode = "--check" in sys.argv

    manifest = load_toml(TOML_PATH)
    languages = manifest.get("language", [])
    wired_names = extract_wired_languages(TS_RS_PATH)

    content = generate_matrix(languages, wired_names)

    if check_mode:
        try:
            with open(OUT_PATH, encoding="utf-8") as f:
                existing = f.read()
            if existing == content:
                print(f"ok: {OUT_PATH} is up to date")
                sys.exit(0)
            else:
                print(f"STALE: {OUT_PATH} does not match generated output — re-run without --check")
                sys.exit(1)
        except FileNotFoundError:
            print(f"MISSING: {OUT_PATH} — run without --check to generate")
            sys.exit(1)

    os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
    with open(OUT_PATH, "w", encoding="utf-8") as f:
        f.write(content)

    wired_count = sum(1 for lang in languages if lang["name"] in wired_names)
    iac_only = [n for n in wired_names if n not in {lang["name"] for lang in languages}]

    print(f"wrote {OUT_PATH}")
    print(f"  manifest: {len(languages)} languages")
    print(f"  wired (LANG_TABLE): {wired_count} from manifest + {len(iac_only)} IaC-only")
    print(f"  wired names: {', '.join(sorted(lang['name'] for lang in languages if lang['name'] in wired_names) + sorted(iac_only))}")


if __name__ == "__main__":
    main()
