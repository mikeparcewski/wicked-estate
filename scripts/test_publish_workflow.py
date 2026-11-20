"""Release certification guardrails; text-level checks need only the stdlib."""

from pathlib import Path
import re
import subprocess
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[1]


def job(workflow, name):
    match = re.search(
        rf"^  {re.escape(name)}:\n(.*?)(?=^  [\w-]+:\n|\Z)",
        workflow,
        re.MULTILINE | re.DOTALL,
    )
    if not match:
        raise AssertionError(f"missing job: {name}")
    return match.group(1)


class PublishWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = (ROOT / ".github/workflows/publish.yml").read_text()
        cls.release = job(cls.workflow, "github-release")

    def test_registry_dependency_and_checkouts(self):
        registry = job(self.workflow, "registry")
        self.assertRegex(registry, r"needs: publish\b")
        self.assertIn("contents: read", registry)
        self.assertIn("runs-on: ubuntu-latest", registry)
        publish = job(self.workflow, "publish")
        for line in publish.splitlines():
            if any(part in line for part in (
                "uses: actions/checkout@", "uses: dtolnay/rust-toolchain@",
                "if: github.event_name", "ref: refs/tags/",
            )):
                self.assertIn(line, registry)
        self.assertEqual(registry.count("uses: actions/checkout@"), 2)
        self.assertIn("needs: [registry, binaries]", self.release)

    def test_registry_polls_every_publishable_crate(self):
        registry = job(self.workflow, "registry")
        for text in (
            "bash scripts/publish.sh --list", 'VERSION="${TAG#v}"',
            "https://index.crates.io/", '${crate:0:2}/${crate:2:2}/$crate',
            "curl -fsS --max-time 30", '--arg version "$VERSION"',
            '.vers == $version', '.yanked == false', "sleep 30",
            "15 * 60", "::error::", "re-run failed jobs", "dispatch the tag",
            "exit 1",
        ):
            self.assertIn(text, registry)
        self.assertIn('for crate in $crates; do', registry)
        self.assertIn('TAG: ${{ github.event_name', registry)
        self.assertNotIn('< <(bash scripts/publish.sh --list)', registry)

    def test_release_is_draft_only(self):
        create = next(line for line in self.release.splitlines() if "gh release create" in line)
        self.assertIn('--draft', create.split())
        self.assertIn('--verify-tag', create.split())
        self.assertNotIn('gh release edit', self.workflow)
        self.assertNotIn('--draft=false', self.workflow)
        self.assertNotIn('--latest', self.workflow)

    def test_existing_release_keeps_state_and_notes(self):
        self.assertIn('if gh release view "$TAG"', self.release)
        # Notes and creation belong only to the missing-release branch.
        branch = self.release.split('if gh release view "$TAG"', 1)[1]
        existing, missing = branch.split('          else\n', 1)
        self.assertNotIn('changelog-section.py', existing)
        self.assertNotIn('gh release edit', existing)
        self.assertIn('changelog-section.py "$TAG"', missing)
        self.assertIn('elif [ "$code" -ne 0 ]; then', missing)
        self.assertIn('exit "$code"', missing)
        self.assertIn('gh release create "$TAG"', missing)

    def test_release_tag_comes_from_env(self):
        step = self.release.split('      - name: draft the GitHub Release and attach the assets\n', 1)
        self.assertEqual(len(step), 2, 'missing draft step')
        env, run = step[1].split('        run: |\n', 1)
        self.assertIn('TAG: ${{ github.event_name', env)
        self.assertNotIn('${{', run)

    def test_assets_always_uploaded_after_create_or_reuse(self):
        self.assertIn('expected exactly 5 archives', self.release)
        self.assertIn('sha256sum wicked-estate-* > SHA256SUMS', self.release)
        self.assertIn('actions/attest-build-provenance@', self.release)
        self.assertRegex(
            self.release,
            r'fi\n          gh release upload "\$TAG" dist/wicked-estate-\* dist/SHA256SUMS --clobber',
        )

    def test_ci_gate_runs_python_tests(self):
        gate = job((ROOT / '.github/workflows/ci.yml').read_text(), 'gate')
        self.assertIn('runs-on: ubuntu-latest', gate)
        self.assertIn("python3 -m unittest discover -s scripts -p 'test_*.py' -v", gate)
        self.assertLess(gate.index('uses: dtolnay/rust-toolchain@'), gate.index('python3 -m unittest'))

    def test_publish_list_matches_crates_order(self):
        script = (ROOT / 'scripts/publish.sh').read_text()
        crates = re.search(r'^CRATES=\(\n(.*?)\n\)', script, re.MULTILINE | re.DOTALL)[1].split()
        expected = []
        for crate in crates:
            if crate.startswith('wicked-estate-tree-sitter-'):
                suffix = crate.removeprefix('wicked-estate-tree-sitter-')
                manifest = ROOT / f'crates/wicked-estate-extract/vendor/tree-sitter-{suffix}/Cargo.toml'
            else:
                manifest = ROOT / f'crates/{crate}/Cargo.toml'
            package = tomllib.loads(manifest.read_text())['package']
            if package.get('publish') is not False:
                expected.append(crate)
        self.assertIn('wicked-estate', expected)
        self.assertIn('wicked-estate-mcp', expected)
        result = subprocess.run(
            ['bash', 'scripts/publish.sh', '--list'], cwd=ROOT,
            text=True, capture_output=True, check=True, timeout=180,
        )
        self.assertEqual(result.stdout, ''.join(f'{crate}\n' for crate in expected))


if __name__ == '__main__':
    unittest.main()
