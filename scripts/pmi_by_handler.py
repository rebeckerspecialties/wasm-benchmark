#!/usr/bin/env python3
"""Tally the samples of a sampling CPU Counters mode by event, tinywasm handler and
innermost source line.

Export the samples of a capture in a sampling mode (e.g. bottleneck:discarded_sampling):
  xcrun xctrace export --input <trace> \
    --xpath '//trace-toc/run/data/table[@schema="SamplingModeSamples"]' --output samples.xml
usage: pmi_by_handler.py samples.xml [event] [top]
  event: a PMI event name such as discarded_memory or discarded_branch (default: all)"""
import collections, re, sys, xml.etree.ElementTree as ET
path = sys.argv[1]; want = sys.argv[2] if len(sys.argv) > 2 else None; top = int(sys.argv[3]) if len(sys.argv) > 3 else 15
ids = {}
def resolve(e):
    r = e.get('ref')
    return ids[r] if r else e
by_ev = collections.Counter(); by_handler = collections.Counter(); by_line = collections.Counter(); cores = collections.Counter()
for _, el in ET.iterparse(path, events=('end',)):
    if el.get('id'):
        ids[el.get('id')] = el
    if el.tag != 'row':
        continue
    cells = [resolve(c) for c in el]
    ev = cells[1].text or cells[1].get('fmt')
    core = cells[4].get('fmt', '')
    bt = cells[5]
    frames = [f for f in bt.iter('frame')] if bt is not None else []
    # frames may be refs
    frames = [ids.get(f.get('ref'), f) if f.get('ref') else f for f in frames]
    by_ev[ev] += 1
    cores['E' if 'E Core' in core else 'P'] += 1
    if want and ev != want:
        continue
    handler = next((re.sub(r'.*Unbudgeted\d+', '', f.get('name', '')) for f in frames if 'Unbudgeted' in f.get('name', '')), '?')
    inner = frames[0] if frames else None
    loc = '?'
    if inner is not None:
        src = inner.find('source')
        src = ids.get(src.get('ref'), src) if src is not None and src.get('ref') else src
        if src is not None:
            p = src.find('path'); p = ids.get(p.get('ref'), p) if p is not None and p.get('ref') else p
            loc = f"{(p.text or '').split('/src/')[-1]}:{src.get('line')} {inner.get('name','')[:40]}"
    by_handler[handler] += 1
    by_line[loc] += 1
print('events:', dict(by_ev), 'cores:', dict(cores))
n = sum(by_handler.values())
print(f'\n{want or "all"}: {n} samples, by handler')
for h, c in by_handler.most_common(top):
    print(f'  {100*c/n:5.1f}%  {h}')
print(f'\nby innermost source line')
for h, c in by_line.most_common(top):
    print(f'  {100*c/n:5.1f}%  {h}')
