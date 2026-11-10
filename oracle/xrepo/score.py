#!/usr/bin/env python3
"""Score the project linker's package-import links against labels (PLAN §6.3/§6.4, slice X1).

usage: score.py <labels.json> [--floor 0.97] [--min-n 50]

labels.json: [{"case": str, "from_file": str, "to_file": str, "identity": str,
               "label": "true" | "false", "reason": str}, ...]
  one row per PREDICTED link, labelled independently of the linker (a reviewer read the importing
  line and the provider file and judged whether the import resolves to that file).

Prints per (kind, identity) and overall: n, correct, precision, the Wilson 95% lower bound, and
whether the link kind clears the floor (lower bound >= floor with n >= min-n). Exit 1 when it does
not: a kind that does not qualify ships its edges as "possible" items only (PLAN §6.4).
"""
import json
import math
import sys
from collections import defaultdict

Z = 1.959964  # 95%


def wilson_lower(k: int, n: int) -> float:
    if n == 0:
        return 0.0
    p = k / n
    d = 1 + Z * Z / n
    c = p + Z * Z / (2 * n)
    r = Z * math.sqrt(p * (1 - p) / n + Z * Z / (4 * n * n))
    return (c - r) / d


def main() -> int:
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 2
    floor, min_n = 0.97, 50
    if "--floor" in args:
        floor = float(args[args.index("--floor") + 1])
    if "--min-n" in args:
        min_n = int(args[args.index("--min-n") + 1])
    rows = json.load(open(args[0]))
    groups = defaultdict(lambda: [0, 0])
    for r in rows:
        ok = r["label"] == "true"
        for key in (("package-import", r.get("identity", "?")), ("package-import", "*"), ("case:" + r.get("case", "?"), "*")):
            groups[key][0] += 1
            groups[key][1] += ok
    print(f"{'kind':<34}{'identity':<32}{'n':>5}{'ok':>5}{'prec':>8}{'wilson-lb':>11}")
    for (kind, ident), (n, k) in sorted(groups.items()):
        print(f"{kind:<34}{ident:<32}{n:>5}{k:>5}{k / n:>8.3f}{wilson_lower(k, n):>11.4f}")
    n, k = groups[("package-import", "*")]
    lb = wilson_lower(k, n)
    ships = n >= min_n and lb >= floor
    print(f"\nPACKAGE-IMPORT LINKS: n={n} correct={k} wilson95-lower={lb:.4f} floor={floor} min-n={min_n} -> "
          + ("SHIPS" if ships else "DOES NOT QUALIFY (edges stay 'possible')"))
    # PLAN §6.4 gates per link KIND and RANK (one here: package-import / package-manifest). The
    # identity rule is not a rank, but a wrong link under any identity is named so it is not
    # averaged away: every identity with an error, or too few labels to judge on its own.
    for (kind, ident), (gn, gk) in sorted(groups.items()):
        if kind != "package-import" or ident == "*":
            continue
        if gk < gn:
            print(f"NOTE: identity {ident}: {gn - gk} wrong of {gn} (wilson95-lower {wilson_lower(gk, gn):.4f})")
        elif gn < min_n:
            print(f"NOTE: identity {ident}: n={gn} < {min_n}, not judged on its own")
    return 0 if ships else 1


if __name__ == "__main__":
    sys.exit(main())
