#!/usr/bin/env python3
"""tos-bench PMU on a device: events per 1,000 dispatches and pipeline-slot shares from
scripts/run-device-pmu.sh's pmu.jsonl (rows `tos-bench <variant> (m=<copies>)`), with cycles per
dispatch from the timing run's CSV (see device_table.py).

    device_pmu_table.py <pmu.jsonl> <timing.csv>...

A capture's counts belong to its busiest thread (the benchmark worker). Events per dispatch are
the count over that thread's cycles, times the timing run's median cycles per dispatch.
"""
import collections, csv, json, re, statistics, sys

DISPATCHES_PER_CALL = 2000 * 277
pmu_path, timing = sys.argv[1], sys.argv[2:]
cpd = collections.defaultdict(list)
for path in timing:
    for r in csv.DictReader(open(path)):
        m = re.match(r'tos-bench (\w+) \(m=(\d+)\)', r['case'])
        if m:
            cpd[m.group(1), int(m.group(2))].append(float(r['cycles_per_call']) / DISPATCHES_PER_CALL)
best = {}
for l in open(pmu_path):
    d = json.loads(l)
    m = re.search(r'tos-bench (\w+) \(m=(\d+)\)', d.get('workload', ''))
    if not m: continue
    k = (m.group(1), int(m.group(2)), d['mode'])
    if k not in best or d['cycles'] > best[k]['cycles']:
        best[k] = d
vs = list(dict.fromkeys(v for v, _, _ in best)); ms = sorted({m for _, m, _ in best})
rows = [('cycles / dispatch (timing run)', None, None),
        ('memory-order flushes / 1k', 'bottleneck:discarded_sampling', 'discarded_memory'),
        ('branch mispredicts / 1k', 'bottleneck:discarded_sampling', 'discarded_branch'),
        ('  conditional / 1k', 'bottleneck:discarded_sampling', 'discarded_cond_branch'),
        ('useful %', 'bottleneck:bottlenecks', 'useful'),
        ('back-end (processing) %', 'bottleneck:bottlenecks', 'processing'),
        ('discarded %', 'bottleneck:bottlenecks', 'discarded'),
        ('delivery %', 'bottleneck:bottlenecks', 'delivery')]
for m in ms:
    cols = [v for v in vs if any((v, m, mode) in best for mode in ('bottleneck:discarded_sampling', 'bottleneck:bottlenecks'))]
    if not cols: continue
    print(f'\n### m={m}\n')
    print('| per dispatch | ' + ' | '.join(f'`{v}`' for v in cols) + ' |')
    print('|---|' + '---:|' * len(cols))
    for name, mode, key in rows:
        cells = []
        for v in cols:
            c = statistics.median(cpd[v, m]) if cpd[v, m] else None
            if mode is None:
                cells.append(f'{c:.2f}' if c else '')
            elif (v, m, mode) not in best:
                cells.append('')
            elif mode.endswith('bottlenecks'):
                cells.append(f"{100 * best[v, m, mode]['fractions'].get(key, 0):.1f}")
            else:
                d = best[v, m, mode]
                cells.append(f"{1000 * d['counts'].get(key, 0) / d['cycles'] * c:.2f}" if c else '')
        print(f'| {name} | ' + ' | '.join(cells) + ' |')
