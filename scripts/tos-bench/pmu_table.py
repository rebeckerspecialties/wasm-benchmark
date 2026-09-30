#!/usr/bin/env python3
"""Tables from pmu.sh's pmu.jsonl: per variant and copy count, events per 1,000 dispatches and
pipeline-slot shares. A capture's dispatches are its cycles over the run's cycles per dispatch."""
import json, sys, collections
rows = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
by = collections.defaultdict(dict)
for r in rows:
    if 'error' in r: continue
    by[r['variant'], r['m']][r['mode']] = r
vs = list(dict.fromkeys(r['variant'] for r in rows)); ms = sorted({r['m'] for r in rows})
def per1k(r, key):
    d = r['cap_cycles'] / r['cycles_per_dispatch']
    return 1000 * r['counts'].get(key, 0) / d
cols = [('cycles/dispatch', lambda b: b['ds']['cycles_per_dispatch'] if 'ds' in b else None, '{:.2f}'),
        ('flushes /1k', lambda b: per1k(b['ds'], 'discarded_memory') if 'ds' in b else None, '{:.2f}'),
        ('mispredicts /1k', lambda b: per1k(b['ds'], 'discarded_branch') if 'ds' in b else None, '{:.2f}'),
        ('  conditional /1k', lambda b: per1k(b['ds'], 'discarded_cond_branch') if 'ds' in b else None, '{:.2f}'),
        ('useful %', lambda b: 100 * b['bottlenecks']['fractions']['useful'] if 'bottlenecks' in b else None, '{:.1f}'),
        ('processing %', lambda b: 100 * b['bottlenecks']['fractions']['processing'] if 'bottlenecks' in b else None, '{:.1f}'),
        ('discarded %', lambda b: 100 * b['bottlenecks']['fractions']['discarded'] if 'bottlenecks' in b else None, '{:.1f}'),
        ('delivery %', lambda b: 100 * b['bottlenecks']['fractions']['delivery'] if 'bottlenecks' in b else None, '{:.1f}'),
        # the processing mode's ratios are summed per segment: compare their shares
        ('execution latency % (processing mode)', lambda b: 100 * b['processing']['weights']['execution_latency'] / sum(b['processing']['weights'].values()) if 'processing' in b else None, '{:.1f}')]
for m in ms:
    first = next((by[v, m][k] for v in vs for k in by[v, m]), None)
    if not first: continue
    print(f"\n### {m} {'copy' if m == 1 else 'copies'} per kind: {first['handlers']} handlers, {first['pairs']} handler pairs\n")
    print('| per dispatch | ' + ' | '.join(f'`{v}`' for v in vs if by[v, m]) + ' |')
    print('|---|' + '---:|' * sum(1 for v in vs if by[v, m]))
    for name, f, fmt in cols:
        cells = []
        for v in vs:
            if not by[v, m]: continue
            try: x = f(by[v, m])
            except KeyError: x = None
            cells.append('' if x is None else fmt.format(x))
        print(f'| {name} | ' + ' | '.join(cells) + ' |')
