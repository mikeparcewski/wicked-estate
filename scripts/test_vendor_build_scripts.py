"""#322 step 4 (rule 4): a vendored grammar's build script only compiles C with `cc`.

A build script runs on every developer machine and CI runner that builds the workspace, before any
test. A vendored one must not spawn processes, read the environment, touch the network or write
outside OUT_DIR. Rather than list what is forbidden, this allows a fixed vocabulary: after comments
and string literals are removed, every identifier must be one a plain `cc` build uses or a name the
script itself binds. Anything else (`std::process`, `std::env`, `std::fs`, `include!`, a second
build dependency, a `links` key, a `cargo:` instruction other than rerun-if-changed) fails CI.
"""

from pathlib import Path
import re
import shutil
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
VENDOR = ROOT / "crates/wicked-estate-extract/vendor"

# What a build script that compiles C with `cc` needs, and nothing more. A name the script binds
# (`let`, `for`) may stand alone, but never as a path segment (`a::b`) or a method (`.b`).
ALLOWED = {
    "fn", "main", "let", "mut", "for", "in",
    "std", "path", "Path", "new", "cc", "Build",
    "include", "flag_if_supported", "file", "compile", "join", "to_str", "unwrap", "println",
}
PUNCT = {"(", ")", "{", "}", "[", "]", ";", ",", ".", "::", "=", "&", "!"}
TOKEN = re.compile(r"""
    (?P<ws>\s+)
  | (?P<line_comment>//[^\n]*)
  | (?P<block_comment>/\*)
  | (?P<raw>(?:b|c)?r\#*")
  | (?P<string>(?:b|c)?"(?:[^"\\]|\\.)*")
  | (?P<char>b?'(?:[^'\\]|\\.)')
  | (?P<ident>(?:r\#)?[A-Za-z_]\w*)
  | (?P<number>\d[\w.]*)
  | (?P<punct>::|.)
""", re.X | re.S)
# Every string literal is plain, so what Rust decodes is what is checked here (no escapes): a
# compiler flag from a short list (a flag can load code, as `-fplugin=` or an `@file` does), or a
# relative path whose segments never start with `.` (so never `..`), or `.` itself. The one
# println! format is a rerun-if-changed line over such a path or a `{}` placeholder.
FLAG = re.compile(r'"-(?:w|std=c\d\d|Wno-[A-Za-z0-9-]+)"')
SEGMENT = r"[A-Za-z0-9_][A-Za-z0-9_.-]*"
PLAIN = re.compile(rf'"(?:\.|{SEGMENT}(?:/{SEGMENT})*)"')
RERUN = re.compile(rf'"cargo:rerun-if-changed=(?:{SEGMENT}(?:/{SEGMENT})*|\{{[A-Za-z_]*\}})"')


def tokens(source):
    """(kind, text) for each token, comments dropped. Block comments nest, as in Rust."""
    out, i = [], 0
    while i < len(source):
        m = TOKEN.match(source, i)
        kind = m.lastgroup
        if kind == "block_comment":
            depth, i = 1, m.end()
            while depth and i < len(source):
                if source.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif source.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
            continue
        if kind not in ("ws", "line_comment"):
            out.append((kind, m.group()))
        i = m.end()
    return out


def crates(vendor):
    """Every vendored crate: each Cargo.toml under vendor/, at any depth."""
    return sorted(p.parent for p in vendor.rglob("Cargo.toml"))


def dependency_problems(package):
    """The build dependencies are exactly the registry crate `cc`: in `[build-dependencies]`, and
    nowhere else (Cargo also reads `build_dependencies` and per-target tables)."""
    spellings = ("build-dependencies", "build_dependencies")
    main = package.get("build-dependencies", {})
    others = [package.get("build_dependencies", {})]
    others += [target.get(s, {}) for target in package.get("target", {}).values() for s in spellings]
    problems = []
    if sorted(main) != ["cc"]:
        problems.append(f"build-dependencies are {sorted(main)}, not just cc")
    problems += [f"another build-dependencies table lists {sorted(table)}" for table in others if table]
    spec = main.get("cc")
    if isinstance(spec, dict) and set(spec) - {"version"}:
        problems.append(f"cc is not the registry crate: it sets {sorted(set(spec) - {'version'})}")
    return problems


def build_script_problems(vendor=VENDOR):
    problems = []
    scripts = set()
    for crate in crates(vendor):
        name = str(crate.relative_to(vendor))
        package = tomllib.loads((crate / "Cargo.toml").read_text())
        pkg = package.get("package", {})
        problems += [f"{name}: {p}" for p in dependency_problems(package)]
        if "links" in pkg:
            problems.append(f"{name}: sets links = {pkg['links']!r}")
        # Cargo runs build.rs at the crate root when `build` is unset.
        rel = pkg.get("build", "build.rs")
        if rel is False:
            continue
        script = crate / rel
        if not script.is_file():
            problems.append(f"{name}: build script {rel} is missing")
            continue
        scripts.add(script)
        problems += [f"{name}: {p}" for p in script_problems(script.read_text())]
    # A build.rs no manifest points at is still a file someone could wire up; hold it to the rule.
    for stray in sorted(set(vendor.rglob("build.rs")) - scripts):
        problems += [f"{stray.relative_to(vendor)}: {p}" for p in script_problems(stray.read_text())]
    return problems


def script_problems(source):
    problems = []
    toks = tokens(source)
    for kind, text in toks:
        if kind in ("raw", "char", "number"):
            problems.append(f"uses a {kind} literal {text!r}")
        elif kind == "punct" and text not in PUNCT:
            problems.append(f"uses {text!r}, which a cc-only build script does not need")
        elif kind == "string" and not (RERUN.fullmatch(text) or FLAG.fullmatch(text) or PLAIN.fullmatch(text)):
            problems.append(f"uses the string {text}, which is not a listed flag or a plain path in the crate")
    idents = [(i, text.removeprefix("r#")) for i, (kind, text) in enumerate(toks) if kind == "ident"]
    bound = set()
    for i, word in idents:
        if word == "let":
            j = i + 1 + (toks[i + 1][1] == "mut" if i + 1 < len(toks) else 0)
            if j < len(toks) and toks[j][0] == "ident":
                bound.add(toks[j][1])
        elif word == "for":
            j = i + 1
            while j < len(toks) and toks[j][1] != "in":
                if toks[j][0] == "ident":
                    bound.add(toks[j][1])
                j += 1
    bound -= ALLOWED
    for i, word in idents:
        joined = (i > 0 and toks[i - 1][1] in ("::", ".")) or (i + 1 < len(toks) and toks[i + 1][1] == "::")
        if word not in ALLOWED and (word not in bound or joined):
            problems.append(f"uses {word!r}, which a cc-only build script does not need")
    fns = [i for i, word in idents if word == "fn"]
    if len(fns) != 1 or toks[fns[0] + 1][1] != "main":
        problems.append("declares something other than one fn main")
    for i, (kind, text) in enumerate(toks):
        if text != "!":
            continue
        if i == 0 or toks[i - 1][1] != "println" or i + 1 >= len(toks) or toks[i + 1][1] != "(":
            problems.append("uses a macro other than println!(...)")
        elif i + 2 >= len(toks) or not RERUN.fullmatch(toks[i + 2][1]):
            arg = toks[i + 2][1] if i + 2 < len(toks) else ""
            problems.append(f"println! {arg} is not a rerun-if-changed instruction")
    return sorted(set(problems), key=problems.index)


class VendorBuildScriptTests(unittest.TestCase):
    def copy(self, tmp):
        vendor = Path(tmp) / "vendor"
        shutil.copytree(VENDOR, vendor, ignore=shutil.ignore_patterns("*.c", "*.h", "*.json", "corpus", "test"))
        return vendor

    def test_every_vendored_crate_is_found(self):
        self.assertGreaterEqual(len(crates(VENDOR)), 11)

    def test_every_vendored_build_script_only_compiles_c(self):
        self.assertEqual(build_script_problems(VENDOR), [])

    def test_a_build_script_that_does_more_is_caught(self):
        cases = {
            'std::process::Command::new("curl").status().unwrap();': ["uses 'Command'", "uses 'process'", "uses 'status'"],
            'let home = std::env::var("HOME").unwrap();': ["uses 'env'", "uses 'var'"],
            'std::fs::write("x", "y").unwrap();': ["uses 'fs'", "uses 'write'"],
            'include!("other.rs");': ["uses a macro other than println!(...)"],
            'println!("cargo:rustc-link-search=/tmp");': ["is not a rerun-if-changed instruction"],
            'let s = std::net::TcpStream::connect("h:80");': ["uses 'TcpStream'", "uses 'connect'", "uses 'net'"],
            # Round-1 review: each of these passed the first version of this lint.
            # A bound name does not make a path segment or a method allowed.
            'let env = 0; let var = 0; std::env::var("HOME").unwrap();': ["uses 'env'", "uses 'var'"],
            'let write = 0; let fs = 0; std::fs::write("x", "y").unwrap();': ["uses 'fs'", "uses 'write'"],
            # Comment markers inside strings do not hide the code between them.
            'let s = "/*"; std::fs::write("x", "owned").unwrap(); let t = "*/";': ["uses 'fs'", "uses 'write'"],
            # Any macro delimiter, and no second instruction inside the literal.
            'println!{"cargo:rustc-link-search=/tmp"};': ["uses a macro other than println!(...)"],
            'println!("cargo:rerun-if-changed=x\\ncargo:rustc-link-search=/tmp");': ["is not a rerun-if-changed instruction"],
            'println!(r"cargo:rerun-if-changed=x");': ["uses a raw literal"],
            # Raw identifiers, attributes, closures, `?`, `use` and more functions.
            'let h = std::r#env::var("HOME");': ["uses 'env'"],
            '#[allow(unused)] let a = 1;': ["uses '#'", "uses 'allow'"],
            'let run = |c| c; run(1);': ["uses '|'"],
            'use std::env as e;': ["uses 'use'", "uses 'as'", "uses 'e'"],
            '} fn helper() {': ["declares something other than one fn main"],
            # Flags that load code, and paths outside the crate, however they are spelt.
            'cc::Build::new().flag_if_supported("-fplugin=/tmp/p.so");': ['uses the string "-fplugin=/tmp/p.so"'],
            'cc::Build::new().flag_if_supported("@/tmp/flags");': ['uses the string "@/tmp/flags"'],
            'cc::Build::new().file("../../../x.c");': ['uses the string "../../../x.c"'],
            'cc::Build::new().include("/usr/local/include");': ['uses the string "/usr/local/include"'],
            'cc::Build::new().file("./\\x2e\\x2e/payload.c");': ['uses the string "./\\x2e\\x2e/payload.c"'],
            # The format argument cannot carry a second Cargo instruction either.
            'println!("cargo:rerun-if-changed={}", "x\\ncargo:rustc-link-search=/tmp");':
                ['uses the string "x\\ncargo:rustc-link-search=/tmp"'],
        }
        for line, expected in cases.items():
            with tempfile.TemporaryDirectory() as tmp:
                vendor = self.copy(tmp)
                script = vendor / "tree-sitter-vb6" / "bindings" / "rust" / "build.rs"
                script.write_text(script.read_text().replace("fn main() {\n", "fn main() {\n    " + line + "\n", 1))
                problems = build_script_problems(vendor)
                self.assertTrue(problems, line)
                self.assertTrue(all(p.startswith("tree-sitter-vb6: ") for p in problems), problems)
                for fragment in expected:
                    self.assertTrue(any(fragment in p for p in problems), (line, fragment, problems))

    def test_a_crate_that_adds_a_build_dependency_or_links_is_caught(self):
        with tempfile.TemporaryDirectory() as tmp:
            vendor = self.copy(tmp)
            cargo = vendor / "tree-sitter-vba" / "Cargo.toml"
            text = cargo.read_text().replace("[build-dependencies]\n", "[build-dependencies]\nreqwest = \"0.12\"\n", 1)
            text = text.replace("[package]\n", "[package]\nlinks = \"vba\"\n", 1)
            cargo.write_text(text)
            self.assertEqual(build_script_problems(vendor),
                             ["tree-sitter-vba: build-dependencies are ['cc', 'reqwest'], not just cc",
                              "tree-sitter-vba: sets links = 'vba'"])

    def test_any_other_build_dependency_table_or_a_renamed_cc_is_caught(self):
        for extra, expected in (
            ("[target.'cfg(unix)'.build-dependencies]\nreqwest = \"0.12\"\n",
             "tree-sitter-vba: another build-dependencies table lists ['reqwest']"),
            ("[build_dependencies]\nreqwest = \"0.12\"\n",
             "tree-sitter-vba: another build-dependencies table lists ['reqwest']"),
        ):
            with tempfile.TemporaryDirectory() as tmp:
                vendor = self.copy(tmp)
                cargo = vendor / "tree-sitter-vba" / "Cargo.toml"
                cargo.write_text(cargo.read_text() + "\n" + extra)
                self.assertEqual(build_script_problems(vendor), [expected])
        with tempfile.TemporaryDirectory() as tmp:
            vendor = self.copy(tmp)
            cargo = vendor / "tree-sitter-vba" / "Cargo.toml"
            cargo.write_text(cargo.read_text().replace('cc = "1"', 'cc = { package = "reqwest", version = "0.12" }', 1))
            self.assertEqual(build_script_problems(vendor),
                             ["tree-sitter-vba: cc is not the registry crate: it sets ['package']"])

    def test_a_nested_crate_is_held_to_the_rule(self):
        with tempfile.TemporaryDirectory() as tmp:
            vendor = self.copy(tmp)
            nested = vendor / "tree-sitter-vba" / "inner"
            nested.mkdir()
            (nested / "Cargo.toml").write_text('[package]\nname = "inner"\nversion = "0.1.0"\n\n'
                                               '[build-dependencies]\nreqwest = "0.12"\n')
            (nested / "build.rs").write_text("fn main() {}\n")
            self.assertEqual(build_script_problems(vendor),
                             ["tree-sitter-vba/inner: build-dependencies are ['reqwest'], not just cc"])

    def test_a_stray_build_rs_is_held_to_the_rule(self):
        with tempfile.TemporaryDirectory() as tmp:
            vendor = self.copy(tmp)
            (vendor / "tree-sitter-vba" / "build.rs").write_text('fn main() { std::env::var("X"); }\n')
            problems = build_script_problems(vendor)
            self.assertIn("tree-sitter-vba/build.rs: uses 'env', which a cc-only build script does not need", problems)


if __name__ == "__main__":
    unittest.main()
