"""#322: every grammar vendored as source has a PROVENANCE row and a NOTICE entry."""

import hashlib
from pathlib import Path
import re
import shutil
import tempfile
import tomllib
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
        text = license_file.read_text() if license_file.exists() else ""
        copyrights = [line.strip() for line in text.splitlines() if line.strip().startswith("Copyright (c)")]
        # MIT requires the copyright notice to travel with the binary; NOTICE ships in every archive.
        # An upstream that states none gets a LICENSE that says so (see UPSTREAM_DECLARATIONS).
        if tree in own_license and not copyrights and NO_COPYRIGHT not in text:
            problems.append(f"{tree}: no LICENSE copyright line, but its row needs one")
        problems += [f"{tree}: {c!r} missing from NOTICE" for c in copyrights if c not in notice]
    return problems


# The intake record (#322): what each tree's upstream declares, read at the upstream commit whose
# files the vendored ones match by git blob. A copyright line is listed only when upstream states
# one; we never supply a line upstream does not state. Add an entry when you vendor upstream code.
#   files:        the vendored files that are byte-identical to that commit (git blob sha); the
#                 rest of the tree (Cargo.toml, README.md, bindings/rust/, a LICENSE we wrote) is
#                 this repository's packaging or a documented patch
#   license_blob: upstream's own LICENSE, vendored unchanged; None when upstream ships none
#   licenses:     the declared license names, as NOTICE and the row spell them
#   notice_texts: license texts NOTICE itself must carry (binary archives ship only the root
#                 LICENSE, NOTICE and README; the root LICENSE already carries the MIT text)
#   copyrights:   upstream's copyright lines; extra_copyrights: other declared holders in NOTICE
UPSTREAM_DECLARATIONS = {
    "tree-sitter-cfml": {
        # LICENSE; package.json, Cargo.toml, tree-sitter.json: MIT.
        "repo": "https://github.com/cfmleditor/tree-sitter-cfml",
        "commit": "6b4a43943c191950ee2c77f7b94cec5e903318a4",
        "files": {
            "cfml/grammar.js": "df8dab9f30af00ce3836e5e64243d8e5523de90b",
            "cfml/src/grammar.json": "a1ca24d4363fc37eca53938fbc8c7fc638e6adac",
            "cfml/src/node-types.json": "5163d06532e59ec7eb04c09de8dd2068e66168ca",
            "cfml/src/parser.c": "3dfc93f3e9a9a60779e4b6043ad414cf2e1c2262",
            "cfml/src/scanner.c": "74cd1565e362b3d39a10792fdfc20e0689b0587e",
            "cfml/src/tag.c": "36788ffa85f346445c8d79fb3eedd0cab5202233",
            "cfml/src/tree_sitter/alloc.h": "1abdd1201578605b8d05a3001f01ebc0d58826d9",
            "cfml/src/tree_sitter/array.h": "56fc8cd470996ded64a5452be25e26dbd2aa682a",
            "cfml/src/tree_sitter/parser.h": "858107deb9e4a1ffcd2ed5e6ffe98a5fcab3e1f6",
            "cfscript/grammar.js": "a4502bba82fd2eaf445f4cb37c78a0dd87f3135c",
            "cfscript/src/grammar.json": "4993b623333585e0237e1958dab4c4883f171571",
            "cfscript/src/node-types.json": "15ed603f8bc6794cf02e2834b014ae833d4efac8",
            "cfscript/src/parser.c": "c1e853b81024b7c20541ac72bcb42e531cdb207c",
            "cfscript/src/scanner.c": "1ad53fe82763c4bf4ea0eff5e046dcafe1b897ee",
            "cfscript/src/tree_sitter/alloc.h": "1abdd1201578605b8d05a3001f01ebc0d58826d9",
            "cfscript/src/tree_sitter/array.h": "56fc8cd470996ded64a5452be25e26dbd2aa682a",
            "cfscript/src/tree_sitter/parser.h": "858107deb9e4a1ffcd2ed5e6ffe98a5fcab3e1f6",
            "common/scanner.h": "2e938459c2000aae38b0a4d20caf2f7b1eb2b879",
            "common/tag.h": "d441dc4fc5f21b35db1d279be38e2e19ec8b6ab7",
        },
        "license_blob": "aaf7a85db9f6e80c1ade3484da50e719087fe4a8",
        "licenses": ("MIT",),
        "notice_texts": (),
        "spdx": "MIT",
        "copyrights": ("Copyright (c) 2014 Gareth Edwards & Gavin Baumanis",),
        "extra_copyrights": (),
        "author": None,
    },
    "tree-sitter-vb6": {
        # No LICENSE file. package.json, Cargo.toml, pyproject.toml, tree-sitter.json: MIT, author
        # "joanne".
        "repo": "https://github.com/joannefan/tree-sitter-vb6",
        "commit": "1ee443353ce49f12e39a8e627b557c59e348b729",
        "files": {
            "src/grammar.json": "25414d73cd35ca6c6e451ab37cbc85c450ca122b",
            "src/node-types.json": "cf33cd9efa62bf898e90c024b3bd2a7121243b06",
            "src/parser.c": "0eb2591bd62ec5e7191f416801ef075a6da8188d",
            "src/tree_sitter/alloc.h": "1abdd1201578605b8d05a3001f01ebc0d58826d9",
            "src/tree_sitter/array.h": "a17a574f04e0c20c38463149640badc5126063f8",
            "src/tree_sitter/parser.h": "858107deb9e4a1ffcd2ed5e6ffe98a5fcab3e1f6",
        },
        "license_blob": None,
        "licenses": ("MIT",),
        "notice_texts": (),
        "spdx": "MIT",
        "copyrights": (),
        "extra_copyrights": (),
        "author": "joanne",
    },
    "tree-sitter-vba": {
        # No LICENSE file. package.json, Cargo.toml, tree-sitter.json: MIT; tree-sitter.json
        # author "Todd Epple".
        "repo": "https://github.com/tmepple/tree-sitter-vba",
        "commit": "c691f237b2a703732d4b6a1f01d5b4f73f94d41e",
        "files": {
            "src/grammar.json": "bda4515758c394bf567075b79b89f23bda8466b3",
            "src/node-types.json": "abc4087f379ac56d594aab22f0c5d7b7754e5b24",
            "src/parser.c": "7e4b74c8f5d1bdbb5fe417d6259a54a3e721b376",
            "src/tree_sitter/alloc.h": "1abdd1201578605b8d05a3001f01ebc0d58826d9",
            "src/tree_sitter/array.h": "56fc8cd470996ded64a5452be25e26dbd2aa682a",
            "src/tree_sitter/parser.h": "858107deb9e4a1ffcd2ed5e6ffe98a5fcab3e1f6",
        },
        "license_blob": None,
        "licenses": ("MIT",),
        "notice_texts": (),
        "spdx": "MIT",
        "copyrights": (),
        "extra_copyrights": (),
        "author": "Todd Epple",
    },
    "tree-sitter-vbscript": {
        # No LICENSE file. package.json: ISC, author "Jan-Jaap Korpershoek"; Cargo.toml and
        # pyproject.toml: MIT. The declarations conflict, so the tree ships under both.
        "repo": "https://github.com/JJK96/tree-sitter-vbscript",
        "commit": "6d9548eab0514e0572a616fd58d5997a93b37415",
        "files": {
            "src/grammar.json": "5ea3c12e9fb2e433229523de62928c5186b10087",
            "src/node-types.json": "f19438497a64026d6452ac4f98302315ff94b6f0",
            "src/parser.c": "b4d8d08cd50a95c30dcfc6a771059f076e2de831",
            "src/tree_sitter/alloc.h": "1f4466d75c40b6e62728b2de94c82c6398e42151",
            "src/tree_sitter/array.h": "15a3b233bbb8760a5d87089b259fe0122da75c8a",
            "src/tree_sitter/parser.h": "17f0e94bfcf4d8a54615c24fe61820a18e4d1411",
        },
        "license_blob": None,
        "licenses": ("MIT", "ISC"),
        "notice_texts": ("ISC",),
        "spdx": "MIT AND ISC",
        "copyrights": (),
        "extra_copyrights": (),
        "author": "Jan-Jaap Korpershoek",
    },
    "tree-sitter-cobol-scanner": {
        # LICENSE at that commit. scanner.c carries four WICKED PATCHes and arborium's packaging
        # edits (the arborium-cobol 2.18.0 crate declares MIT, author Amos Wenger; arborium's
        # LICENSE-MIT: "Copyright (c) 2024 Amos Wenger"), so no source file is pinned here.
        "repo": "https://github.com/zharinov/tree-sitter-cobol",
        "commit": "befb226951b65585617f42ecbe526d00c124100c",
        "files": {},
        "license_blob": "ff8ed93cb0dd57d68feeefe4829dbc9083584b5e",
        "licenses": ("MIT",),
        "notice_texts": (),
        "spdx": None,
        "copyrights": ("Copyright (c) 2016 Max Brunsfeld",),
        "extra_copyrights": ("Copyright (c) 2024 Amos Wenger",),
        "author": None,
    },
    "tree-sitter-apexlang": {
        # LICENSE.txt at that commit; the grammar is authored here from its EBNF, so no source
        # file is pinned here (the fixtures are listed in their own MANIFEST.tsv).
        "repo": "https://github.com/oracle/skills",
        "commit": "014cdb29f12dfa6a689bbe8a80ee1c5f0bf15be1",
        "files": {},
        "license_blob": "46c0c79d9927710e5dd88cad7b31eddb33bd39dd",
        "licenses": ("Universal Permissive License (UPL), Version 1.0",),
        "notice_texts": (),
        "spdx": None,
        "copyrights": ("Copyright (c) 2025 Oracle and/or its affiliates.",),
        "extra_copyrights": (),
        "author": None,
    },
}

NO_COPYRIGHT = "states no copyright notice"

# The standard license bodies (from the grant through the disclaimer), compared with whitespace
# collapsed. A LICENSE we write for an upstream that ships none must carry each declared one.
LICENSE_BODIES = {
    "MIT": (
        'Permission is hereby granted, free of charge, to any person obtaining a copy of this '
        'software and associated documentation files (the "Software"), to deal in the Software '
        'without restriction, including without limitation the rights to use, copy, modify, merge, '
        'publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons '
        'to whom the Software is furnished to do so, subject to the following conditions: The above '
        'copyright notice and this permission notice shall be included in all copies or substantial '
        'portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, '
        'EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS '
        'FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT '
        'HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF '
        'CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE '
        'USE OR OTHER DEALINGS IN THE SOFTWARE.'
    ),
    "ISC": (
        'Permission to use, copy, modify, and/or distribute this software for any purpose with or '
        'without fee is hereby granted, provided that the above copyright notice and this permission '
        'notice appear in all copies. THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL '
        'WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY '
        'AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR '
        'CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, '
        'WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN '
        'CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.'
    ),
}


def git_blob(path):
    """The git blob sha of a file's bytes, as `git hash-object` gives it."""
    data = path.read_bytes()
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()


def collapsed(text):
    """`text` with each run of whitespace (and NOTICE's indentation) as one space."""
    return " ".join(text.split())


def copyright_lines(text):
    return tuple(m.group(0).strip() for m in re.finditer(r"Copyright \(c\)[^\n]*", text))


def notice_block(notice, tree):
    """The NOTICE entry (between separator lines) that names `tree`'s path."""
    path = f"crates/wicked-estate-extract/vendor/{tree}/"
    for block in re.split(r"\n-{20,}\n", notice):
        if path in block:
            return block
    return ""


def declaration_problems(vendor, notice, declarations=UPSTREAM_DECLARATIONS):
    """Every way a tree's PROVENANCE row, files, LICENSE, NOTICE entry or crate manifest differs
    from what its upstream declares; empty when they match."""
    problems = []
    rows = {sub: line for line in (vendor / "PROVENANCE.md").read_text().splitlines()
            for sub in [m.group(1) for m in [re.search(r"\| `(tree-sitter-[\w-]+)/` \|", line)] if m]}
    for tree, d in sorted(declarations.items()):
        holders = d["copyrights"] + d["extra_copyrights"]
        # A row or NOTICE line may go on after the holder ("...; `tree/LICENSE`"), so compare
        # without a closing period.
        stems = tuple(h.rstrip(".") for h in holders)
        # The row: upstream, commit, declared licenses and holders, nothing else.
        row = rows.get(tree, "")
        cells = [c.strip() for c in row.strip().strip("|").split("|")]
        license_cell = cells[5] if len(cells) == 7 else ""
        if d["repo"] not in row or d["commit"] not in row:
            problems.append(f"{tree}: PROVENANCE row does not name {d['repo']} at {d['commit']}")
        for name in d["licenses"] + stems[:len(d["copyrights"])] + ((d["author"], NO_COPYRIGHT) if d["author"] else ()):
            if name not in license_cell:
                problems.append(f"{tree}: PROVENANCE license cell lacks {name!r}")
        for line in copyright_lines(license_cell):
            if not line.startswith(stems):
                problems.append(f"{tree}: PROVENANCE license cell states {line!r}, which upstream does not")
        # The files the record pins are the upstream bytes.
        for rel, sha in d["files"].items():
            path = vendor / tree / rel
            if not path.is_file() or git_blob(path) != sha:
                problems.append(f"{tree}: {rel} is not upstream's blob {sha}")
        # LICENSE: upstream's own file unchanged, or one that states the declaration.
        license_file = vendor / tree / "LICENSE"
        text = license_file.read_text() if license_file.is_file() else ""
        if copyright_lines(text) != d["copyrights"]:
            problems.append(f"{tree}: LICENSE copyright lines {copyright_lines(text)} != upstream's {d['copyrights']}")
        if d["license_blob"] is not None:
            if not license_file.is_file() or git_blob(license_file) != d["license_blob"]:
                problems.append(f"{tree}: LICENSE is not upstream's blob {d['license_blob']}")
        else:
            for name in (d["repo"], d["commit"], d["author"], NO_COPYRIGHT):
                if name not in text:
                    problems.append(f"{tree}: LICENSE does not state {name!r}")
            for name in d["licenses"]:
                if collapsed(LICENSE_BODIES[name]) not in collapsed(text):
                    problems.append(f"{tree}: LICENSE lacks the {name} license text")
        # NOTICE: upstream, commit, licenses, holders (and no other), texts it must carry.
        block = notice_block(notice, tree)
        flat = collapsed(block)
        for name in (d["repo"], d["commit"]) + d["licenses"] + holders + ((d["author"], NO_COPYRIGHT) if d["author"] else ()):
            if collapsed(name) not in flat:
                problems.append(f"{tree}: NOTICE entry lacks {name!r}")
        for line in copyright_lines(block):
            if not line.startswith(stems):
                problems.append(f"{tree}: NOTICE entry states {line!r}, which upstream does not")
        for name in d["notice_texts"]:
            if collapsed(LICENSE_BODIES[name]) not in flat:
                problems.append(f"{tree}: NOTICE entry lacks the {name} license text")
        # The crate: the row's SPDX expression, and a package that carries the LICENSE.
        cargo = vendor / tree / "Cargo.toml"
        if d["spdx"] is not None:
            package = tomllib.loads(cargo.read_text()).get("package", {}) if cargo.is_file() else {}
            if package.get("license") != d["spdx"]:
                problems.append(f"{tree}: Cargo.toml license {package.get('license')!r} != {d['spdx']!r}")
            if "license-file" in package:
                problems.append(f"{tree}: Cargo.toml sets license-file; use license = {d['spdx']!r}")
            include, exclude = package.get("include"), package.get("exclude", [])
            if (include is not None and "LICENSE" not in include) or "LICENSE" in exclude:
                problems.append(f"{tree}: Cargo.toml packages no LICENSE")
    return problems


class UpstreamDeclarationTests(unittest.TestCase):
    """#322 (operator ruling): each vendored tree states exactly what its upstream declares."""

    def setUp(self):
        self.notice = (ROOT / "NOTICE").read_text()

    def copy(self, tmp):
        vendor = Path(tmp) / "vendor"
        shutil.copytree(VENDOR, vendor, ignore=shutil.ignore_patterns("corpus", "test"))
        return vendor

    def test_every_upstream_derived_tree_matches_its_declaration(self):
        self.assertEqual(declaration_problems(VENDOR, self.notice), [])

    def test_every_third_party_tree_has_a_declaration(self):
        third_party = {sub for sub, source, lic in provenance_rows(VENDOR)
                       if not source.startswith("In-house") or f"`{sub}/LICENSE`" in lic}
        self.assertEqual(sorted(third_party), sorted(UPSTREAM_DECLARATIONS))

    def test_the_license_bodies_are_the_standard_texts(self):
        # The cfml LICENSE is upstream's own file (blob-pinned): its MIT body is the standard one.
        cfml = collapsed((VENDOR / "tree-sitter-cfml" / "LICENSE").read_text())
        self.assertIn(collapsed(LICENSE_BODIES["MIT"]), cfml)

    def test_an_invented_copyright_line_is_caught(self):
        with tempfile.TemporaryDirectory() as tmp:
            vendor = self.copy(tmp)
            lic = vendor / "tree-sitter-vba" / "LICENSE"
            lic.write_text("Copyright (c) 2023 tmepple\n" + lic.read_text())
            self.assertEqual(declaration_problems(vendor, self.notice),
                             ["tree-sitter-vba: LICENSE copyright lines ('Copyright (c) 2023 tmepple',) != upstream's ()"])
        notice = self.notice.replace("states no copyright notice, so none is reproduced here.\n"
                                     "             The license text ships in the directory (LICENSE).",
                                     "states no copyright notice, so none is reproduced here.\n"
                                     "             Copyright (c) 2023 tmepple", 1)
        self.assertIn("tree-sitter-vb6: NOTICE entry states 'Copyright (c) 2023 tmepple', which upstream does not",
                      declaration_problems(VENDOR, notice))

    def test_a_removed_license_text_is_caught(self):
        with tempfile.TemporaryDirectory() as tmp:
            vendor = self.copy(tmp)
            lic = vendor / "tree-sitter-vbscript" / "LICENSE"
            lic.write_text(lic.read_text().split("MIT License")[0])
            self.assertEqual(declaration_problems(vendor, self.notice),
                             ["tree-sitter-vbscript: LICENSE lacks the MIT license text",
                              "tree-sitter-vbscript: LICENSE lacks the ISC license text"])
        notice = self.notice.replace("Permission to use, copy, modify", "Permission to copy", 1)
        self.assertEqual(declaration_problems(VENDOR, notice),
                         ["tree-sitter-vbscript: NOTICE entry lacks the ISC license text"])

    def test_a_wrong_row_or_notice_attribution_is_caught(self):
        with tempfile.TemporaryDirectory() as tmp:
            vendor = self.copy(tmp)
            prov = vendor / "PROVENANCE.md"
            rows = prov.read_text().splitlines()
            at = next(i for i, r in enumerate(rows) if "| `tree-sitter-vba/` |" in r)
            cells = rows[at].split("|")
            cells[6] = " Apache-2.0, Copyright (c) 1900 Wrong Person "
            rows[at] = "|".join(cells)
            prov.write_text("\n".join(rows) + "\n")
            problems = declaration_problems(vendor, self.notice)
        self.assertIn("tree-sitter-vba: PROVENANCE license cell lacks 'MIT'", problems)
        self.assertIn("tree-sitter-vba: PROVENANCE license cell states 'Copyright (c) 1900 Wrong Person', "
                      "which upstream does not", problems)
        notice = self.notice.replace("c691f237b2a703732d4b6a1f01d5b4f73f94d41e", "0" * 40)
        self.assertEqual(declaration_problems(VENDOR, notice),
                         ["tree-sitter-vba: NOTICE entry lacks 'c691f237b2a703732d4b6a1f01d5b4f73f94d41e'"])

    def test_a_changed_upstream_file_or_license_is_caught(self):
        with tempfile.TemporaryDirectory() as tmp:
            vendor = self.copy(tmp)
            parser = vendor / "tree-sitter-vb6" / "src" / "parser.c"
            parser.write_bytes(parser.read_bytes() + b"\n")
            lic = vendor / "tree-sitter-cfml" / "LICENSE"
            lic.write_text(lic.read_text().replace("MERCHANTABILITY", "MERCHANTIBILITY"))
            self.assertEqual(declaration_problems(vendor, self.notice),
                             ["tree-sitter-cfml: LICENSE is not upstream's blob aaf7a85db9f6e80c1ade3484da50e719087fe4a8",
                              "tree-sitter-vb6: src/parser.c is not upstream's blob "
                              + UPSTREAM_DECLARATIONS["tree-sitter-vb6"]["files"]["src/parser.c"]])

    def test_a_crate_that_drops_its_license_is_caught(self):
        with tempfile.TemporaryDirectory() as tmp:
            vendor = self.copy(tmp)
            cargo = vendor / "tree-sitter-vba" / "Cargo.toml"
            # Valid TOML a line-based regex would misread: no spaces, a multi-line array.
            cargo.write_text(cargo.read_text().replace(
                'include = ["LICENSE", "README.md", "bindings/rust/*.rs", "src/**/*"]',
                'include=[\n  "README.md",\n  "bindings/rust/*.rs",\n  "src/**/*",\n]'))
            vbs = vendor / "tree-sitter-vbscript" / "Cargo.toml"
            vbs.write_text(vbs.read_text().replace('license = "MIT AND ISC"', "license = 'MIT AND ISC'"))
            self.assertEqual(declaration_problems(vendor, self.notice),
                             ["tree-sitter-vba: Cargo.toml packages no LICENSE"])

    def test_a_dropped_declaration_or_commit_is_caught(self):
        problems = declaration_problems(VENDOR, self.notice.replace("ISC", ""))
        self.assertIn("tree-sitter-vbscript: NOTICE entry lacks 'ISC'", problems)
        with tempfile.TemporaryDirectory() as tmp:
            vendor = self.copy(tmp)
            prov = vendor / "PROVENANCE.md"
            prov.write_text(prov.read_text().replace("1ee443353ce49f12e39a8e627b557c59e348b729", "unknown"))
            self.assertEqual(declaration_problems(vendor, self.notice),
                             ["tree-sitter-vb6: PROVENANCE row does not name "
                              "https://github.com/joannefan/tree-sitter-vb6 at 1ee443353ce49f12e39a8e627b557c59e348b729"])


class VendoredGrammarProvenanceTests(unittest.TestCase):
    def test_trees_are_found(self):
        self.assertGreaterEqual(len(vendored_trees(VENDOR)), 12)

    def test_every_tree_is_recorded_and_attributed(self):
        self.assertEqual(provenance_problems(VENDOR, (ROOT / "NOTICE").read_text()), [])

    def test_a_lost_or_emptied_tree_license_is_caught(self):
        cases = (("tree-sitter-cfml", "Copyright (c) 2014 Gareth Edwards & Gavin Baumanis"),
                 ("tree-sitter-vb6", None),
                 ("tree-sitter-apexlang", "Copyright (c) 2025 Oracle and/or its affiliates."))
        for tree, line in cases:
            notice = (ROOT / "NOTICE").read_text().replace(line or "\0", "")
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
        line = "Copyright (c) 2016 Max Brunsfeld"
        notice = (ROOT / "NOTICE").read_text().replace(line, "")
        self.assertEqual(provenance_problems(VENDOR, notice),
                         [f"tree-sitter-cobol-scanner: {line!r} missing from NOTICE"])

    def test_a_tree_without_a_row_is_caught(self):
        with tempfile.TemporaryDirectory() as tmp:
            vendor = Path(tmp) / "vendor"
            shutil.copytree(VENDOR, vendor, ignore=shutil.ignore_patterns("*.c", "*.h", "*.json", "corpus"))
            (vendor / "tree-sitter-new").mkdir()
            problems = provenance_problems(vendor, (ROOT / "NOTICE").read_text())
            self.assertEqual(len(problems), 2, problems)


if __name__ == "__main__":
    unittest.main()
