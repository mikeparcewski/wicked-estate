#!/usr/bin/env python3
"""Report community-quality metrics from `wicked-estate clusters --json` (stdin).

Cross-platform (python3 only, no deps). Prints community count, the mega-community fraction (the
key regression signal), and a sample of each of the largest clusters for human module-alignment
review. Exits non-zero if the mega-community gate (max fraction >= 0.30) is breached.
"""
import json
import sys
import collections


def main() -> int:
    clusters = json.load(sys.stdin)
    if not clusters:
        print("no communities (empty graph or unindexed db)")
        return 0

    total = sum(len(c) for c in clusters)
    clusters.sort(key=len, reverse=True)
    largest = len(clusters[0])
    frac = largest / total if total else 0.0

    print(f"communities = {len(clusters)}")
    print(f"symbols clustered = {total}")
    print(f"largest community = {largest}  (fraction {frac:.3f})")
    gate = "PASS" if frac < 0.30 else "FAIL"
    print(f"mega-community gate (max fraction < 0.30): {gate}")

    print("\ntop communities (sampled members for module-alignment review):")
    for i, c in enumerate(clusters[:5]):
        # Heuristic: bucket members by a leading path/module segment for a quick alignment read.
        buckets = collections.Counter()
        for sym in c[:60]:
            seg = sym.replace("\\", "/").split("/")[0][:24]
            buckets[seg] += 1
        top = ", ".join(f"{k}:{v}" for k, v in buckets.most_common(4))
        print(f"  #{i + 1} size {len(c)}: {top}")

    return 0 if frac < 0.30 else 1


if __name__ == "__main__":
    sys.exit(main())
