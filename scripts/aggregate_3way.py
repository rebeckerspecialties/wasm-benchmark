#!/usr/bin/env python3
"""Build a 3-way per-workload comparison table: wasmtime baseline,
wasmtime + phase3 fusion patches, and WAMR fast-interp.

Combines:
  - Wallclock medians from out/exp-3way/n10/iphone12-{baseline,phase3}-r*.log
  - PMU bucket totals from out/exp-3way/pmu-{baseline,phase3,wamr}/*.xml

Usage:
    aggregate_3way.py out/exp-3way

Emits Markdown tables to stdout.
"""

import os
import re
import sys
from collections import defaultdict
from glob import glob
from statistics import median
import xml.etree.ElementTree as ET


# ----------------------------------------------------------------------
# wallclock log parsing (mirrors parse_n10.py)
# ----------------------------------------------------------------------

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


# ----------------------------------------------------------------------
# PMU XML parsing (mirrors analyze_pmu.py)
# ----------------------------------------------------------------------


class Resolver:
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


def parse_pmu_rows(path):
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
        yield ctype, dur_ns, useful, processing, delivery, discarded
        elem.clear()


def aggregate_pmu(path):
    by_core = defaultdict(
        lambda: dict(useful=0, processing=0, delivery=0, discarded=0, dur_ns=0)
    )
    for ctype, dur, u, pr, d, dc in parse_pmu_rows(path):
        b = by_core[ctype]
        b["useful"] += u
        b["processing"] += pr
        b["delivery"] += d
        b["discarded"] += dc
        b["dur_ns"] += dur
    return by_core


# ----------------------------------------------------------------------
# XML discovery
# ----------------------------------------------------------------------

WORKLOADS = [
    ("call_indirect", "call_indirect"),
    ("xmrsplayer", "xmrsplayer"),
    ("vtable_mono", "vtable_mono"),
    ("vtable_bi", "vtable_bi"),
    ("vtable_poly4", "vtable_poly4"),
    ("vtable_poly6", "vtable_poly6"),
    ("graphql-validation_AS", "graphql-validation (AS)"),
    ("graphql-validation_Porffor", "graphql-validation (Porffor)"),
]


def pmu_path(root, cond, runtime, filename_tok):
    return os.path.join(
        root, f"pmu-{cond}", f"{cond}-{runtime}-{filename_tok}.xml"
    )


def load_pmu(path):
    """Return E-core totals dict or None if file is missing/empty."""
    if not os.path.exists(path) or os.path.getsize(path) < 1000:
        return None
    by_core = aggregate_pmu(path)
    e = by_core.get("E")
    if e is None or e["dur_ns"] == 0:
        return None
    return e


# ----------------------------------------------------------------------
# Output
# ----------------------------------------------------------------------


def fmt_int(x):
    return f"{x:,}"


def pct(a, b):
    """Return signed pct diff a→b."""
    if a == 0:
        return float("nan")
    return 100.0 * (b - a) / a


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    root = sys.argv[1]

    base_med = wallclock_medians(os.path.join(root, "n10"), "baseline")
    phase3_med = wallclock_medians(os.path.join(root, "n10"), "phase3")

    print("# 3-way comparison: wasmtime baseline vs phase3 fusion vs WAMR")
    print()
    print("Device: iPhone 12 (A14, Icestorm E-core). PMU = `CPU Counters` template,")
    print("`CPU Bottlenecks` mode (Useful / Processing / Delivery / Discarded cycles).")
    print("Wallclock = N=10 median of per-rep medians.")
    print()
    print("## Wallclock medians (lower = better)")
    print()
    print("| workload | baseline (Pulley) ms | phase3 (Pulley) ms | WAMR ms | Δ base→phase3 % | phase3 vs WAMR |")
    print("|---|---:|---:|---:|---:|---:|")

    for tok, wname in WORKLOADS:
        # Pulley rows live under runtime "Pulley"; WAMR under "WAMR"
        b_pulley = base_med.get(("Pulley", wname))
        p_pulley = phase3_med.get(("Pulley", wname))
        wamr = phase3_med.get(("WAMR", wname))
        if wamr is None:
            # WAMR may have its own log row; if it errored (Porffor) it's None
            wamr_s = "N/A"
            cmp_s = "—"
        else:
            wamr_s = f"{wamr:.2f}"
            if p_pulley:
                cmp_s = f"WAMR {p_pulley / wamr:.2f}× faster"
            else:
                cmp_s = "—"
        if b_pulley and p_pulley:
            dpct = pct(b_pulley, p_pulley)
            dpct_s = f"{dpct:+.2f}%"
        else:
            dpct_s = "—"
        b_s = f"{b_pulley:.2f}" if b_pulley else "—"
        p_s = f"{p_pulley:.2f}" if p_pulley else "—"
        print(f"| {wname} | {b_s} | {p_s} | {wamr_s} | {dpct_s} | {cmp_s} |")

    print()
    print("## PMU E-core cycle totals (per ~12s benchmark window)")
    print()
    print("Per workload, four rows (Useful / Processing / Delivery / Discarded), three columns")
    print("(baseline, phase3, WAMR). Below the absolute table: phase3 vs baseline diff (our patches'")
    print("contribution) and phase3 vs WAMR diff (remaining gap to WAMR).")
    print()

    for tok, wname in WORKLOADS:
        baseline = load_pmu(pmu_path(root, "baseline", "pulley", tok))
        phase3 = load_pmu(pmu_path(root, "phase3", "pulley", tok))
        wamr = load_pmu(pmu_path(root, "wamr", "pulley", tok))  # filename uses "phase3" prefix
        # actual WAMR files are phase3-wamr-*; pmu_path won't find them because cond != phase3-wamr
        if wamr is None:
            wpath = os.path.join(root, "pmu-wamr", f"phase3-wamr-{tok}.xml")
            wamr = load_pmu(wpath)

        print(f"### {wname}")
        print()
        if not (baseline or phase3 or wamr):
            print("_no PMU data_\n")
            continue
        print("| bucket | baseline | phase3 | WAMR |")
        print("|---|---:|---:|---:|")
        for key in ("useful", "processing", "delivery", "discarded"):
            b = baseline[key] if baseline else None
            p = phase3[key] if phase3 else None
            w = wamr[key] if wamr else None
            b_s = fmt_int(b) if b is not None else "—"
            p_s = fmt_int(p) if p is not None else "—"
            w_s = fmt_int(w) if w is not None else "—"
            print(f"| {key} | {b_s} | {p_s} | {w_s} |")
        # totals
        def tot(d):
            return sum(d[k] for k in ("useful", "processing", "delivery", "discarded")) if d else None
        bt = tot(baseline); pt = tot(phase3); wt = tot(wamr)
        bt_s = fmt_int(bt) if bt else "—"
        pt_s = fmt_int(pt) if pt else "—"
        wt_s = fmt_int(wt) if wt else "—"
        print(f"| **total** | **{bt_s}** | **{pt_s}** | **{wt_s}** |")
        print()
        # Phase3 vs baseline (our patches)
        if baseline and phase3:
            print("phase3 vs baseline (our patches' contribution):")
            print()
            print("| bucket | Δ cycles | Δ % | share base | share phase3 |")
            print("|---|---:|---:|---:|---:|")
            for key in ("useful", "processing", "delivery", "discarded"):
                b = baseline[key]; p = phase3[key]
                d = p - b
                dp = pct(b, p)
                sb = 100.0 * b / bt if bt else 0
                sp = 100.0 * p / pt if pt else 0
                print(f"| {key} | {d:+,} | {dp:+.2f}% | {sb:.1f}% | {sp:.1f}% |")
            tot_dp = pct(bt, pt)
            print(f"| **total** | **{pt-bt:+,}** | **{tot_dp:+.2f}%** | | |")
            print()
        # Phase3 vs WAMR — suppress when WAMR clearly errored early
        # (its capture has <10% of phase3's cycle volume).
        wamr_valid = phase3 and wamr and tot(wamr) and tot(phase3) and tot(wamr) > 0.1 * tot(phase3)
        if phase3 and wamr and not wamr_valid:
            print("_phase3 vs WAMR: WAMR exited early on this workload — diff is not meaningful._")
            print()
        if wamr_valid:
            print("phase3 vs WAMR (remaining gap to WAMR):")
            print()
            print("| bucket | Δ cycles | Δ % | share phase3 | share WAMR |")
            print("|---|---:|---:|---:|---:|")
            for key in ("useful", "processing", "delivery", "discarded"):
                p = phase3[key]; w = wamr[key]
                d = w - p
                dp = pct(p, w)
                sp = 100.0 * p / pt if pt else 0
                sw = 100.0 * w / wt if wt else 0
                print(f"| {key} | {d:+,} | {dp:+.2f}% | {sp:.1f}% | {sw:.1f}% |")
            tot_dp = pct(pt, wt)
            print(f"| **total** | **{wt-pt:+,}** | **{tot_dp:+.2f}%** | | |")
            print()

    print("---")
    print()
    print("**Notes**")
    print()
    print("- `graphql-validation (Porffor)` on WAMR: structurally N/A. WAMR's interp can support")
    print("  SIMD **or** wasm-exceptions, never both at once. Porffor's WAT uses both.")
    print("- All Pulley runs include opcode-fusion peephole pass during module load — load time is")
    print("  separated from steady-state median by the harness and is excluded here.")
    print("- Cycle counts are aggregate Icestorm E-core cycles across the ~12 s xctrace window")
    print("  (single core, single workload per trace).")


if __name__ == "__main__":
    main()
