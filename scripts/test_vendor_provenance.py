"""#322: every grammar vendored as source has a PROVENANCE row and a NOTICE entry."""

from pathlib import Path
import re
import shutil
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
VENDOR = ROOT / "crates/wicked-estate-extract/vendor"


def provenance_rows(vendor):
    """(subpath, source, license) per table row of PROVENANCE.md."""
    rows = []
    for line in (vendor / "PROVENANCE.md").read_text().splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) == 7 and re.fullmatch(r"`[^`]+/`", cells[1]):
            rows.append((cells[1].strip("`/"), cells[2], cells[5]))
    return rows


def vendored_trees(vendor):
    return sorted(p.name for p in vendor.iterdir() if p.is_dir() and p.name.startswith("tree-sitter-"))


def provenance_problems(vendor, notice):
    """Every reason the vendored trees are not fully recorded and attributed; empty when they are."""
    problems = []
    trees = vendored_trees(vendor)
    rows = provenance_rows(vendor)
    if sorted(sub for sub, _, _ in rows) != trees:
        problems.append(f"PROVENANCE rows {sorted(sub for sub, _, _ in rows)} != trees {trees}")
    noticed = sorted(set(re.findall(r"crates/wicked-estate-extract/vendor/(tree-sitter-[\w-]+)/", notice)))
    if noticed != trees:
        problems.append(f"NOTICE paths {noticed} != trees {trees}")
    # A tree carries its own LICENSE when it is third-party or its row names one (APEXlang: authored
    # here, but under Oracle's UPL). Only trees covered by this repository's LICENSE alone are exempt.
    own_license = {sub for sub, source, lic in rows
                   if not source.startswith("In-house") or f"`{sub}/LICENSE`" in lic}
    for tree in trees:
        license_file = vendor / tree / "LICENSE"
        lines = license_file.read_text().splitlines() if license_file.exists() else []
        copyrights = [line.strip() for line in lines if line.strip().startswith("Copyright (c)")]
        # MIT requires the copyright notice to travel with the binary; NOTICE ships in every archive.
        if tree in own_license and not copyrights:
            problems.append(f"{tree}: no LICENSE copyright line, but its row needs one")
        problems += [f"{tree}: {c!r} missing from NOTICE" for c in copyrights if c not in notice]
    return problems


# The intake record (#322): what each tree's upstream declares, read at the upstream commit whose
# files the vendored ones match by git blob. A copyright line is listed only when upstream states
# one; we never supply a line upstream does not state. Add an entry when you vendor upstream code.
UPSTREAM_DECLARATIONS = {
    "tree-sitter-cfml": {
        # LICENSE (blob-identical to ours); package.json, Cargo.toml, tree-sitter.json: MIT.
        "repo": "https://github.com/cfmleditor/tree-sitter-cfml",
        "commit": "6b4a43943c191950ee2c77f7b94cec5e903318a4",
        "licenses": ("MIT",),
        "spdx": "MIT",
        "copyrights": ("Copyright (c) 2014 Gareth Edwards & Gavin Baumanis",),
        "author": None,
    },
    "tree-sitter-vb6": {
        # No LICENSE file. package.json, Cargo.toml, pyproject.toml, tree-sitter.json: MIT, author
        # "joanne".
        "repo": "https://github.com/joannefan/tree-sitter-vb6",
        "commit": "1ee443353ce49f12e39a8e627b557c59e348b729",
        "licenses": ("MIT",),
        "spdx": "MIT",
        "copyrights": (),
        "author": "joanne",
    },
    "tree-sitter-vba": {
        # No LICENSE file. package.json, Cargo.toml, tree-sitter.json: MIT; tree-sitter.json
        # author "Todd Epple".
        "repo": "https://github.com/tmepple/tree-sitter-vba",
        "commit": "c691f237b2a703732d4b6a1f01d5b4f73f94d41e",
        "licenses": ("MIT",),
        "spdx": "MIT",
        "copyrights": (),
        "author": "Todd Epple",
    },
    "tree-sitter-vbscript": {
        # No LICENSE file. package.json: ISC, author "Jan-Jaap Korpershoek"; Cargo.toml and
        # pyproject.toml: MIT. The declarations conflict, so the tree ships under both.
        "repo": "https://github.com/JJK96/tree-sitter-vbscript",
        "commit": "6d9548eab0514e0572a616fd58d5997a93b37415",
        "licenses": ("MIT", "ISC"),
        "spdx": "MIT AND ISC",
        "copyrights": (),
        "author": "Jan-Jaap Korpershoek",
    },
    "tree-sitter-cobol-scanner": {
        # LICENSE at that commit (the vendored scanner comes through the arborium-cobol crate).
        "repo": "https://github.com/zharinov/tree-sitter-cobol",
        "commit": "befb226951b65585617f42ecbe526d00c124100c",
        "licenses": ("MIT",),
        "spdx": None,
        "copyrights": ("Copyright (c) 2016 Max Brunsfeld",),
        "author": None,
    },
    "tree-sitter-apexlang": {
        # LICENSE.txt at that commit; the grammar is authored here from its EBNF.
        "repo": "https://github.com/oracle/skills",
        "commit": "014cdb29f12dfa6a689bbe8a80ee1c5f0bf15be1",
        "licenses": ("Universal Permissive License (UPL), Version 1.0",),
        "spdx": None,
        "copyrights": ("Copyright (c) 2025 Oracle and/or its affiliates.",),
        "author": None,
    },
}

NO_COPYRIGHT = "states no copyright notice"


def notice_block(notice, tree):
    """The NOTICE entry (between separator lines) that names `tree`'s path."""
    path = f"crates/wicked-estate-extract/vendor/{tree}/"
    for block in re.split(r"\n-{20,}\n", notice):
        if path in block:
            return block
    return ""


def declaration_problems(vendor, notice, declarations=UPSTREAM_DECLARATIONS):
    """Every way a tree's PROVENANCE row, LICENSE, NOTICE entry or crate license differs from what
    its upstream declares; empty when they match."""
    problems = []
    rows = {sub: line for line in (vendor / "PROVENANCE.md").read_text().splitlines()
            for sub in [m.group(1) for m in [re.search(r"\| `(tree-sitter-[\w-]+)/` \|", line)] if m]}
    for tree, d in sorted(declarations.items()):
        row = rows.get(tree, "")
        if d["repo"] not in row or d["commit"] not in row:
            problems.append(f"{tree}: PROVENANCE row does not name {d['repo']} at {d['commit']}")
        license_file = vendor / tree / "LICENSE"
        text = license_file.read_text() if license_file.exists() else ""
        copyrights = tuple(line.strip() for line in text.splitlines() if line.strip().startswith("Copyright"))
        if copyrights != d["copyrights"]:
            problems.append(f"{tree}: LICENSE copyright lines {copyrights} != upstream's {d['copyrights']}")
        block = notice_block(notice, tree)
        for name in d["licenses"] + d["copyrights"]:
            if name not in block:
                problems.append(f"{tree}: NOTICE entry lacks {name!r}")
        if not d["copyrights"]:
            for where, body in (("LICENSE", text), ("NOTICE entry", block)):
                if NO_COPYRIGHT not in body or d["author"] not in body:
                    problems.append(f"{tree}: {where} does not say upstream {NO_COPYRIGHT} and name {d['author']!r}")
        cargo = vendor / tree / "Cargo.toml"
        if d["spdx"] is not None:
            m = re.search(r'^license = "([^"]*)"', cargo.read_text(), re.M) if cargo.exists() else None
            if not m or m.group(1) != d["spdx"]:
                problems.append(f"{tree}: Cargo.toml license {m and m.group(1)!r} != {d['spdx']!r}")
            # The published crate carries upstream's terms: its `include` lists the LICENSE.
            inc = re.search(r"^include = \[([^\]]*)\]", cargo.read_text(), re.M) if cargo.exists() else None
            if inc and '"LICENSE"' not in inc.group(1):
                problems.append(f"{tree}: Cargo.toml include leaves out LICENSE")
    return problems


class UpstreamDeclarationTests(unittest.TestCase):
    """#322 (operator ruling): each vendored tree states exactly what its upstream declares."""

    def test_every_upstream_derived_tree_matches_its_declaration(self):
        self.assertEqual(declaration_problems(VENDOR, (ROOT / "NOTICE").read_text()), [])

    def test_every_third_party_tree_has_a_declaration(self):
        third_party = {sub for sub, source, lic in provenance_rows(VENDOR)
                       if not source.startswith("In-house") or f"`{sub}/LICENSE`" in lic}
        self.assertEqual(sorted(third_party), sorted(UPSTREAM_DECLARATIONS))

    def test_an_invented_copyright_line_is_caught(self):
        with tempfile.TemporaryDirectory() as tmp:
            vendor = Path(tmp) / "vendor"
            shutil.copytree(VENDOR, vendor, ignore=shutil.ignore_patterns("*.c", "*.h", "*.json", "corpus"))
            lic = vendor / "tree-sitter-vba" / "LICENSE"
            lic.write_text("Copyright (c) 2023 tmepple\n" + lic.read_text())
            problems = declaration_problems(vendor, (ROOT / "NOTICE").read_text())
            self.assertIn("tree-sitter-vba: LICENSE copyright lines ('Copyright (c) 2023 tmepple',) != upstream's ()",
                          problems)

    def test_a_dropped_declaration_or_commit_is_caught(self):
        notice = (ROOT / "NOTICE").read_text().replace("ISC", "")
        problems = declaration_problems(VENDOR, notice)
        self.assertIn("tree-sitter-vbscript: NOTICE entry lacks 'ISC'", problems)
        with tempfile.TemporaryDirectory() as tmp:
            vendor = Path(tmp) / "vendor"
            shutil.copytree(VENDOR, vendor, ignore=shutil.ignore_patterns("*.c", "*.h", "*.json", "corpus"))
            prov = vendor / "PROVENANCE.md"
            prov.write_text(prov.read_text().replace("1ee443353ce49f12e39a8e627b557c59e348b729", "unknown"))
            self.assertEqual(declaration_problems(vendor, (ROOT / "NOTICE").read_text()),
                             ["tree-sitter-vb6: PROVENANCE row does not name "
                              "https://github.com/joannefan/tree-sitter-vb6 at 1ee443353ce49f12e39a8e627b557c59e348b729"])


class VendoredGrammarProvenanceTests(unittest.TestCase):
    def test_trees_are_found(self):
        self.assertGreaterEqual(len(vendored_trees(VENDOR)), 12)

    def test_every_tree_is_recorded_and_attributed(self):
        self.assertEqual(provenance_problems(VENDOR, (ROOT / "NOTICE").read_text()), [])

    def test_a_lost_or_emptied_tree_license_is_caught(self):
        cases = (("tree-sitter-vb6", "Copyright (c) 2023 joannefan"),
                 ("tree-sitter-apexlang", "Copyright (c) 2025 Oracle and/or its affiliates."))
        for tree, line in cases:
            notice = (ROOT / "NOTICE").read_text().replace(line, "")
            for emptied in (False, True):
                with tempfile.TemporaryDirectory() as tmp:
                    vendor = Path(tmp) / "vendor"
                    shutil.copytree(VENDOR, vendor, ignore=shutil.ignore_patterns("*.c", "*.h", "*.json", "corpus"))
                    if emptied:
                        (vendor / tree / "LICENSE").write_text("")
                    else:
                        (vendor / tree / "LICENSE").unlink()
                    self.assertEqual(provenance_problems(vendor, notice),
                                     [f"{tree}: no LICENSE copyright line, but its row needs one"], (tree, emptied))

    def test_a_copyright_line_missing_from_notice_is_caught(self):
        notice = (ROOT / "NOTICE").read_text().replace("Copyright (c) 2023 tmepple", "")
        self.assertEqual(provenance_problems(VENDOR, notice),
                         ["tree-sitter-vba: 'Copyright (c) 2023 tmepple' missing from NOTICE"])

    def test_a_tree_without_a_row_is_caught(self):
        with tempfile.TemporaryDirectory() as tmp:
            vendor = Path(tmp) / "vendor"
            shutil.copytree(VENDOR, vendor, ignore=shutil.ignore_patterns("*.c", "*.h", "*.json", "corpus"))
            (vendor / "tree-sitter-new").mkdir()
            problems = provenance_problems(vendor, (ROOT / "NOTICE").read_text())
            self.assertEqual(len(problems), 2, problems)


if __name__ == "__main__":
    unittest.main()
