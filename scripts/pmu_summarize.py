#!/usr/bin/env python3
"""Reduce one xctrace CPU Counters export (MetricAggregationForThread) to
one JSON line per benchmark thread.

The PMU pass (scripts/run-m4-pmu-pass.sh) runs each case on its own thread
named `case:<id>` (run_matrix MATRIX_THREAD_PER_CASE=1) and the femtovg E2E
on `femtovg-e2e`, so xctrace's per-thread aggregation attributes the
counters to cases.

The table holds every interval twice: precise rows (`is-precise` Yes,
bounded by context switches) and imprecise ones (No, 10 ms buckets), with
identical totals, so only one kind is summed (precise when present). Per
thread this prints:

  cycles   the `cycle` metric (CORE_ACTIVE_CYCLE)
  counts   sums of the integer metrics (event counts, e.g. l1d_miss_ld_spec)
  weights  sums of the fractional metrics. In the bottleneck mode these are
           the useful / processing / delivery / discarded slot weights, and
           `shares` gives each as a percentage of their total (the four-bucket
           view of scripts/analyze_pmu.py). In the delivery mode they are the
           delivery sub-buckets, and `shares` is their split of delivery.

Usage: pmu_summarize.py EXPORT.xml RUNTIME MODE [E2E_CASE] >> pmu.jsonl
  E2E_CASE names the `femtovg-e2e` thread's case (e.g. femtovg_e2e.scene0).
"""
import collections
import json
import sys
import xml.etree.ElementTree as ET

BUCKETS = {
    "bottlenecks": ["useful", "processing", "delivery", "discarded"],
    "delivery": ["delivery_latency_icache", "delivery_latency_itlb", "delivery_latency_other",
                 "delivery_bandwidth"],
}


def main():
    path, runtime, mode = sys.argv[1:4]
    e2e_case = sys.argv[4] if len(sys.argv) > 4 else "femtovg-e2e"
    ids = {}
    # (thread, precise) -> metric -> sum
    ints = collections.defaultdict(collections.Counter)
    dbls = collections.defaultdict(lambda: collections.defaultdict(float))
    for _, elem in ET.iterparse(path, events=("end",)):
        if elem.tag != "row":
            if elem.get("id"):
                ids[elem.get("id")] = elem
            continue
        cells = []
        for c in elem:
            if c.get("id"):
                ids[c.get("id")] = c
            cells.append(ids[c.get("ref")] if c.get("ref") else c)
        elem.clear()  # the cells stay reachable through `ids`
        if len(cells) < 8:
            continue
        thread = (cells[2].get("fmt") or "").split(" (0x")[0]
        if not (thread.startswith("case:") or thread == "femtovg-e2e"):
            continue
        precise = (cells[7].get("fmt") or cells[7].text or "") in ("Yes", "1", "true")
        metric = cells[6].get("fmt") or cells[6].text
        ints[(thread, precise)][metric] += int(cells[4].text or 0)
        try:
            dbls[(thread, precise)][metric] += float(cells[5].text or 0)
        except ValueError:
            pass
    threads = sorted({t for t, _ in ints})
    for thread in threads:
        key = (thread, True) if (thread, True) in ints else (thread, False)
        m, d = ints[key], dbls[key]
        weights = {k: v for k, v in d.items() if v}
        shares = {}
        base = mode.split(":")[-1]
        if base in BUCKETS:
            total = sum(weights.get(b, 0.0) for b in BUCKETS[base])
            if total:
                shares = {b: round(100.0 * weights.get(b, 0.0) / total, 2) for b in BUCKETS[base]}
        print(json.dumps({
            "runtime": runtime,
            "mode": mode,
            "thread": thread,
            "case": thread[5:] if thread.startswith("case:") else e2e_case,
            "cycles": m.get("cycle", 0),
            "counts": {k: v for k, v in m.items() if k != "cycle" and v},
            "weights": {k: round(v, 3) for k, v in weights.items()},
            "shares": shares,
        }))


if __name__ == "__main__":
    main()
