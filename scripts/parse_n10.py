#!/usr/bin/env python3
"""Aggregate per-rep iPhone 12 wallclock logs from `run_fusion_n10.sh`
into median + range per (workload, condition).

Usage:
    parse_n10.py out/exp-fusion-xband-brif/n10

Picks up `iphone12-{cond}-r{i}.log` files. Per log, scans for the
`[[<runtime>] <workload>]` result lines and pulls the
`median=<float>` value. Then prints a table per condition.
"""

import os
import re
import sys
from collections import defaultdict
from glob import glob
from statistics import median


LINE_RE = re.compile(
    r'^\[\[\s*(?P<runtime>\w+)\s*\]\s+(?P<workload>.+?)\]\s+'
    r'result=\S+\s+iter=\d+\s+load=\S+\s+'
    r'min=(?P<min>\S+)\s+median=(?P<median>\S+)\s+p99=(?P<p99>\S+)'
)


def normalize_workload(s):
    """Canonicalise the label-derived workload string for tabular display.
    Drop dispatch-count parentheticals like `(200K dispatches)` /
    `(1024-frame buffer)` / `(200K)` — they're informational, not
    distinguishing. But KEEP `(AS)` and `(Porffor)` since they
    distinguish graphql-validation variants.
    """
    s = s.strip()
    if "graphql-validation" in s:
        if "(AS)" in s:
            return "graphql-validation (AS)"
        if "(Porffor)" in s:
            return "graphql-validation (Porffor)"
    # strip trailing parenthetical "(...)" if present
    s = re.sub(r"\s*\([^)]*\)\s*$", "", s)
    return s


def parse_log(path):
    rows = []
    for line in open(path):
        m = LINE_RE.match(line)
        if m:
            rows.append({
                "runtime": m.group("runtime"),
                "workload": normalize_workload(m.group("workload")),
                "median_ms": float(m.group("median")),
                "min_ms": float(m.group("min")),
                "p99_ms": float(m.group("p99")),
            })
    return rows


def aggregate(logdir, cond):
    paths = sorted(glob(os.path.join(logdir, f"iphone12-{cond}-r*.log")))
    by_key = defaultdict(list)
    for p in paths:
        for r in parse_log(p):
            by_key[(r["runtime"], r["workload"])].append(r["median_ms"])
    return by_key, len(paths)


def fmt_summary(by_key, n):
    rows = []
    for (runtime, workload), vals in sorted(by_key.items()):
        rows.append({
            "runtime": runtime,
            "workload": workload,
            "n": len(vals),
            "med": median(vals),
            "min": min(vals),
            "max": max(vals),
            "range": max(vals) - min(vals),
        })
    return rows


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    logdir = sys.argv[1]
    print(f"# {logdir}")
    print()
    # Discover conditions
    conds = sorted(set(
        re.match(r'iphone12-(.+?)-r\d+\.log', os.path.basename(p)).group(1)
        for p in glob(os.path.join(logdir, 'iphone12-*-r*.log'))
        if re.match(r'iphone12-(.+?)-r\d+\.log', os.path.basename(p))
    ))
    summaries = {}
    for cond in conds:
        by_key, n = aggregate(logdir, cond)
        rows = fmt_summary(by_key, n)
        summaries[cond] = {(r["runtime"], r["workload"]): r for r in rows}
        print(f"## {cond} (N={n})")
        print()
        print(f"  {'runtime':<8} {'workload':<24} {'n':>3} {'med':>8} {'min':>8} {'max':>8} {'range':>8}")
        for r in rows:
            print(f"  {r['runtime']:<8} {r['workload']:<24} {r['n']:>3} "
                  f"{r['med']:>8.3f} {r['min']:>8.3f} {r['max']:>8.3f} {r['range']:>8.3f}")
        print()

    # Pairwise diff (if exactly two conditions: baseline, fusion)
    if len(conds) == 2 and "baseline" in conds and "fusion" in conds:
        print("## fusion - baseline (median delta)")
        print()
        print(f"  {'runtime':<8} {'workload':<24} {'base':>8} {'fused':>8} {'Δ ms':>8} {'Δ %':>8}")
        keys = sorted(set(summaries["baseline"]) & set(summaries["fusion"]))
        for k in keys:
            base = summaries["baseline"][k]["med"]
            fused = summaries["fusion"][k]["med"]
            dms = fused - base
            dpct = 100.0 * dms / base
            mark = " <<<" if abs(dpct) >= 1.0 else ""
            print(f"  {k[0]:<8} {k[1]:<24} {base:>8.3f} {fused:>8.3f} "
                  f"{dms:>+8.3f} {dpct:>+7.2f}%{mark}")


if __name__ == "__main__":
    main()
