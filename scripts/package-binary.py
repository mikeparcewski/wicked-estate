#!/usr/bin/env python3
"""Package one prebuilt target into its release archive (wicked-estate#360).

    package-binary.py <target> <version> <out-dir>

Reads `target/<target>/release/{wicked-estate,wicked-estate-mcp}[.exe]` and writes
`<out-dir>/wicked-estate-<version>-<target>/` (the two binaries plus LICENSE, NOTICE and
README.md) and next to it `wicked-estate-<version>-<target>.tar.gz` (`.zip` for a Windows
target). Prints the archive path. One script for every OS, like `smoke-binary.py`.
"""
import shutil
import sys
import tarfile
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
BINS = ("wicked-estate", "wicked-estate-mcp")
DOCS = ("LICENSE", "NOTICE", "README.md")


def main() -> None:
    if len(sys.argv) != 4:
        sys.exit("usage: package-binary.py <target> <version> <out-dir>")
    target, version, out = sys.argv[1], sys.argv[2].strip().lstrip("v"), Path(sys.argv[3]).resolve()
    windows = "windows" in target
    exe = ".exe" if windows else ""
    name = f"wicked-estate-{version}-{target}"
    stage = out / name
    shutil.rmtree(stage, ignore_errors=True)
    stage.mkdir(parents=True)
    for b in BINS:
        src = REPO / "target" / target / "release" / f"{b}{exe}"
        if not src.is_file():
            sys.exit(f"missing {src} — build it with --target {target} --release first")
        shutil.copy2(src, stage / src.name)
    for d in DOCS:
        shutil.copy2(REPO / d, stage / d)
    if windows:
        archive = out / f"{name}.zip"
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as z:
            for f in sorted(stage.iterdir()):
                z.write(f, f"{name}/{f.name}")
    else:
        archive = out / f"{name}.tar.gz"
        with tarfile.open(archive, "w:gz") as t:
            t.add(stage, arcname=name)
    print(archive)


if __name__ == "__main__":
    main()
