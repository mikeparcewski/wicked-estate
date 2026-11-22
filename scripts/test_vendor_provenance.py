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
