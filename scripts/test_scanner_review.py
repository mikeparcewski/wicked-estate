"""#322 rule 5: every hand-written C file in a vendored grammar has a recorded review of its bytes.

A scanner runs on every file the index reads, so its review is pinned to the file's git blob in
`vendor/SCANNER-REVIEW.md`. Any change to a scanner (an upstream update, a local patch) fails CI
until the file is reviewed again and its row records the new blob. Generated code (`parser.c` and
the CLI's `tree_sitter/` headers) is not hand-written and is checked by regeneration instead.
"""

import hashlib
from pathlib import Path
import re
import shutil
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
VENDOR = ROOT / "crates/wicked-estate-extract/vendor"
ROW = re.compile(r"^\| `([^`]+)` \| `([0-9a-f]{40})` \|", re.M)


def blob(path):
    data = path.read_bytes()
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()


# Compared lower-cased, so `.C` (C++) and `.S` (assembly) count too.
CODE = {".c", ".h", ".i", ".ii", ".cc", ".cp", ".cpp", ".cxx", ".c++", ".hh", ".hp", ".hpp", ".hxx", ".h++",
        ".inc", ".inl", ".ipp", ".tcc", ".s", ".sx", ".asm", ".m", ".mm"}
# What `scripts/regen-vendored-parsers.py` writes next to each `src/grammar.json`; CI checks those
# files by regenerating them, so they are not reviewed here. Nothing else is exempt.
GENERATED = ("parser.c", "tree_sitter/alloc.h", "tree_sitter/array.h", "tree_sitter/parser.h")


def hand_written(vendor):
    """Every C or assembly source or header under vendor/ that the pinned CLI does not generate."""
    generated = {g / name for g in (p.parent for p in vendor.rglob("src/grammar.json")) for name in GENERATED}
    return sorted(str(p.relative_to(vendor)) for p in vendor.rglob("*")
                  if p.is_file() and p.suffix.lower() in CODE and p not in generated)


def review_problems(vendor=VENDOR):
    record = vendor / "SCANNER-REVIEW.md"
    rows = dict(ROW.findall(record.read_text())) if record.is_file() else {}
    files = hand_written(vendor)
    problems = [f"{rel}: no recorded review" for rel in files if rel not in rows]
    problems += [f"{rel}: changed since its review (reviewed {rows[rel]}, now {blob(vendor / rel)})"
                 for rel in files if rel in rows and rows[rel] != blob(vendor / rel)]
    problems += [f"{rel}: reviewed, but no such file" for rel in sorted(set(rows) - set(files))]
    return problems


class ScannerReviewTests(unittest.TestCase):
    def copy(self, tmp):
        vendor = Path(tmp) / "vendor"
        shutil.copytree(VENDOR, vendor, ignore=shutil.ignore_patterns("parser.c", "node-types.json", "corpus", "test"))
        return vendor

    def test_the_hand_written_c_is_found(self):
        files = hand_written(VENDOR)
        for rel in ("tree-sitter-cfml/cfscript/src/scanner.c", "tree-sitter-cfml/common/scanner.h",
                    "tree-sitter-cobol-scanner/scanner.c", "tree-sitter-apexlang/src/scanner.c"):
            self.assertIn(rel, files)
        # The COBOL tree has no grammar.json, so its copied header is reviewed, not regenerated.
        self.assertIn("tree-sitter-cobol-scanner/tree_sitter/parser.h", files)
        self.assertFalse([f for f in files if f.endswith("src/parser.c") or "/src/tree_sitter/" in f], files)

    def test_every_hand_written_c_file_has_a_review_of_its_current_bytes(self):
        self.assertEqual(review_problems(VENDOR), [])

    def test_a_changed_new_or_removed_scanner_is_caught(self):
        with tempfile.TemporaryDirectory() as tmp:
            vendor = self.copy(tmp)
            scanner = vendor / "tree-sitter-apexlang/src/scanner.c"
            reviewed = blob(scanner)
            scanner.write_bytes(scanner.read_bytes() + b"\n")
            (vendor / "tree-sitter-vba/src/scanner.c").write_text("/* new */\n")
            (vendor / "tree-sitter-cfml/cfml/src/tag.c").unlink()
            # Hand-written code beside the generated headers, or under another suffix, is still code.
            (vendor / "tree-sitter-vba/src/tree_sitter/helper.c").write_text("/* new */\n")
            (vendor / "tree-sitter-vba/src/keywords.inc").write_text("/* new */\n")
            (vendor / "tree-sitter-vba/src/helper.C").write_text("/* new */\n")
            self.assertEqual(review_problems(vendor), [
                "tree-sitter-vba/src/helper.C: no recorded review",
                "tree-sitter-vba/src/keywords.inc: no recorded review",
                "tree-sitter-vba/src/scanner.c: no recorded review",
                "tree-sitter-vba/src/tree_sitter/helper.c: no recorded review",
                f"tree-sitter-apexlang/src/scanner.c: changed since its review (reviewed {reviewed}, now {blob(scanner)})",
                "tree-sitter-cfml/cfml/src/tag.c: reviewed, but no such file",
            ])


if __name__ == "__main__":
    unittest.main()
