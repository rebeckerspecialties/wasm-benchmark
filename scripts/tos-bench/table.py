#!/usr/bin/env python3
"""Median cycles and instructions per dispatch per (variant, copies) from sweep.sh output."""
import re, sys, statistics, collections
cyc = collections.defaultdict(list); ins = collections.defaultdict(list); meta = {}
for path in sys.argv[1:]:
    for l in open(path):
        m = re.search(r'(\w+) m=(\d+): [0-9.]+ ns/dispatch, ([0-9.]+) instr/dispatch, ([0-9.]+) cycles/dispatch, (\d+) handlers, (\d+) pairs', l)
        if not m: continue
        v, k = m.group(1), int(m.group(2))
        ins[v, k].append(float(m.group(3))); cyc[v, k].append(float(m.group(4)))
        meta[k] = (int(m.group(5)), int(m.group(6)))
vs = list(dict.fromkeys(v for v, _ in cyc)); ks = sorted(meta)
print('| cycles / dispatch | ' + ' | '.join(f'{k} ({meta[k][0]} h, {meta[k][1]} p)' for k in ks) + ' |')
print('|---|' + '---:|' * len(ks))
for v in vs:
    print(f'| `{v}` | ' + ' | '.join(f'{statistics.median(cyc[v, k]):.2f}' if cyc[v, k] else '' for k in ks) + ' |')
print()
print('| instructions / dispatch | ' + ' | '.join(str(k) for k in ks) + ' |')
print('|---|' + '---:|' * len(ks))
for v in vs:
    print(f'| `{v}` | ' + ' | '.join(f'{statistics.median(ins[v, k]):.2f}' if ins[v, k] else '' for k in ks) + ' |')
print()
spread = max((max(c) - min(c)) / statistics.median(c) for c in cyc.values())
print(f'runs per cell: {min(len(c) for c in cyc.values())}-{max(len(c) for c in cyc.values())}; largest cycles spread (max-min)/median: {100*spread:.1f}%')
