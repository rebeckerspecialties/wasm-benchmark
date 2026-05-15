#!/usr/bin/env python3
"""Print phase-4 PMU bucket shares for M4 E-core and iPhone 12 E-core
side-by-side. Phase-4 bucket shares are stable single-shot metrics
(unlike absolute cycle counts which are sensitive to macOS scheduler
contention on the M4 host). The shares answer:

    "Does the M4 Sawtooth E-core have the same bottleneck pattern as
    A14 Icestorm under phase-4 Pulley, so phase-5 design choices
    inform both?"
"""

import sys
from collections import defaultdict
sys.path.insert(0, "scripts")
from analyze_pmu import aggregate

WORKLOADS = [
    ("call_indirect", "call_indirect"),
    ("xmrsplayer", "xmrsplayer"),
    ("vtable_mono", "vtable_mono"),
    ("vtable_bi", "vtable_bi"),
    ("vtable_poly4", "vtable_poly4"),
    ("vtable_poly6", "vtable_poly6"),
    # iPhone uses _AS/_Porffor suffix; M4 uses -as/-porf suffix
    ("graphql-validation_AS", "graphql-validation-as"),
    ("graphql-validation_Porffor", "graphql-validation-porf"),
]


def shares(path):
    agg = aggregate(path)
    e = agg.get("E")
    if not e:
        return None
    tot = sum(e[k] for k in ("useful", "processing", "delivery", "discarded"))
    if tot == 0:
        return None
    return {k: 100.0 * e[k] / tot for k in ("useful", "processing", "delivery", "discarded")}


print("# Phase-4 PMU bucket shares — A14 Icestorm vs M4 Sawtooth E-core")
print()
print(
    "| workload | platform | Useful | Processing | Delivery | Discarded |"
)
print("|---|---|---:|---:|---:|---:|")
for ip_tok, m4_tok in WORKLOADS:
    ip = shares(f"out/exp-3way/pmu-phase4/phase4-pulley-{ip_tok}.xml")
    m4 = shares(f"out/exp-3way-m4/pmu-phase4/phase4-m4-{m4_tok}.xml")
    name = ip_tok.replace("_AS", " (AS)").replace("_Porffor", " (Porffor)")
    if ip:
        print(
            f"| {name} | iPhone 12 (A14) | "
            f"{ip['useful']:.1f}% | {ip['processing']:.1f}% | "
            f"{ip['delivery']:.1f}% | {ip['discarded']:.1f}% |"
        )
    if m4:
        print(
            f"| {name} | M4 (Sawtooth) | "
            f"{m4['useful']:.1f}% | {m4['processing']:.1f}% | "
            f"{m4['delivery']:.1f}% | {m4['discarded']:.1f}% |"
        )
