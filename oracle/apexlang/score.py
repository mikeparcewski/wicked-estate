#!/usr/bin/env python3
"""Score oracle/apexlang/labels.tsv: precision with a Wilson 95% lower bound (floor 0.97, n >= 200).

  score.py labels.tsv
"""
import collections
import math
import sys


def wilson_lower(k, n, z=1.959963984540054):
    if n == 0:
        return 0.0
    p = k / n
    d = 1 + z * z / n
    c = p + z * z / (2 * n)
    a = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    return (c - a) / d


def main():
    rows = []
    with open(sys.argv[1], encoding="utf-8") as fh:
        header = fh.readline().rstrip("\n").split("\t")
        for line in fh:
            rows.append(dict(zip(header, line.rstrip("\n").split("\t"))))
    verdicts = collections.Counter(r["label"] for r in rows)
    by_prop = collections.defaultdict(collections.Counter)
    for r in rows:
        by_prop[r["property"]][r["label"]] += 1
    k, n = verdicts["correct"], verdicts["correct"] + verdicts["wrong"]
    lb = wilson_lower(k, n)
    print(f"edges {len(rows)}: correct {k}, wrong {verdicts['wrong']}, unlabelled {verdicts['unlabelled']}")
    print(f"precision {k}/{n} = {k / n if n else 0:.4f}; Wilson 95% lower bound {lb:.4f}")
    print("property\tcorrect\twrong\tunlabelled")
    for p, c in sorted(by_prop.items(), key=lambda kv: -sum(kv[1].values())):
        print(f"{p}\t{c['correct']}\t{c['wrong']}\t{c['unlabelled']}")
    ok = n >= 200 and lb >= 0.97
    print("SHIPS" if ok else "DOES NOT SHIP (needs n >= 200 and lower bound >= 0.97)")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
