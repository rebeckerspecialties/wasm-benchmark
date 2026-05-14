#!/usr/bin/env python3
"""Aggregate xctrace CounterMetricByThread XML rows into per-core-type
totals + ratios. Each `value` array has 4 entries from the CPU Counters
"CPU Bottlenecks" mode: (Useful, Processing, Delivery, Discarded).

Usage:
    analyze_pmu.py LABEL_A path/a.xml LABEL_B path/b.xml

Pass labels for each file so the diff table is self-describing.
"""

import re
import sys
import xml.etree.ElementTree as ET
from collections import defaultdict


class Resolver:
    """Resolve xctrace's id/ref-pattern XML where elements are written
    once with `id="N"` and reused elsewhere via `ref="N"`."""

    def __init__(self):
        self.by_id = {}

    def add(self, elem):
        i = elem.get("id")
        if i is not None:
            self.by_id[i] = elem

    def resolve(self, elem):
        ref = elem.get("ref")
        if ref is not None:
            return self.by_id[ref]
        return elem


def parse_rows(path):
    """Yield {ctype, dur_ns, useful, processing, delivery, discarded} per row."""
    res = Resolver()
    for ev, elem in ET.iterparse(path, events=("end",)):
        if elem.tag != "row":
            continue
        for child in elem:
            res.add(child)
        core_el = arr_el = dur_el = None
        for child in elem:
            r = res.resolve(child)
            if r.tag == "core":
                core_el = r
            elif r.tag == "uint64-array":
                arr_el = r
            elif r.tag == "duration":
                dur_el = r
        if core_el is None or arr_el is None:
            elem.clear()
            continue
        m = re.match(r"CPU (\d+) \((P|E) Core\)", core_el.get("fmt") or "")
        if not m:
            elem.clear()
            continue
        ctype = m.group(2)
        nums = (arr_el.text or "").strip().split()
        if len(nums) != 4:
            elem.clear()
            continue
        try:
            useful, processing, delivery, discarded = [int(x) for x in nums]
        except ValueError:
            elem.clear()
            continue
        dur_ns = 0
        if dur_el is not None and dur_el.text:
            try:
                dur_ns = int(dur_el.text)
            except ValueError:
                pass
        yield {
            "ctype": ctype,
            "dur_ns": dur_ns,
            "useful": useful,
            "processing": processing,
            "delivery": delivery,
            "discarded": discarded,
        }
        elem.clear()


def aggregate(path):
    by_core = defaultdict(
        lambda: {
            "useful": 0,
            "processing": 0,
            "delivery": 0,
            "discarded": 0,
            "dur_ns": 0,
            "samples": 0,
        }
    )
    for row in parse_rows(path):
        b = by_core[row["ctype"]]
        for k in ("useful", "processing", "delivery", "discarded", "dur_ns"):
            b[k] += row[k]
        b["samples"] += 1
    return by_core


def main():
    if len(sys.argv) != 5:
        print("usage: analyze_pmu.py LABEL_A a.xml LABEL_B b.xml", file=sys.stderr)
        sys.exit(2)
    label_a, path_a, label_b, path_b = sys.argv[1:]
    a = aggregate(path_a)
    b = aggregate(path_b)

    for ctype, name in [("E", "E-core"), ("P", "P-core")]:
        ad, bd = a.get(ctype), b.get(ctype)
        if not ad or not bd:
            continue
        print(f"\n=== {name} aggregate ({label_a} → {label_b}) ===")
        ad_total = sum(ad[k] for k in ("useful", "processing", "delivery", "discarded"))
        bd_total = sum(bd[k] for k in ("useful", "processing", "delivery", "discarded"))
        header_a = f"{label_a:<10}"
        header_b = f"{label_b:<10}"
        print(f"                       {header_a:>14}  {header_b:>14}     Δ abs       Δ %")
        for k in ("useful", "processing", "delivery", "discarded"):
            av, bv = ad[k], bd[k]
            delta_abs = bv - av
            delta_pct = (100.0 * delta_abs / av) if av else float("nan")
            print(
                f"  {k:<11}        {av:>14,d}  {bv:>14,d}  {delta_abs:>+12,d}  {delta_pct:>+6.2f}%"
            )
        print(
            f"  {'TOTAL':<11}        {ad_total:>14,d}  {bd_total:>14,d}  "
            f"{bd_total - ad_total:>+12,d}  "
            f"{100.0 * (bd_total - ad_total) / ad_total:>+6.2f}%"
        )
        print(f"\n  share of total cycles taken by each bottleneck:")
        print(f"                       {header_a:>14}  {header_b:>14}")
        for k in ("useful", "processing", "delivery", "discarded"):
            av_pct = f"{100.0 * ad[k] / ad_total:5.1f}%" if ad_total else "—"
            bv_pct = f"{100.0 * bd[k] / bd_total:5.1f}%" if bd_total else "—"
            print(f"  {k:<11}        {av_pct:>14}  {bv_pct:>14}")
        print(
            f"\n  samples: {ad['samples']:,} {label_a} | {bd['samples']:,} {label_b}"
        )
        print(
            f"  total duration: {ad['dur_ns'] / 1e6:.1f} ms {label_a} | "
            f"{bd['dur_ns'] / 1e6:.1f} ms {label_b}"
        )


if __name__ == "__main__":
    main()
