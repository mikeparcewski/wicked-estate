"""#322: every grammar vendored as source has a PROVENANCE row and a NOTICE entry."""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[1]
VENDOR = ROOT / "crates/wicked-estate-extract/vendor"


def vendored_trees():
    return sorted(p.name for p in VENDOR.iterdir() if p.is_dir() and p.name.startswith("tree-sitter-"))


class VendoredGrammarProvenanceTests(unittest.TestCase):
    def test_trees_are_found(self):
        self.assertGreaterEqual(len(vendored_trees()), 12)

    def test_every_tree_has_one_provenance_row(self):
        table = (VENDOR / "PROVENANCE.md").read_text()
        subpaths = re.findall(r"^\| `[^`]+`[^|]*\| `([^`]+)/` \|", table, re.MULTILINE)
        self.assertEqual(sorted(subpaths), vendored_trees())

    def test_every_tree_is_attributed_in_notice(self):
        notice = (ROOT / "NOTICE").read_text()
        paths = set(re.findall(r"crates/wicked-estate-extract/vendor/(tree-sitter-[\w-]+)/", notice))
        self.assertEqual(sorted(paths), vendored_trees())

    def test_third_party_copyright_lines_are_reproduced_in_notice(self):
        # MIT requires the copyright notice to travel with the binary; NOTICE ships in every archive.
        notice = (ROOT / "NOTICE").read_text()
        for tree in vendored_trees():
            license_file = VENDOR / tree / "LICENSE"
            if not license_file.exists():
                continue
            for line in license_file.read_text().splitlines():
                if line.strip().startswith("Copyright (c)"):
                    self.assertIn(line.strip(), notice, f"{tree}: {line.strip()!r} missing from NOTICE")


if __name__ == "__main__":
    unittest.main()
