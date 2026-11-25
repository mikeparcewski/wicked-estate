"""wicked-estate#636: memory faults in vendored scanner code, each shown by a small C harness.

The harnesses in scripts/scanner-harness/ include a scanner's own source and drive the function at
fault directly. A guard page turns an over-read into a deterministic crash, and counting allocators
show a leak, so each test fails on the unpatched scanner and passes on the patched one. CI's
runners and developer machines both have `cc`; the guard page needs POSIX `mmap`.
"""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
VENDOR = ROOT / "crates/wicked-estate-extract/vendor"
HARNESS = ROOT / "scripts/scanner-harness"


@unittest.skipIf(os.name != "posix", "the guard page needs POSIX mmap")
class ScannerMemoryTests(unittest.TestCase):
    def build(self, source, include):
        cc = shutil.which("cc")
        self.assertIsNotNone(cc, "a C compiler (cc) is needed")
        out = Path(self.tmp.name) / source.removesuffix(".c")
        # Undefined behaviour (a NULL handed to memcpy, say) stops the harness where the
        # toolchain has the sanitizer runtime; the guard page and the counters work either way.
        for sanitize in (["-fsanitize=undefined", "-fno-sanitize-recover=all"], []):
            r = subprocess.run([cc, "-std=gnu11", "-w", "-g", "-O0", *sanitize, f"-I{include}", f"-I{HARNESS}",
                                str(HARNESS / source), "-o", str(out)], capture_output=True, text=True)
            if r.returncode == 0:
                return out
        self.fail(r.stderr)

    def run_harness(self, exe, *args):
        r = subprocess.run([str(exe), *args], capture_output=True, text=True, timeout=60)
        # A negative return code is the signal that killed it (SIGSEGV or SIGBUS: an over-read).
        self.assertEqual(r.returncode, 0, f"{exe.name} {' '.join(args)}: rc {r.returncode}\n{r.stdout}{r.stderr}")
        return r.stdout

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()

    def tearDown(self):
        self.tmp.cleanup()

    def test_cfml_start_tags_do_not_leak_their_names(self):
        exe = self.build("cfml_memory.c", VENDOR / "tree-sitter-cfml/cfml/src")
        self.assertIn("live allocations after destroy: 0", self.run_harness(exe, "leak"))

    def test_cfml_deserialize_stays_within_its_buffer(self):
        exe = self.build("cfml_memory.c", VENDOR / "tree-sitter-cfml/cfml/src")
        self.assertIn("deserialized 1017 HTML tags, 0 CF tags", self.run_harness(exe, "deserialize"))

    def test_cfml_reset_hands_memcpy_no_null(self):
        exe = self.build("cfml_memory.c", VENDOR / "tree-sitter-cfml/cfml/src")
        self.assertIn("reset ok", self.run_harness(exe, "reset"))

    def test_cobol_keyword_match_stops_at_ruled_out_keywords(self):
        exe = self.build("cobol_keywords.c", VENDOR / "tree-sitter-cobol-scanner")
        self.assertIn("matched: 1", self.run_harness(exe))


if __name__ == "__main__":
    unittest.main()
