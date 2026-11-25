"""estate#595: the shipped binaries carry no recorded-SQL codegraph hook.

The hook (`--features codegraph-fixture`, test builds only) prints `CODEGRAPH-FIXTURE:` whenever
it stands recorded SQL in for a codegraph run; smoke-binary.py fails an archive whose binaries
contain that marker.
"""
import importlib.util
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("smoke_binary", HERE / "smoke-binary.py")
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


class RecordedHookMarker(unittest.TestCase):
    def write(self, d: str, name: str, data: bytes) -> Path:
        p = Path(d) / name
        p.write_bytes(data)
        return p

    def test_a_binary_with_the_marker_is_named(self):
        with tempfile.TemporaryDirectory() as d:
            clean = self.write(d, "a", b"\x7fELF ... CODEGRAPH-FAILED: ... \x00")
            hooked = self.write(d, "b", b"\x7fELF ... " + smoke.FIXTURE_MARKER + b" recorded\x00")
            self.assertEqual(smoke.with_recorded_hook([clean, hooked]), [hooked])

    def test_the_marker_is_found_across_a_read_boundary(self):
        with tempfile.TemporaryDirectory() as d:
            pad = b"x" * (smoke.READ_CHUNK - 5)
            hooked = self.write(d, "c", pad + smoke.FIXTURE_MARKER + b"\x00")
            self.assertEqual(smoke.with_recorded_hook([hooked]), [hooked])

    def test_clean_binaries_pass(self):
        with tempfile.TemporaryDirectory() as d:
            clean = self.write(d, "d", b"CODEGRAPH-SKIPPED: CODEGRAPH-OFF:")
            self.assertEqual(smoke.with_recorded_hook([clean]), [])


if __name__ == "__main__":
    unittest.main()
