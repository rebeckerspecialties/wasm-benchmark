#!/usr/bin/env python3
"""Per-call PMU event counts for A/B builds, from device PMU captures and a timing run.

pmu_per_call.py <pmu-root> <timing.csv> <steps>
  pmu-root/<variant>/pmu.jsonl  scripts/run-device-pmu.sh output, one directory per build
                                (captures of one row each; the `workload` field is the
                                label prefix the row was selected with)
  timing.csv                    scripts/tinywasm_ab_summary.py --csv of the same builds on the
                                same phone
  steps                         comma-separated new:old variant pairs

Each capture's counts belong to the busiest thread (the benchmark worker). A count over the
capture's cycles is a rate per cycle; times the row's median cycles per call (timing run) it is a
count per call, and over the median IPC a rate per 1k instructions.
"""
import collections, csv, json, math, statistics, sys

root, timing_csv, steps = sys.argv[1], sys.argv[2], sys.argv[3]
steps = [tuple(s.split(':')) for s in steps.split(',')]
variants = sorted({v for s in steps for v in s})

# timing: (variant, case) -> (cycles/call, instructions/call)
tim = collections.defaultdict(lambda: collections.defaultdict(list))
for r in csv.DictReader(open(timing_csv)):
    tim[r['variant']][r['case']].append((float(r['cycles_per_call']), float(r['instructions_per_call'])))

def timing(variant, workload):
    cases = [c for c in tim[variant] if c.lower().startswith(workload.lower())]
    assert len(cases) == 1, (variant, workload, cases)
    xs = tim[variant][cases[0]]
    return statistics.median(x[0] for x in xs), statistics.median(x[1] for x in xs), len(xs)

# pmu: (variant, workload, mode) -> busiest thread's line
pmu = {}
for v in variants:
    try:
        lines = [json.loads(l) for l in open(f'{root}/{v}/pmu.jsonl')]
    except FileNotFoundError:
        continue
    for l in lines:
        k = (v, l['workload'], l['mode'])
        if k not in pmu or l['cycles'] > pmu[k]['cycles']:
            pmu[k] = l
workloads = []
for (_, w, _) in pmu:
    if w not in workloads:
        workloads.append(w)

EVENTS = [  # (label, mode, count names summed)
    ('mispredicts (all)', 'bottleneck:discarded_sampling', ['discarded_branch']),
    ('  conditional', 'bottleneck:discarded_sampling', ['discarded_cond_branch']),
    ('  indirect (br)', 'bottleneck:discarded_indirect_sampling', ['discarded_indirect_branch']),
    ('  call', 'bottleneck:discarded_indirect_sampling', ['discarded_call']),
    ('  return', 'bottleneck:discarded_indirect_sampling', ['discarded_return']),
    ('memory-order flushes', 'bottleneck:discarded_sampling', ['discarded_memory']),
    ('L1I demand misses', 'metrics:instruction_address_translation_metrics', ['l1i_demand_miss_spec']),
    ('L1 iTLB misses', 'metrics:instruction_address_translation_metrics', ['l1i_tlb_miss_spec']),
    ('fetch restarts', 'metrics:instruction_address_translation_metrics', ['fetch_restart_spec']),
    ('L2 iTLB misses', 'metrics:instruction_address_translation_metrics', ['l2i_tlb_miss_spec']),
    ('L1D load misses', 'metrics:l1d_metrics', ['l1d_miss_ld_spec']),
    ('L1D store misses', 'metrics:l1d_metrics', ['l1d_miss_st_spec']),
    ('load uops', 'metrics:l1d_metrics', ['ld_uop_spec']),
    ('store uops', 'metrics:l1d_metrics', ['st_uop_spec']),
    ('branches', 'characteristics:branch_simd_vector_instructions', ['branch']),
    ('taken branches', 'characteristics:branch_simd_vector_instructions', ['taken_branch']),
]
BUCKETS = ['useful', 'processing', 'delivery', 'discarded']


def per_call(v, w):
    """metric -> value per call (events), plus cycles, instructions, IPC and slot shares."""
    cyc, ins, n = timing(v, w)
    out = {'cycles': cyc, 'instructions': ins, 'IPC': ins / cyc, '_n': n}
    for label, mode, names in EVENTS:
        l = pmu.get((v, w, mode))
        if l and l['cycles']:
            out[label] = sum(l['counts'].get(x, 0) for x in names) / l['cycles'] * cyc
    b = pmu.get((v, w, 'bottleneck:bottlenecks'))
    if b:
        for k in BUCKETS:
            out[f'slots {k} %'] = 100 * b['fractions'].get(k, 0)
    d = pmu.get((v, w, 'bottleneck:delivery'))
    if d:
        wt = d['weights']
        lat, bw = wt.get('delivery_latency', 0), wt.get('delivery_bandwidth', 0)
        if lat + bw:
            out['delivery: latency share %'] = 100 * lat / (lat + bw)
    return out


def pct(new, old):
    if old == 0:
        return '' if new == 0 else 'new'
    return f'{100 * (new / old - 1):+.1f}%'.replace('-', '−')


rowsets = {w: {v: per_call(v, w) for v in variants if (v, w, 'bottleneck:bottlenecks') in pmu} for w in workloads}

# 1. absolute per-call table per workload
for w in workloads:
    vs = [v for v in variants if v in rowsets[w]]
    print(f'\n### {w}\n')
    print('| per call | ' + ' | '.join(vs) + ' |')
    print('|---|' + '---:|' * len(vs))
    keys = ['cycles', 'instructions', 'IPC'] + [f'slots {k} %' for k in BUCKETS] + ['delivery: latency share %'] + [e[0] for e in EVENTS]
    for k in keys:
        cells = []
        for v in vs:
            x = rowsets[w][v].get(k)
            if x is None:
                cells.append('')
            elif k == 'IPC':
                cells.append(f'{x:.2f}')
            elif k.startswith('slots') or k.endswith('%'):
                cells.append(f'{x:.1f}')
            elif x >= 1e6:
                cells.append(f'{x / 1e6:.2f} M')
            elif x >= 1e3:
                cells.append(f'{x / 1e3:.1f} k')
            else:
                cells.append(f'{x:.0f}')
        print(f'| {k.strip()} | ' + ' | '.join(cells) + ' |')

# 2. per step: change per call on each workload
for new, old in steps:
    print(f'\n### {new} against {old}: change per call\n')
    ws = [w for w in workloads if new in rowsets[w] and old in rowsets[w]]
    print('| metric | ' + ' | '.join(ws) + ' |')
    print('|---|' + '---:|' * len(ws))
    keys = ['cycles', 'instructions'] + [e[0] for e in EVENTS]
    for k in keys:
        print(f'| {k.strip()} | ' + ' | '.join(pct(rowsets[w][new].get(k, 0), rowsets[w][old].get(k, 0)) if k in rowsets[w][new] and k in rowsets[w][old] else '' for w in ws) + ' |')
    for k in BUCKETS:
        key = f'slots {k} %'
        print(f'| slots {k} (points) | ' + ' | '.join(f"{rowsets[w][new][key] - rowsets[w][old][key]:+.1f}".replace('-', '−') if key in rowsets[w][new] and key in rowsets[w][old] else '' for w in ws) + ' |')
