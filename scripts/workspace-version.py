#!/usr/bin/env python3
"""Print the workspace version (`[workspace.package] version` in the root Cargo.toml).

Used by the binaries/CI workflows (wicked-estate#360) to know what `--version` must report.
"""
import re
from pathlib import Path

text = (Path(__file__).resolve().parent.parent / "Cargo.toml").read_text(encoding="utf-8")
section = text.split("[workspace.package]", 1)[1]
print(re.search(r'^version\s*=\s*"([^"]+)"', section, re.M).group(1))
