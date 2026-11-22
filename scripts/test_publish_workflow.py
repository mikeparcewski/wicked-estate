"""Release certification guardrails; text-level checks need only the stdlib."""

from pathlib import Path
import os
import re
import subprocess
import tempfile
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


def step_script(job_text, step_name):
    """The dedented `run: |` body of one step, as the runner would execute it."""
    step = job_text.split(f"      - name: {step_name}\n", 1)
    if len(step) != 2:
        raise AssertionError(f"missing step: {step_name}")
    body = step[1].split("        run: |\n", 1)[1]
    lines = []
    for line in body.splitlines():
        if line.strip() and not line.startswith("          "):
            break
        lines.append(line[10:])
    return "\n".join(lines) + "\n"


def manifest_versions():
    """crate -> version from each publishable crate's own Cargo.toml (#563)."""
    workspace_version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    script = (ROOT / "scripts/publish.sh").read_text()
    crates = re.search(r"^CRATES=\(\n(.*?)\n\)", script, re.MULTILINE | re.DOTALL)[1].split()
    versions = {}
    for crate in crates:
        if crate.startswith("wicked-estate-tree-sitter-"):
            suffix = crate.removeprefix("wicked-estate-tree-sitter-")
            manifest = ROOT / f"crates/wicked-estate-extract/vendor/tree-sitter-{suffix}/Cargo.toml"
        else:
            manifest = ROOT / f"crates/{crate}/Cargo.toml"
        package = tomllib.loads(manifest.read_text())["package"]
        if package.get("publish") is False:
            continue
        version = package["version"]
        versions[crate] = workspace_version if version == {"workspace": True} else version
    return versions


# A stand-in for `scripts/publish.sh` in the registry-wait tests: the real one needs cargo.
STUB_PUBLISH = """#!/usr/bin/env bash
# Supports --list and --list-versions.
case "${1:-}" in
  --list) printf '%s\\n' wicked-estate-tree-sitter-rpg wicked-estate-tree-sitter-cfml wicked-estate-core wicked-estate ;;
  --list-versions) printf '%s\\n' wicked-estate-tree-sitter-rpg=0.1.3 wicked-estate-tree-sitter-cfml=0.26.21 \\
      wicked-estate-core=0.35.9 wicked-estate=0.35.9 ;;
  *) echo "stub publish.sh: unexpected argument ${1:-}" >&2; exit 2 ;;
esac
"""

# curl serves sparse-index files from $INDEX_DIR; sleep fails so a wait that would loop ends the run.
STUB_CURL = """#!/bin/sh
for url; do :; done
file="$INDEX_DIR/${url##*/}"
[ -f "$file" ] || exit 22
cat "$file"
"""
STUB_SLEEP = """#!/bin/sh
echo "stub sleep $*: the wait would loop" >&2
exit 1
"""
# For the timeout test: sleep "passes", and the clock jumps 16 minutes after the first sleep.
STUB_SLEEP_PASSES = """#!/bin/sh
touch "$INDEX_DIR/.slept"
"""
STUB_DATE = """#!/bin/sh
if [ -f "$INDEX_DIR/.slept" ]; then echo 1000960; else echo 1000000; fi
"""


class RegistryWaitTests(unittest.TestCase):
    """Runs the registry job's wait script against a fake crates.io index (#563)."""

    @classmethod
    def setUpClass(cls):
        workflow = (ROOT / ".github/workflows/publish.yml").read_text()
        cls.script = step_script(job(workflow, "registry"), "wait until crates.io serves every package (up to 15 min)")

    def run_wait(self, index, tag="v0.35.9", publish=STUB_PUBLISH, stubs=(("curl", STUB_CURL), ("sleep", STUB_SLEEP))):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "scripts").mkdir()
            (tmp / "scripts/publish.sh").write_text(publish)
            bin_dir, index_dir = tmp / "bin", tmp / "index"
            bin_dir.mkdir()
            index_dir.mkdir()
            for name, body in stubs:
                (bin_dir / name).write_text(body)
                (bin_dir / name).chmod(0o755)
            for crate, versions in index.items():
                (index_dir / crate).write_text("".join(
                    f'{{"name":"{crate}","vers":"{v}","yanked":{"true" if y else "false"}}}\n'
                    for v, y in versions
                ))
            (tmp / "wait.sh").write_text(self.script)
            env = dict(os.environ, PATH=f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
                       INDEX_DIR=str(index_dir), TAG=tag)
            return subprocess.run(
                ["bash", "--noprofile", "--norc", "-eo", "pipefail", str(tmp / "wait.sh")],
                cwd=tmp, env=env, text=True, capture_output=True, timeout=60,
            )

    PUBLISHED = {
        "wicked-estate-tree-sitter-rpg": [("0.1.2", False), ("0.1.3", False)],
        "wicked-estate-tree-sitter-cfml": [("0.26.21", False)],
        "wicked-estate-core": [("0.35.8", False), ("0.35.9", False)],
        "wicked-estate": [("0.35.8", False), ("0.35.9", False)],
    }

    def test_vendored_crates_resolve_at_their_own_versions(self):
        result = self.run_wait(self.PUBLISHED)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("all packages resolve", result.stdout)
        self.assertNotIn("not yet", result.stdout)

    def test_vendored_crate_missing_at_its_own_version_is_waited_for(self):
        index = dict(self.PUBLISHED, **{
            # The tag version is NOT the version the manifest asks for, and 0.1.3 is yanked.
            "wicked-estate-tree-sitter-rpg": [("0.1.2", False), ("0.1.3", True), ("0.35.9", False)],
        })
        result = self.run_wait(index)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not yet: wicked-estate-tree-sitter-rpg@0.1.3;", result.stdout)
        self.assertNotIn("cfml", result.stdout)

    def test_workspace_crate_missing_at_the_tag_version_is_waited_for(self):
        index = dict(self.PUBLISHED, **{"wicked-estate-core": [("0.35.8", False)]})
        result = self.run_wait(index)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not yet: wicked-estate-core@0.35.9;", result.stdout)

    def test_tag_must_match_the_wicked_estate_manifest_version(self):
        result = self.run_wait(self.PUBLISHED, tag="v0.35.10")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("::error::", result.stdout)
        self.assertIn("0.35.10", result.stdout)
        self.assertNotIn("all packages resolve", result.stdout)

    def test_tag_predating_list_versions_stops_with_a_clear_error(self):
        # The publish.sh of a tag cut before #563: it knows --list only.
        old = "#!/usr/bin/env bash\n[ \"$1\" = --list ] && echo wicked-estate && exit 0\necho PUBLISHING; exit 3\n"
        result = self.run_wait(self.PUBLISHED, publish=old)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("::error::scripts/publish.sh at v0.35.9 predates --list-versions", result.stdout)
        self.assertNotIn("PUBLISHING", result.stdout)

    def test_gives_up_after_15_minutes_naming_what_is_missing(self):
        index = dict(self.PUBLISHED, **{"wicked-estate-tree-sitter-rpg": [("0.1.2", False)]})
        stubs = (("curl", STUB_CURL), ("sleep", STUB_SLEEP_PASSES), ("date", STUB_DATE))
        result = self.run_wait(index, stubs=stubs)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("not yet: wicked-estate-tree-sitter-rpg@0.1.3; next check in 30s", result.stdout)
        self.assertIn(
            "::error::crates.io still does not serve every package at its manifest version after 15 minutes "
            "(missing on the last full check: wicked-estate-tree-sitter-rpg@0.1.3). Publish succeeded; "
            "re-run failed jobs, or dispatch the tag.",
            result.stdout,
        )

    def test_malformed_pair_is_refused(self):
        bad = STUB_PUBLISH.replace("wicked-estate-core=0.35.9", "wicked-estate-core=")
        result = self.run_wait(self.PUBLISHED, publish=bad)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("malformed crate=version pair: wicked-estate-core=", result.stdout)


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
            "bash scripts/publish.sh --list-versions", 'VERSION="${TAG#v}"',
            "https://index.crates.io/", '${crate:0:2}/${crate:2:2}/$crate',
            "curl -fsS --max-time 30", '--arg version "$version"',
            '.vers == $version', '.yanked == false', "sleep 30",
            "15 * 60", "::error::", "re-run failed jobs", "dispatch the tag",
            "exit 1",
        ):
            self.assertIn(text, registry)
        self.assertIn('for pair in $pairs; do', registry)
        # Each crate at its own version (#563), never every crate at the tag version.
        self.assertNotIn('--arg version "$VERSION"', registry)
        self.assertIn('TAG: ${{ github.event_name', registry)
        self.assertNotIn('${{', step_script(registry, 'wait until crates.io serves every package (up to 15 min)'))
        self.assertNotIn('< <(bash scripts/publish.sh', registry)

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


    def test_list_versions_maps_each_crate_to_its_own_manifest_version(self):
        expected = manifest_versions()
        workspace_version = expected["wicked-estate"]
        # The vendored grammars carry their own versions; that is what #563 is about.
        vendored = {c: v for c, v in expected.items() if c.startswith("wicked-estate-tree-sitter-")}
        self.assertTrue(vendored)
        self.assertTrue(any(v != workspace_version for v in vendored.values()))
        result = subprocess.run(
            ['bash', 'scripts/publish.sh', '--list-versions'], cwd=ROOT,
            text=True, capture_output=True, check=True, timeout=180,
        )
        self.assertEqual(result.stdout, ''.join(f'{c}={v}\n' for c, v in expected.items()))
        listed = subprocess.run(
            ['bash', 'scripts/publish.sh', '--list'], cwd=ROOT,
            text=True, capture_output=True, check=True, timeout=180,
        ).stdout.split()
        self.assertEqual(listed, [line.split('=', 1)[0] for line in result.stdout.split()])

    def test_publish_rejects_an_unknown_mode(self):
        # An older tag's publish.sh treated any unknown argument as a REAL publish.
        result = subprocess.run(
            ['bash', 'scripts/publish.sh', '--list-version'], cwd=ROOT,
            text=True, capture_output=True, timeout=60,
        )
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn('unknown argument', result.stderr)
        self.assertNotIn('>>>', result.stdout)


class BinariesWorkflowTests(unittest.TestCase):
    """#381: every macOS archive pins its floor for Rust and C alike."""

    @classmethod
    def setUpClass(cls):
        cls.binaries = (ROOT / ".github/workflows/binaries.yml").read_text()

    def legs(self):
        build = job(self.binaries, "build")
        include = build.split("        include:\n", 1)[1].split("    steps:\n", 1)[0]
        return [leg for leg in re.split(r"^          - ", include, flags=re.MULTILINE)[1:]]

    def test_every_macos_leg_pins_a_deployment_target(self):
        mac = [leg for leg in self.legs() if "apple-darwin" in leg.splitlines()[0]]
        self.assertEqual(len(mac), 2, mac)
        for leg in mac:
            target = leg.splitlines()[0]
            self.assertRegex(leg, r"\n\s+macos_min: '\d+\.\d+'\n", f"{target} has no macos_min")

    def test_shipped_macos_binaries_are_checked_against_the_floor(self):
        script = step_script(job(self.binaries, "build"), "minimum macOS recorded in the binaries")
        self.assertIn("vtool -show-build", script)
        self.assertIn("matrix.macos_min", script)


if __name__ == '__main__':
    unittest.main()
