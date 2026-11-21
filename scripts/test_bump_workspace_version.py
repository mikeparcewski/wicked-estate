"""release.yml's version bump must touch only the workspace version and internal pins (#479)."""

import difflib
from pathlib import Path
import unittest

from bump_workspace_version import bump

ROOT = Path(__file__).resolve().parents[1]

FIXTURE = '''[workspace]
members = ["crates/wicked-estate-core"]

[workspace.package]
version = "0.35.8"   # bump this
edition = "2024"
rust-version = "1.85"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
wicked-estate-core = { path = "crates/wicked-estate-core", version = "0.35.8" }
wicked-estate-store = { path = "crates/wicked-estate-store", version = "0.35.8" }
wicked-estate-tree-sitter-foo = { path = "vendor/foo", version = "1.2.3" }
zip = { version = "9.0.5", default-features = false }
calamine = "0.38.1"
wicked-estate-registry-only = { version = "0.35.8" }

[profile.release]
version = "1.2.3"
'''


def added(old, new):
    return [
        l[2:] for l in difflib.ndiff(old.splitlines(), new.splitlines()) if l.startswith("+ ")
    ]


class BumpWorkspaceVersionTests(unittest.TestCase):
    def test_only_workspace_version_and_internal_pins_change(self):
        new = bump(FIXTURE, "0.35.9")
        self.assertEqual(
            added(FIXTURE, new),
            [
                'version = "0.35.9"   # bump this',
                'wicked-estate-core = { path = "crates/wicked-estate-core", version = "0.35.9" }',
                'wicked-estate-store = { path = "crates/wicked-estate-store", version = "0.35.9" }',
            ],
        )
        for kept in (
            'zip = { version = "9.0.5", default-features = false }',
            'calamine = "0.38.1"',
            'wicked-estate-registry-only = { version = "0.35.8" }',
            'wicked-estate-tree-sitter-foo = { path = "vendor/foo", version = "1.2.3" }',
        ):
            self.assertIn(kept, new)

    def test_real_root_manifest_keeps_third_party_pins(self):
        text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
        new = bump(text, "9.9.9")
        self.assertEqual(
            [l for l in new.splitlines() if l.startswith("zip = ")],
            [l for l in text.splitlines() if l.startswith("zip = ")],
        )
        self.assertTrue(
            all(
                '"9.9.9"' in l
                for l in new.splitlines()
                if l.startswith(("wicked-estate-core =", "wicked-estate-store ="))
            )
        )


if __name__ == "__main__":
    unittest.main()
