#!/usr/bin/env python3
"""Summarize a tinywasm A/B from scripts/tinywasm-ab-iphone.sh.

Usage: tinywasm_ab_summary.py <ab-dir> <variant>... [--steps new:old,...] [--csv out.csv]

The first variant is the baseline. Per case: the median over reps of cycles
per call, as a ratio to the baseline for every other variant, then the
geomean. Per step `new:old` (default: each variant against the one before
it, then the last against the baseline): the geomean change in cycles and
instructions per call and how many cases got faster. Every variant must
return the same result on every case; a mismatch is reported.
"""
import argparse
import csv
import glob
import math
import os
import re
import statistics
from collections import defaultdict

LINE = re.compile(r"^\[\[tinywm\] (.*?)\] result=(-?\d+)\s+iter=(\d+).*?cpu\(u/s\)=([\d.]+)/[\d.]+ ms.*?"
                  r"e_share=([\d.]+)\s+ipc=([\d.]+)\s+insns=(\d+)\s+cycles=(\d+)")


def geo(xs):
    return math.exp(sum(math.log(x) for x in xs) / len(xs))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dir")
    ap.add_argument("variants", nargs="+")
    ap.add_argument("--steps", help="comma-separated new:old pairs")
    ap.add_argument("--csv", help="write every sample here")
    args = ap.parse_args()
    variants = args.variants
    base = variants[0]
    if args.steps:
        steps = [tuple(s.split(":")) for s in args.steps.split(",")]
    else:
        steps = list(zip(variants[1:], variants[:-1]))
        if len(variants) > 2:
            steps.append((variants[-1], base))

    data = defaultdict(lambda: defaultdict(list))  # case -> variant -> samples
    rows = []
    for v in variants:
        for log in sorted(glob.glob(os.path.join(args.dir, v, "*.log"))):
            m = re.search(r"-r(\d+)-tinywasm-r\d+\.log$", log)
            rep = int(m.group(1)) if m else 0
            for line in open(log, errors="replace"):
                m = LINE.match(line.strip())
                if not m:
                    continue
                it = int(m.group(3))
                s = dict(rep=rep, variant=v, case=m.group(1), result=int(m.group(2)), iterations=it,
                         cycles_per_call=int(m.group(8)) / it, instructions_per_call=int(m.group(7)) / it,
                         cpu_ms_per_call=float(m.group(4)) / it, e_share=float(m.group(5)))
                data[s["case"]][v].append(s)
                rows.append(s)
    if not rows:
        raise SystemExit(f"no tinywasm result lines under {args.dir}")
    if args.csv:
        with open(args.csv, "w", newline="") as f:
            w = csv.DictWriter(f, fieldnames=list(rows[0]))
            w.writeheader()
            w.writerows(sorted(rows, key=lambda r: (r["case"], variants.index(r["variant"]), r["rep"])))

    def med(case, v, k):
        return statistics.median(s[k] for s in data[case][v])

    cases = [c for c in data if all(v in data[c] for v in variants)]
    reps = {v: len({s["rep"] for c in cases for s in data[c][v]}) for v in variants}
    print(f"{len(cases)} cases in every variant; reps: " + ", ".join(f"{v} {n}" for v, n in reps.items()))
    missing = sorted(set(data) - set(cases))
    if missing:
        print("cases missing from some variant:", ", ".join(missing))
    for c in cases:
        # Compare results between samples that made the same number of calls: a stateful
        # case (xmrsplayer renders the song's next buffer on every call) legitimately
        # returns a different value after a different number of calls.
        by_calls = defaultdict(set)
        for v in variants:
            for s in data[c][v]:
                by_calls[s["iterations"]].add(s["result"])
        bad = {n: sorted(r) for n, r in by_calls.items() if len(r) > 1}
        if bad:
            print(f"RESULT MISMATCH on {c} (per call count): {bad}")
    print()
    print(f"| case | {base} Mcycles/call | " + " | ".join(f"{v} ÷ {base}" for v in variants[1:]) + " |")
    print("|---|---:|" + "---:|" * (len(variants) - 1))
    for c in cases:
        b = med(c, base, "cycles_per_call")
        print(f"| {c} | {b / 1e6:.3f} | "
              + " | ".join(f"{med(c, v, 'cycles_per_call') / b:.3f}" for v in variants[1:]) + " |")
    print(f"| **geomean** | | " + " | ".join(
        f"**{geo([med(c, v, 'cycles_per_call') / med(c, base, 'cycles_per_call') for c in cases]):.3f}**"
        for v in variants[1:]) + " |")
    print()
    print("| step | cycles | instructions | cases faster |")
    print("|---|---:|---:|---:|")
    for new, old in steps:
        rc = [med(c, new, "cycles_per_call") / med(c, old, "cycles_per_call") for c in cases]
        ri = [med(c, new, "instructions_per_call") / med(c, old, "instructions_per_call") for c in cases]
        print(f"| {new} vs {old} | {100 * (geo(rc) - 1):+.1f} % | {100 * (geo(ri) - 1):+.1f} % | "
              f"{sum(1 for r in rc if r < 1)}/{len(rc)} |")
    print()
    for v in variants:
        spread = [(max(s["cycles_per_call"] for s in data[c][v]) - min(s["cycles_per_call"] for s in data[c][v]))
                  / med(c, v, "cycles_per_call") for c in cases if len(data[c][v]) > 1]
        es = min(s["e_share"] for c in cases for s in data[c][v])
        line = f"{v}: e_share min {es:.3f}"
        if spread:
            line += (f"; cycles/call spread across reps: median {100 * statistics.median(spread):.2f} %,"
                     f" max {100 * max(spread):.2f} %")
        print(line)


if __name__ == "__main__":
    main()
