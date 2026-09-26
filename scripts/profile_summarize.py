#!/usr/bin/env python3
"""Reduce an xctrace Time Profiler export (schema "time-profile") to the
frame histograms of the busiest thread.

Usage: profile_summarize.py EXPORT.xml [TOP_N]
Prints the busiest thread, its sample count and E-/P-core split, then the
TOP_N (default 40) frames by self time (leaf frame) and by inclusive time
(anywhere on the stack), each as a share of that thread's samples.
"""
import collections
import sys
import xml.etree.ElementTree as ET


def main():
    path = sys.argv[1]
    top_n = int(sys.argv[2]) if len(sys.argv) > 2 else 40
    ids = {}
    leaf = collections.defaultdict(collections.Counter)  # thread -> leaf frame -> samples
    incl = collections.defaultdict(collections.Counter)  # thread -> frame on stack -> samples
    cores = collections.defaultdict(collections.Counter)  # thread -> "E Core"/"P Core" -> samples
    total = collections.Counter()

    def resolve(e):
        r = e.get("ref")
        return ids.get(r, e) if r else e

    for _, elem in ET.iterparse(path, events=("end",)):
        if elem.get("id"):
            ids[elem.get("id")] = elem
        if elem.tag != "row":
            continue
        thread, core, frames = None, "", []
        for c in elem:
            c = resolve(c)
            if c.tag == "thread":
                thread = (c.get("fmt") or "?").split(" (WasmBench")[0]
            elif c.tag == "core":
                core = "E" if "E Core" in (c.get("fmt") or "") else "P"
            elif c.tag in ("tagged-backtrace", "backtrace"):
                for f in c.iter("frame"):
                    f = resolve(f)
                    frames.append(f.get("name") or f.get("addr") or "?")
        elem.clear()  # children stay reachable through `ids`
        if thread is None or not frames:
            continue
        total[thread] += 1
        cores[thread][core] += 1
        leaf[thread][frames[0]] += 1
        for name in set(frames):
            incl[thread][name] += 1
    if not total:
        print("no samples")
        return
    thread, n = total.most_common(1)[0]
    e = cores[thread]["E"]
    print(f"busiest thread: {thread}  samples: {n} ({100.0 * e / n:.1f}% on E cores)  "
          f"all threads: {sum(total.values())}")
    print(f"\n# self time (leaf frame), top {top_n}")
    for name, c in leaf[thread].most_common(top_n):
        print(f"{100.0 * c / n:6.2f}%  {c:6d}  {name}")
    print(f"\n# inclusive (on stack), top {top_n}")
    for name, c in incl[thread].most_common(top_n):
        print(f"{100.0 * c / n:6.2f}%  {c:6d}  {name}")


if __name__ == "__main__":
    main()
