#!/usr/bin/env python3
"""Cycles and instructions per dispatch of the tos-bench cases on a device, from the CSV that
`scripts/tinywasm_ab_summary.py <out> tosb --csv <file>` writes for a run of the model build.

    device_table.py <csv>... [--variants a,b,...]

Each timed call runs the program TOS_MODEL_ITERS times (crates/benchmark-core/src/cases.rs):
2,000 x 277 = 554,000 dispatches. Medians over launches; e_share is the lowest seen.
"""
import collections, csv, re, statistics, sys

DISPATCHES_PER_CALL = 2000 * 277
args = [a for a in sys.argv[1:] if not a.startswith('--')]
only = None
if '--variants' in sys.argv:
    only = sys.argv[sys.argv.index('--variants') + 1].split(',')
    args = [a for a in args if a != sys.argv[sys.argv.index('--variants') + 1]]
cyc = collections.defaultdict(list); ins = collections.defaultdict(list); esh = collections.defaultdict(list)
for path in args:
    for r in csv.DictReader(open(path)):
        m = re.match(r'tos-bench (\w+) \(m=(\d+)\)', r['case'])
        if not m: continue
        k = (m.group(1), int(m.group(2)))
        cyc[k].append(float(r['cycles_per_call']) / DISPATCHES_PER_CALL)
        ins[k].append(float(r['instructions_per_call']) / DISPATCHES_PER_CALL)
        esh[k].append(float(r['e_share']))
vs = list(dict.fromkeys(v for v, _ in cyc)); ms = sorted({m for _, m in cyc})
if only: vs = [v for v in only if v in vs]
def table(title, d, fmt):
    print(f'| {title} | ' + ' | '.join(f'm={m}' for m in ms) + ' |')
    print('|---|' + '---:|' * len(ms))
    for v in vs:
        print(f'| `{v}` | ' + ' | '.join(fmt.format(statistics.median(d[v, m])) if d[v, m] else '' for m in ms) + ' |')
    print()
table('cycles / dispatch', cyc, '{:.2f}')
table('instructions / dispatch', ins, '{:.2f}')
n = [len(x) for x in cyc.values()]
spread = max((max(c) - min(c)) / statistics.median(c) for c in cyc.values())
print(f'launches per cell: {min(n)}-{max(n)}; largest spread (max-min)/median: {100 * spread:.1f}%; '
      f'lowest e_share: {min(min(x) for x in esh.values()):.3f}')
