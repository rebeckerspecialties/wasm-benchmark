#!/usr/bin/env python3
"""Reduce one xctrace CPU Counters export (MetricAggregationForThread) to
one JSON line per benchmark thread.

The PMU pass (scripts/run-m4-pmu-pass.sh) runs each case on its own thread
named `case:<id>` (run_matrix MATRIX_THREAD_PER_CASE=1; a runtime's own
big-stack thread inside it takes the same name) and the femtovg E2E on
`femtovg-e2e`, so xctrace's per-thread aggregation attributes the counters
to cases. Threads with the same name are summed. The launched process's
other threads (main thread, the Metal driver's) get lines too, with
`case` null, so their share of the process's cycles is visible.

The table holds every interval twice: precise rows (`is-precise` Yes,
1 ms slices cut at context switches) and imprecise ones (No, 10 ms
buckets), with identical integer totals, so only one kind is used (precise
when present). Each interval carries a `cycle` row (CORE_ACTIVE_CYCLE) and
one row per metric of the mode. An integer metric's value is its event
count in the interval. A ratio metric's value is the sum of that ratio
over the interval's n on-core segments, so for a set of ratios that
partitions the pipeline slots (the four bottleneck buckets sum to 1 in
every segment) n is the interval's sum over the set. Per thread this
prints:

  cycles     the `cycle` total
  counts     totals of the integer metrics (event counts, e.g.
             l1d_miss_ld_spec, discarded_indirect_branch, indirect_branch)
  fractions  in the bottlenecks mode, each bucket's per-interval mean
             (value / n), averaged over the intervals weighted by their
             cycles: the fraction of the thread's pipeline slots
  shares     the same four buckets in %, the four-bucket view of
             scripts/analyze_pmu.py
  weights    raw sums of any other ratio metric (no exact aggregation
             exists for those from this table)

Usage: pmu_summarize.py EXPORT.xml RUNTIME MODE [E2E_CASE] >> pmu.jsonl
  E2E_CASE names an E2E capture and its `femtovg-e2e` thread's case (e.g.
  femtovg_e2e.scene0); without it the capture is a run_matrix one.
"""
import collections
import json
import sys
import xml.etree.ElementTree as ET

# Ratio sets that partition every segment's slots (sum to 1 per segment).
BUCKETS = {"bottlenecks": ["useful", "processing", "delivery", "discarded"]}


def main():
    path, runtime, mode = sys.argv[1:4]
    e2e_case = sys.argv[4] if len(sys.argv) > 4 else ""
    ids = {}
    # (thread, precise) -> interval (start, duration) -> metric -> (int, double)
    intervals = collections.defaultdict(lambda: collections.defaultdict(dict))
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
        precise = (cells[7].get("fmt") or cells[7].text or "") in ("Yes", "1", "true")
        metric = cells[6].get("fmt") or cells[6].text
        try:
            dbl = float(cells[5].text or 0)
        except ValueError:
            dbl = 0.0
        iv = intervals[(thread, precise)][(cells[0].text, cells[1].text)]
        i0, d0 = iv.get(metric, (0, 0.0))
        iv[metric] = (i0 + int(cells[4].text or 0), d0 + dbl)
    for thread in sorted({t for t, _ in intervals}):
        key = (thread, True) if (thread, True) in intervals else (thread, False)
        base = mode.split(":")[-1]
        part = BUCKETS.get(base, [])
        counts = collections.Counter()
        weights = collections.defaultdict(float)
        wsum = collections.defaultdict(float)  # bucket -> sum(mean fraction * interval cycles)
        wcyc = 0
        for iv in intervals[key].values():
            cyc = iv.get("cycle", (0, 0.0))[0]
            for metric, (i, d) in iv.items():
                counts[metric] += i
                if d and metric not in part:
                    weights[metric] += d
            n = sum(iv[b][1] for b in part if b in iv)
            if n > 0 and cyc:
                for b in part:
                    wsum[b] += iv.get(b, (0, 0.0))[1] / n * cyc
                wcyc += cyc
        fractions = {b: wsum[b] / wcyc for b in part} if wcyc else {}
        total = sum(fractions.values())
        shares = {b: round(100.0 * v / total, 2) for b, v in fractions.items()} if total else {}
        print(json.dumps({
            "runtime": runtime,
            "mode": mode,
            "capture": e2e_case or "matrix",
            "thread": thread,
            "case": thread[5:] if thread.startswith("case:") else
                    (e2e_case or thread) if thread == "femtovg-e2e" else None,
            "cycles": counts.get("cycle", 0),
            "counts": {k: v for k, v in counts.items() if k != "cycle" and v},
            "fractions": {k: round(v, 5) for k, v in fractions.items()},
            "shares": shares,
            "weights": {k: round(v, 3) for k, v in weights.items()},
        }))


if __name__ == "__main__":
    main()
