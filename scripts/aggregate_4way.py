#!/usr/bin/env python3
"""4-way per-workload wallclock comparison: baseline, phase3, phase4, WAMR.

Reads N=10 logs from a single n10/ directory containing
iphone12-{baseline,phase3,phase4}-r{1..10}.log files. WAMR numbers come
from the same logs (each run produces 8 Pulley + 8 WAMR result lines).

Usage:
    aggregate_4way.py out/exp-3way/n10            # iPhone 12
    aggregate_4way.py out/exp-3way-xs/n10         # iPhone XS

Emits a Markdown table to stdout.
"""

import os
import re
import sys
from collections import defaultdict
from glob import glob
from statistics import median


LINE_RE = re.compile(
    r"^\[\[\s*(?P<runtime>\w+)\s*\]\s+(?P<workload>.+?)\]\s+"
    r"result=\S+\s+iter=\d+\s+load=\S+\s+"
    r"min=(?P<min>\S+)\s+median=(?P<median>\S+)\s+p99=(?P<p99>\S+)"
)


def normalize_workload(s):
    s = s.strip()
    if "graphql-validation" in s:
        if "(AS)" in s:
            return "graphql-validation (AS)"
        if "(Porffor)" in s:
            return "graphql-validation (Porffor)"
    return re.sub(r"\s*\([^)]*\)\s*$", "", s)


def parse_log(path):
    rows = []
    for line in open(path):
        m = LINE_RE.match(line)
        if m:
            rows.append(
                (
                    m.group("runtime"),
                    normalize_workload(m.group("workload")),
                    float(m.group("median")),
                )
            )
    return rows


def wallclock_medians(logdir, cond):
    by_key = defaultdict(list)
    for p in sorted(glob(os.path.join(logdir, f"iphone12-{cond}-r*.log"))):
        for rt, wl, med in parse_log(p):
            by_key[(rt, wl)].append(med)
    return {k: median(v) for k, v in by_key.items() if v}


WORKLOADS = [
    "call_indirect",
    "xmrsplayer",
    "vtable_mono",
    "vtable_bi",
    "vtable_poly4",
    "vtable_poly6",
    "graphql-validation (AS)",
    "graphql-validation (Porffor)",
]


def pct(a, b):
    if a == 0:
        return float("nan")
    return 100.0 * (b - a) / a


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    logdir = sys.argv[1]

    baseline = wallclock_medians(logdir, "baseline")
    phase3 = wallclock_medians(logdir, "phase3")
    phase4 = wallclock_medians(logdir, "phase4")

    # WAMR — use whichever cond produced WAMR rows; they should be identical
    # since WAMR doesn't change across the wasmtime branch. We average across
    # conds for the table to reduce noise.
    wamr_pools = defaultdict(list)
    for cond in ("baseline", "phase3", "phase4"):
        m = wallclock_medians(logdir, cond)
        for (rt, wl), v in m.items():
            if rt == "WAMR":
                wamr_pools[wl].append(v)
    wamr = {wl: median(vs) for wl, vs in wamr_pools.items()}

    print("| workload | baseline | phase3 | phase4 | WAMR | base→phase3 % | phase3→phase4 % | base→phase4 % | phase4 vs WAMR |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
    for wl in WORKLOADS:
        b = baseline.get(("Pulley", wl))
        p3 = phase3.get(("Pulley", wl))
        p4 = phase4.get(("Pulley", wl))
        w = wamr.get(wl)
        b_s = f"{b:.2f}" if b else "—"
        p3_s = f"{p3:.2f}" if p3 else "—"
        p4_s = f"{p4:.2f}" if p4 else "—"
        w_s = f"{w:.2f}" if w else "—"
        dp3 = f"{pct(b, p3):+.2f}%" if b and p3 else "—"
        d34 = f"{pct(p3, p4):+.2f}%" if p3 and p4 else "—"
        d04 = f"{pct(b, p4):+.2f}%" if b and p4 else "—"
        cmp_w = f"WAMR {p4 / w:.2f}× faster" if p4 and w else "—"
        print(f"| {wl} | {b_s} | {p3_s} | {p4_s} | {w_s} | {dp3} | {d34} | {d04} | {cmp_w} |")


if __name__ == "__main__":
    main()
