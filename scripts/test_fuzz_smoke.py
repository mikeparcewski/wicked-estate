"""#322 step 5: the fuzz smoke's own contract. It needs no CLI; CI runs the smoke itself."""

import importlib.util
import os
from pathlib import Path
import random
import re
import signal
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), ROOT / "scripts" / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FuzzSmokeTests(unittest.TestCase):
    def setUp(self):
        self.fz = load("fuzz-smoke-vendored-parsers")
        self.regen = self.fz.regen()

    def test_ci_runs_the_smoke(self):
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertRegex(ci, r"\n {12}python3 scripts/fuzz-smoke-vendored-parsers\.py --inputs \d+\n")
        # Decision B: skipped only on a PR that changes neither vendor/ nor the smoke.
        self.assertIn('git diff --quiet "$BASE" HEAD -- crates/wicked-estate-extract/vendor '
                      "scripts/fuzz-smoke-vendored-parsers.py scripts/regen-vendored-parsers.py; then", ci)

    def test_inputs_are_deterministic_and_cover_the_adversarial_shapes(self):
        grammar = self.regen.grammar_dirs()[0]
        a = self.fz.inputs_for(grammar, 120, random.Random("s"))
        b = self.fz.inputs_for(grammar, 120, random.Random("s"))
        self.assertEqual(a, b)
        labels = {label for label, _ in a[1]}
        for shape in ("repeat:<!--", "repeat:(", "long-line", "nul", "all-bytes", "random-bytes", "token-sequence"):
            self.assertIn(shape, labels)

    def test_every_token_is_also_cut_off_inside_a_string_it_opens(self):
        # The cfscript query-text scan waited for a closing quote that a file ending after
        # `queryExecute("` never has (#322 scanner review). Every grammar token followed by an
        # opener and then the end of the file is in the smoke, so that shape is never left to chance.
        cfscript = next(d for d in self.regen.grammar_dirs() if d.relative_to(self.regen.VENDOR).as_posix() == "tree-sitter-cfml/cfscript")
        _, inputs = self.fz.inputs_for(cfscript, 1, random.Random("s"))
        cut = {data for label, data in inputs if label.startswith("token-open-at-eof")}
        self.assertIn(b'queryExecute("x', cut)
        self.assertIn(b"queryExecute(x", cut)

    @unittest.skipIf(os.name != "posix", "process groups are POSIX")
    def test_the_deadline_kills_the_parser_the_launcher_started(self):
        # In CI the CLI is `npx`, whose launcher starts the native parser as a child. A scanner
        # stuck in a loop ignores SIGTERM, so the deadline must kill that child too (round-1 review).
        with tempfile.TemporaryDirectory() as tmp:
            pidfile = Path(tmp) / "child.pid"
            child = ("import os, signal, sys, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); "
                     "open(sys.argv[1], 'w').write(str(os.getpid())); time.sleep(600)")
            launcher = ("import subprocess, sys; "
                        f"subprocess.Popen([sys.executable, '-c', {child!r}, {str(pidfile)!r}]).wait()")
            case = Path(tmp) / "in"
            case.write_bytes(b"x")
            started = time.monotonic()
            failed, slow = self.fz.run([sys.executable, "-c", launcher], Path(tmp), [case], dict(os.environ), deadline=3)
            self.assertEqual((failed, slow), (True, []))
            self.assertLess(time.monotonic() - started, 30)
            pid = int(pidfile.read_text())
            for _ in range(50):
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    break
                time.sleep(0.1)
            else:
                os.kill(pid, signal.SIGKILL)
                self.fail(f"the parser child {pid} outlived the deadline")

    def test_every_grammar_contributes_its_own_tokens(self):
        for grammar in self.regen.grammar_dirs():
            name, tokens = self.fz.grammar_tokens(grammar)
            self.assertTrue(tokens, name)

    def test_every_known_slow_shape_names_a_parser_and_an_issue(self):
        parsers = {str(d.relative_to(self.regen.VENDOR)) for d in self.regen.grammar_dirs()}
        for (parser, shape), why in self.fz.KNOWN_SLOW.items():
            self.assertIn(parser, parsers)
            self.assertTrue(shape.startswith("repeat:") or shape in ("long-line", "nul", "all-bytes"), shape)
            self.assertRegex(why, r"^#\d+ ")


if __name__ == "__main__":
    unittest.main()
