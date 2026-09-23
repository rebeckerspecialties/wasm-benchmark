#!/usr/bin/env python3
"""Summarize the measurement passes into the report's data files.

Usage: summarize-pass.py <pass-root> <data-dir>

<pass-root> holds whatever of these exist:
  m4/m4-matrix.jsonl      scripts/run-m4-pass.sh (run_matrix lines)
  m4/m4-e2e.jsonl         femtovg E2E lines
  m4/cm-async/cm-async.jsonl, m4/cm-async/rusage.jsonl
  iphonexs/*.log          scripts/run-device-pass.sh (workload list)
  iphonexs-e2e/*.log      scripts/run-device-pass.sh in E2E mode
  pmu/pmu.jsonl, pmu/pmu-matrix.jsonl, pmu/pmu-e2e.jsonl
                          scripts/run-m4-pmu-pass.sh

<data-dir> receives per-rep CSVs (one row per rep × runtime × case), a
summary.json with the aggregated values, and tables.md with the markdown
tables the report embeds. Cells are "median [min–max]" over reps of each
rep's own median; errors show the error text instead.
"""
import csv
import glob
import json
import math
import os
import re
import statistics
import sys
from collections import defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RUNTIMES = ["pulley", "wamr", "wasm3", "wasmedge", "zwasm", "wasmz", "tinywasm"]
PREFIX = {"[Pulley]": "pulley", "[ WAMR ]": "wamr", "[wasm3 ]": "wasm3", "[WE    ]": "wasmedge",
          "[zwasm ]": "zwasm", "[wasmz ]": "wasmz", "[tinywm]": "tinywasm"}


def case_table():
    """(ids in cases.rs order, label -> id) from the CASES table in
    crates/benchmark-core/src/cases.rs (entries are `c("id", "label", ...)`
    or `Case { id: "id", label: "label", ... }`)."""
    src = open(os.path.join(ROOT, "crates/benchmark-core/src/cases.rs")).read()
    body = src[src.index("pub const CASES"):]
    body = body[:body.index("];")]
    pairs = re.findall(r'(?:c\(\s*|id:\s*)"([^"]+)",\s*(?:label:\s*)?"([^"]+)"', body)
    return [cid for cid, _ in pairs], {lab: cid for cid, lab in pairs}


CASES, LABEL_TO_CASE = case_table()


def med_range(xs):
    xs = [x for x in xs if x is not None]
    if not xs:
        return None
    return {"median": statistics.median(xs), "min": min(xs), "max": max(xs), "n": len(xs)}


def fmt_cell(st, scale=1.0, digits=3):
    if st is None:
        return ""
    m, lo, hi = st["median"] * scale, st["min"] * scale, st["max"] * scale
    if st["n"] == 1:
        return f"{m:.{digits}f}"
    return f"{m:.{digits}f} [{lo:.{digits}f}–{hi:.{digits}f}]"


def short_err(e):
    """Error text for a table legend, with the per-case parts (export name,
    call arguments, byte offsets) taken out so one cause gets one code."""
    e = e.replace("|", "/").replace("\n", " ")
    e = re.sub(r"^N/A — (not run: )?", "N/A: ", e)
    e = re.sub(r"m3_FindFunction\(\w+\)", "m3_FindFunction", e)
    e = re.sub(r"`[\w.]+\([^`]*\)` trapped", "call trapped", e)
    e = re.sub(r"function\[\d+\]", "function[N]", e)
    e = re.sub(r"\s+(Bytecode offset|At AST node):.*", "", e)
    e = re.sub(r"\s{2,}", " ", e)
    return e[:240]


# ---------------------------------------------------------------------------
# Workload matrix: M4 (JSONL) and device (logs)
# ---------------------------------------------------------------------------

LOG_RE = re.compile(
    r"^\[(\[[^\]]+\]) (.*?)\] (?:ERROR: (?P<err>.*)|result=(?P<result>-?\d+)\s+iter=(?P<iter>\d+)\s+"
    r"load=(?P<load>[\d.]+)ms\s+min=(?P<min>[\d.]+) median=(?P<median>[\d.]+) p99=(?P<p99>[\d.]+) ms\s+"
    r"cpu\(u/s\)=(?P<cu>[\d.]+)/(?P<cs>[\d.]+) ms\s+rss=(?P<rss>\d+)KB\s+faults=(?P<faults>\d+)\s+"
    r"e_share=(?P<es>-?[\d.]+)\s+ipc=(?P<ipc>-?[\d.]+)\s+insns=(?P<insns>\d+)\s+cycles=(?P<cycles>\d+))")


def load_device_logs(dirs):
    """Result rows from scripts/run-device-pass.sh logs. A launch the OS
    killed (jetsam SIGKILL, crash) leaves its remaining rows absent; every
    case a (rep, runtime) never reported is recorded as an error naming the
    killed launch whose WORKLOADS filter covered it."""
    rows = []
    seen = defaultdict(set)     # (rep, runtime) -> cases reported
    killed = defaultdict(list)  # (rep, runtime) -> [(log, signal, needles)]
    logs = sorted(p for d in dirs for p in glob.glob(os.path.join(d, "*.log")))
    for log in logs:
        m = re.search(r"-(\w+)-r(\d+)(?:-h\d+)?\.log$", os.path.basename(log))
        if not m:
            continue
        rt_file, rep = m.group(1), int(m.group(2))
        text = open(log, errors="replace").read()
        sig = re.search(r"App terminated due to signal (\d+)", text)
        if sig and sig.group(1) != "15":  # 15 = the launcher's own terminate
            f = re.search(r"WORKLOADS filter: (.*?); RUNTIMES filter:", text)
            needles = [] if not f or f.group(1) == "(any)" else [n.strip() for n in f.group(1).split(",")]
            killed[(rep, rt_file)].append((os.path.basename(log), sig.group(1), needles))
        for line in text.splitlines():
            lm = LOG_RE.match(line.strip())
            if not lm:
                continue
            rt = PREFIX.get(lm.group(1))
            case = LABEL_TO_CASE.get(lm.group(2).strip())
            if not rt or not case:
                continue
            seen[(rep, rt)].add(case)
            if lm.group("err") is not None:
                rows.append(dict(rep=rep, runtime=rt, case=case, ok=False, error=lm.group("err")))
                continue
            it = int(lm.group("iter"))
            cu = float(lm.group("cu")) * 1e6
            rows.append(dict(
                rep=rep, runtime=rt, case=case, ok=True, result=int(lm.group("result")), iterations=it,
                load_ns=float(lm.group("load")) * 1e6, min_ns=float(lm.group("min")) * 1e6,
                median_ns=float(lm.group("median")) * 1e6, p99_ns=float(lm.group("p99")) * 1e6,
                cpu_user_ns=cu, cpu_system_ns=float(lm.group("cs")) * 1e6,
                e_share=float(lm.group("es")), ipc=float(lm.group("ipc")),
                instructions=int(lm.group("insns")), cycles=int(lm.group("cycles")),
                rss_peak_bytes=int(lm.group("rss")) * 1024))
    reps = sorted({rep for rep, _ in seen} | {rep for rep, _ in killed})
    for rep in reps:
        for rt in RUNTIMES:
            if (rep, rt) not in seen and (rep, rt) not in killed:
                continue
            for label, case in LABEL_TO_CASE.items():
                if case in seen[(rep, rt)]:
                    continue
                # Prefer the killed launch whose filter names this row; else
                # the unfiltered (main) launch, if it was the one killed.
                who = [k for k in killed[(rep, rt)] if any(n in label.lower() for n in k[2])] or \
                      [k for k in killed[(rep, rt)] if not k[2]]
                err = (f"app killed by signal {who[0][1]} (jetsam) during this row's launch"
                       if who else "not reported by any launch")
                rows.append(dict(rep=rep, runtime=rt, case=case, ok=False, error=err))
    return rows


def load_matrix_jsonl(p):
    rows = []
    for line in open(p):
        r = json.loads(line)
        if r.get("ok"):
            r["cpu_user_ns"] = float(r["cpu_user_ns"])
            r["ipc"] = r["instructions"] / r["cycles"] if r.get("cycles") else None
        rows.append(r)
    return rows


def aggregate_matrix(rows):
    by = defaultdict(list)
    for r in rows:
        by[(r["case"], r["runtime"])].append(r)
    out = {}
    for (case, rt), rs in by.items():
        oks = [r for r in rs if r.get("ok")]
        if oks:
            out[(case, rt)] = {
                "ok": len(oks), "n": len(rs),
                "median_ms": med_range([r["median_ns"] / 1e6 for r in oks]),
                "cpu_ms_per_iter": med_range([r["cpu_user_ns"] / 1e6 / r["iterations"] for r in oks]),
                "insns_per_iter": med_range([r["instructions"] / r["iterations"] for r in oks
                                             if r.get("instructions")]),
                "e_share": med_range([r.get("e_share") for r in oks]),
                "ipc": med_range([r.get("ipc") for r in oks]),
                "result": sorted({r.get("result") for r in oks}),
                "errors": sorted({short_err(r.get("error", "")) for r in rs if not r.get("ok")}),
            }
        else:
            out[(case, rt)] = {"ok": 0, "n": len(rs),
                               "errors": sorted({short_err(r.get("error", "")) for r in rs})}
    return out


def sig(v, scale=1.0):
    """4 significant digits, never in exponent form for the magnitudes here."""
    v *= scale
    return f"{v:.4g}" if abs(v) >= 1e-4 or v == 0 else f"{v:.7f}".rstrip("0")


def fmt_sig(st, scale=1.0):
    if st is None:
        return ""
    if st["n"] == 1:
        return sig(st["median"], scale)
    return f"{sig(st['median'], scale)} [{sig(st['min'], scale)}–{sig(st['max'], scale)}]"


def matrix_table(agg, key, title, scale=1.0):
    """Cells: median [min–max] over reps. A row that failed shows a
    footnote code; the legend under the table gives each distinct error
    text and the runtime that produced it."""
    rts = [rt for rt in RUNTIMES if any(k[1] == rt for k in agg)]
    lines = [f"**{title}**", "", "| case | " + " | ".join(rts) + " |", "|---|" + "---:|" * len(rts)]
    legend = {}  # (runtime, error) -> code
    where = defaultdict(list)  # code -> cases
    for case in CASES:
        cells = []
        for rt in rts:
            a = agg.get((case, rt))
            if a is None:
                cells.append("")
                continue
            codes = []
            for e in a["errors"]:
                code = legend.setdefault((rt, e), f"{'N' if e.startswith('N/A') else 'E'}{len(legend) + 1}")
                codes.append(code)
                where[code].append(case)
            if a["ok"]:
                c = fmt_sig(a[key], scale)
                if a["ok"] < a["n"]:
                    c += f" ({a['ok']}/{a['n']} ok; {','.join(codes)})"
                cells.append(c)
            else:
                cells.append(",".join(codes) or "ERR")
        lines.append(f"| {case} | " + " | ".join(cells) + " |")
    if legend:
        lines.append("")
        for (rt, e), code in legend.items():
            lines.append(f"- {code} ({rt}; {', '.join(where[code])}): {e}")
    return "\n".join(lines) + "\n"


# (feature case, its twin, what the ratio measures)
TWINS = [
    ("callref_dispatch", "callref_dispatch.indirect", "call_ref vs call_indirect"),
    ("mem64_chase", "mem64_chase.mem32", "memory64 vs 32-bit memory"),
    ("multimem_transform", "multimem_transform.single", "3 memories vs 1 (offsets)"),
    ("extconst_init", "extconst_init.mvp", "extended-const vs pre-folded consts"),
    ("eh_parser_exnref", "eh_parser_legacy", "exnref vs legacy EH encoding"),
    ("matmul_fma", "matmul_simd", "relaxed madd vs simd128 mul+add"),
    ("factorial", "factorial.scalar", "auto-vectorized vs scalar build"),
    ("sieve", "sieve.scalar", "auto-vectorized vs scalar build"),
    ("crc32", "crc32.scalar", "auto-vectorized vs scalar build"),
    ("convolution", "convolution.scalar", "auto-vectorized vs scalar build"),
    ("bulk_memory", "bulk_memory.scalar", "auto-vectorized vs scalar build"),
]


def twin_table(agg, title):
    rts = [rt for rt in RUNTIMES if any(k[1] == rt for k in agg)]
    lines = [f"**{title}** — CPU time per call, feature ÷ twin (< 1: the feature's build is faster)", "",
             "| feature | twin | measures | " + " | ".join(rts) + " |",
             "|---|---|---|" + "---:|" * len(rts)]
    for f, t, what in TWINS:
        cells = []
        for rt in rts:
            a, b = agg.get((f, rt)), agg.get((t, rt))
            if a and b and a["ok"] and b["ok"]:
                cells.append(f"{a['cpu_ms_per_iter']['median'] / b['cpu_ms_per_iter']['median']:.2f}")
            else:
                cells.append("—")
        lines.append(f"| {f} | {t} | {what} | " + " | ".join(cells) + " |")
    return "\n".join(lines) + "\n"


SIMD_CASES = {"factorial", "sieve", "crc32", "matmul_simd", "matmul_fma", "convolution",
              "bulk_memory", "relaxed_dot", "relaxed_madd"}
FEATURE_OF = {"eh_parser_exnref": "exnref", "eh_parser_legacy": "legacy EH", "gc_trees": "GC",
              "callref_dispatch": "typed func refs", "mem64_chase": "memory64",
              "multimem_transform": "multi-memory", "graphql_porf": "exceptions",
              "graphql_porf_trycatch": "legacy try/catch", "relaxed_dot": "relaxed SIMD",
              "relaxed_madd": "relaxed SIMD", "matmul_fma": "relaxed SIMD"}


def reason(case, rt, err):
    """Short coverage reason for a failed (case, runtime) row (err is the
    short_err form)."""
    e = err.lower()
    if "wasi preview-1" in e:
        return "N/A: no WASI p1 shim in harness"
    if "never reclaims gc" in e:
        return "N/A: GC heap growth crashes the process"
    if "segfaults" in e:
        return "N/A: v128 ops crash the process"
    if "app killed" in e:
        return "killed by jetsam"
    if "not reported" in e:
        return "not run"
    if "wrong result" in e:
        return "wrong result"
    if "multi-value results" in e:
        return "N/A: C API has no multi-value"
    if "null reference" in e:
        return "traps: typed elem segment not applied"
    if "unreachable" in e and case not in SIMD_CASES:
        return "traps `unreachable`"
    if case in SIMD_CASES and rt in ("wasm3", "zwasm"):
        return "no SIMD" + (" in interpreter" if rt == "zwasm" else "")
    if case in FEATURE_OF:
        return f"no {FEATURE_OF[case]}"
    return "error: " + err[:60]


def coverage_table(aggs):
    """aggs: [(platform, agg)]. ✓ when the row ran on every platform that
    has it; otherwise the reason (per platform when they differ)."""
    lines = ["| case | " + " | ".join(RUNTIMES) + " |", "|---|" + ":-:|" * len(RUNTIMES)]
    for case in CASES:
        cells = []
        for rt in RUNTIMES:
            parts = []
            for plat, agg in aggs:
                a = agg.get((case, rt))
                if a is None:
                    continue
                if a["ok"] == a["n"]:
                    parts.append((plat, "✓"))
                elif a["ok"]:
                    parts.append((plat, f"✓ {a['ok']}/{a['n']}"))
                else:
                    parts.append((plat, reason(case, rt, a["errors"][0] if a["errors"] else "")))
            vals = {v for _, v in parts}
            if not parts:
                cells.append("")
            elif len(vals) == 1:
                cells.append(parts[0][1])
            else:
                cells.append("; ".join(f"{p}: {v}" for p, v in parts))
        lines.append(f"| {case} | " + " | ".join(cells) + " |")
    return "\n".join(lines) + "\n"


def relative_table(agg, title, key="cpu_ms_per_iter", what="CPU time per call ÷ the fastest runtime's (1.00 = fastest)"):
    """A per-call metric relative to the best runtime on each case, and the
    geometric mean over the cases every runtime ran."""
    rts = [rt for rt in RUNTIMES if any(k[1] == rt for k in agg)]
    lines = [f"**{title}** — {what}", "",
             "| case | " + " | ".join(rts) + " |", "|---|" + "---:|" * len(rts)]
    common, logs = [], defaultdict(float)
    for case in CASES:
        vals = {rt: agg[(case, rt)][key]["median"] for rt in rts
                if agg.get((case, rt)) and agg[(case, rt)]["ok"] and agg[(case, rt)].get(key)}
        if not vals:
            continue
        best = min(vals.values())
        lines.append(f"| {case} | " + " | ".join(
            (f"**{vals[rt] / best:.2f}**" if vals[rt] == best else f"{vals[rt] / best:.2f}")
            if rt in vals else "—" for rt in rts) + " |")
        # factorial(20) runs in ~1 µs: timer and call overhead, not the
        # interpreter loop, so it stays out of the geomean.
        if len(vals) == len(rts) and not case.startswith("factorial"):
            common.append(case)
            for rt in rts:
                logs[rt] += math.log(vals[rt] / best)
    if common:
        lines.append(f"| **geomean, {len(common)} cases all ran (not factorial)** | " + " | ".join(
            f"**{math.exp(logs[rt] / len(common)):.2f}**" for rt in rts) + " |")
    return "\n".join(lines) + "\n", common


def residency_table(agg, title):
    lines = [f"**{title}**", "", "| runtime | rows | e_share min | e_share median | IPC median |",
             "|---|---:|---:|---:|---:|"]
    for rt in RUNTIMES:
        es = [a["e_share"]["min"] for (c, r), a in agg.items() if r == rt and a["ok"] and a.get("e_share")]
        em = [a["e_share"]["median"] for (c, r), a in agg.items() if r == rt and a["ok"] and a.get("e_share")]
        ipc = [a["ipc"]["median"] for (c, r), a in agg.items() if r == rt and a["ok"] and a.get("ipc")]
        if es:
            lines.append(f"| {rt} | {len(es)} | {min(es):.3f} | {statistics.median(em):.3f} | "
                         f"{statistics.median(ipc):.2f} |")
    return "\n".join(lines) + "\n"


def write_csv(path, rows, fields):
    with open(path, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields, extrasaction="ignore")
        w.writeheader()
        for r in rows:
            w.writerow(r)


MATRIX_FIELDS = ["rep", "runtime", "case", "ok", "result", "iterations", "load_ns", "min_ns", "median_ns",
                 "p99_ns", "cpu_user_ns", "cpu_system_ns", "p_cpu_ns", "e_share", "ipc", "instructions",
                 "cycles", "rss_peak_bytes", "error"]


# ---------------------------------------------------------------------------
# femtovg E2E
# ---------------------------------------------------------------------------

E2E_FIELDS = ["rep", "runtime", "variant", "scene", "frames", "fps_mean", "fps_median", "frame_ms_p50",
              "frame_ms_p95", "frame_ms_p99", "frame_ms_max", "guest_ms_mean", "encode_ms_mean",
              "gpu_ms_mean", "init_ms", "load_ms", "phys_footprint_peak_bytes",
              "phys_footprint_before_guest_bytes", "linear_memory_peak_bytes", "cpu_ms", "e_share", "ipc",
              "run_instructions", "run_cycles", "all_hash", "final_texture_hash", "adapter", "error"]


def load_e2e_device(d):
    rows = []
    for log in sorted(glob.glob(os.path.join(d, "*.log"))):
        m = re.search(r"-(\w+)-r(\d+)\.log$", os.path.basename(log))
        rep = int(m.group(2)) if m else 0
        for line in open(log, errors="replace"):
            if line.startswith("FEMTOVG_E2E {"):
                r = json.loads(line[len("FEMTOVG_E2E "):])
                r["rep"] = rep
                rows.append(r)
    return rows


def e2e_tables(rows, title):
    """Two tables per platform: frame rate / frame time, and startup /
    memory. Cells are median [min–max] over reps."""
    by = defaultdict(list)
    for r in rows:
        by[(r["runtime"], r["scene"])].append(r)
    speed = [f"**{title}: frame rate and frame time**", "",
             "| runtime | build | scene | FPS mean | FPS median | frame p50 ms | p95 | p99 | "
             "guest ms/frame | encode ms | GPU wait ms | E-share (min) | runs |",
             "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
    has_before = any(r.get("phys_footprint_before_guest_bytes") for r in rows)
    mem = [f"**{title}: startup and memory**", "",
           "| runtime | scene | load ms | init ms | peak footprint MB | "
           + ("footprint before guest MB | " if has_before else "") + "peak linear memory MB |",
           "|---|---:|---:|---:|---:|" + ("---:|" if has_before else "") + "---:|"]
    hashes = defaultdict(set)
    textures = defaultdict(set)
    summary = {}
    keys = ["fps_mean", "fps_median", "frame_ms_p50", "frame_ms_p95", "frame_ms_p99", "guest_ms_mean",
            "encode_ms_mean", "gpu_ms_mean", "phys_footprint_peak_bytes", "linear_memory_peak_bytes",
            "e_share", "load_ms", "init_ms", "phys_footprint_before_guest_bytes"]
    for scene in sorted({s for _, s in by}):
        for rt in RUNTIMES:
            rs = by.get((rt, scene), [])
            oks = [r for r in rs if "error" not in r]
            if not rs:
                continue
            if not oks:
                speed.append(f"| {rt} | | {scene} | {short_err(rs[0]['error'])} |" + " |" * 9)
                continue
            for r in oks:
                hashes[scene].add(r["all_hash"])
                textures[scene].add(r.get("final_texture_hash"))
            st = {k: med_range([r.get(k) for r in oks]) for k in keys}
            summary[f"{rt}/scene{scene}"] = st
            speed.append(
                f"| {rt} | {oks[0]['variant']} | {scene} | {fmt_sig(st['fps_mean'])} | "
                f"{fmt_sig(st['fps_median'])} | {fmt_sig(st['frame_ms_p50'])} | "
                f"{fmt_sig(st['frame_ms_p95'])} | {fmt_sig(st['frame_ms_p99'])} | "
                f"{fmt_sig(st['guest_ms_mean'])} | {fmt_sig(st['encode_ms_mean'])} | "
                f"{fmt_sig(st['gpu_ms_mean'])} | {st['e_share']['min']:.3f} | {len(oks)}/{len(rs)} |")
            before = st["phys_footprint_before_guest_bytes"]
            mem.append(
                f"| {rt} | {scene} | {fmt_sig(st['load_ms'])} | {fmt_sig(st['init_ms'])} | "
                f"{fmt_sig(st['phys_footprint_peak_bytes'], 1e-6)} | "
                + (f"{fmt_sig(before, 1e-6) if before and before['median'] else '—'} | " if has_before else "")
                + f"{fmt_sig(st['linear_memory_peak_bytes'], 1e-6)} |")
    notes = [""]
    for scene, hs in sorted(hashes.items()):
        tx = sorted(t for t in textures[scene] if t)
        notes.append(f"Scene {scene}: {len(hs)} distinct all-frames hash(es) across runtimes and reps "
                     f"({', '.join(sorted(hs))}); {len(tx)} distinct final-texture hash(es) "
                     f"({', '.join(tx)}).")
    return "\n".join(speed) + "\n\n" + "\n".join(mem) + "\n" + "\n".join(notes) + "\n", summary


# ---------------------------------------------------------------------------
# PMU
# ---------------------------------------------------------------------------

# (column, mode, count key): events per 1k instructions. Metric names are
# the Recount ones (Recount.framework/Resources/Analysis/*.json, t6041).
PMU_METRICS = [
    ("L1D ld miss", "l1d_metrics", "l1d_miss_ld_spec"),
    ("L1D st miss", "l1d_metrics", "l1d_miss_st_spec"),
    ("L1I miss", "instruction_address_translation_metrics", "l1i_demand_miss_spec"),
    ("iTLB miss", "instruction_address_translation_metrics", "l1i_tlb_miss_spec"),
    ("iTLB L2 miss", "instruction_address_translation_metrics", "l2i_tlb_miss_spec"),
    ("i-walk", "instruction_address_translation_metrics", "l2i_walk_spec"),
    ("fetch restart", "instruction_address_translation_metrics", "fetch_restart_spec"),
    ("dTLB miss", "data_address_translation_metrics", "l1d_tlb_miss_spec"),
    ("dTLB L2 miss", "data_address_translation_metrics", "l2d_tlb_miss_spec"),
    ("d-walk", "data_address_translation_metrics", "l2d_walk_spec"),
    ("br mispred", "discarded_sampling", "discarded_branch"),
    ("cond mispred", "discarded_sampling", "discarded_cond_branch"),
    ("indir mispred", "discarded_indirect_sampling", "discarded_indirect_branch"),
    ("ret mispred", "discarded_indirect_sampling", "discarded_return"),
    ("indir branches", "call_branch_instructions", "indirect_branch"),
]
MEM_COLS = ["L1D ld miss", "L1D st miss", "L1I miss", "iTLB miss", "iTLB L2 miss", "i-walk",
            "dTLB miss", "dTLB L2 miss", "d-walk"]
CTL_COLS = ["br mispred", "cond mispred", "indir mispred", "ret mispred", "indir branches",
            "indir mispred %", "fetch restart"]
BUCKETS = ["useful", "processing", "delivery", "discarded"]
E2E_CASES = ["femtovg_e2e.scene0", "femtovg_e2e.scene1"]


def load_pmu(d):
    """(runtime, case) -> column -> value. Counts come from the per-thread
    capture; the denominator is the same capture's instruction count for
    the case (run_matrix case_instructions / run_femtovg_e2e
    run_instructions, keyed by the capture's mode index in `rep`)."""
    def jl(name):
        p = os.path.join(d, name)
        return [json.loads(l) for l in open(p)] if os.path.exists(p) else []
    info = open(os.path.join(d, "pmu-pass-info.txt")).read() if os.path.exists(
        os.path.join(d, "pmu-pass-info.txt")) else ""
    modes = info.split("modes:")[1].split() if "modes:" in info else []
    def mode_of(rep):
        return modes[rep - 1] if 0 < rep <= len(modes) else str(rep)
    inst, e_share = {}, {}
    for r in jl("pmu-matrix.jsonl"):
        if r.get("case_instructions"):
            inst[(r["runtime"], r["case"], mode_of(r["rep"]))] = r["case_instructions"]
            e_share.setdefault((r["runtime"], r["case"]), []).append(r.get("e_share"))
    for r in jl("pmu-e2e.jsonl"):
        if r.get("run_instructions"):
            case = f"femtovg_e2e.scene{r['scene']}"
            inst[(r["runtime"], case, mode_of(r.get("rep", 0)))] = r["run_instructions"]
            e_share.setdefault((r["runtime"], case), []).append(r.get("e_share"))
    table = defaultdict(dict)
    # E2E captures: cycles of the femtovg-e2e thread vs the whole process
    # (the Metal driver's threads), per (runtime, case), bottleneck mode.
    e2e_cyc = defaultdict(lambda: [0, 0])
    for s in jl("pmu.jsonl"):
        if s.get("capture", "matrix") != "matrix" and s["mode"].endswith(":bottlenecks"):
            c = e2e_cyc[(s["runtime"], s["capture"])]
            c[1] += s.get("cycles", 0)
            if s.get("case"):
                c[0] += s.get("cycles", 0)
    for s in jl("pmu.jsonl"):
        if not s.get("case"):
            continue
        key = (s["runtime"], s["case"])
        base = s["mode"].split(":")[-1]
        ins = inst.get((s["runtime"], s["case"], s["mode"]))
        if not ins:
            continue  # the case failed in this capture (e.g. the module did not load)
        for col, mode, cnt in PMU_METRICS:
            if base == mode:
                table[key][col] = 1000.0 * s["counts"].get(cnt, 0) / ins
        if base == "bottlenecks":
            for b in BUCKETS:
                if b in s.get("shares", {}):
                    table[key][b] = s["shares"][b]
            if s.get("cycles"):
                table[key]["IPC"] = ins / s["cycles"]
    for (rt, case), (own, total) in e2e_cyc.items():
        if total:
            table[(rt, case)]["thread cycle %"] = 100.0 * own / total
    for key, row in table.items():
        if row.get("indir branches"):
            row["indir mispred %"] = 100.0 * row.get("indir mispred", 0.0) / row["indir branches"]
            row["instr / indir branch"] = 1000.0 / row["indir branches"]
        es = [x for x in e_share.get(key, []) if x is not None]
        if es:
            row["e_share min"] = min(es)
    return table


def fmt_pmu(col, v):
    if col in BUCKETS or col.endswith("%") or col.endswith(" %"):
        return f"{v:.1f}"
    if col == "IPC" or col == "e_share min":
        return f"{v:.2f}"
    return f"{v:.3f}" if v < 10 else f"{v:.1f}"


def pmu_case_table(table, cols, title, runtimes=RUNTIMES, cases=None):
    order = CASES + E2E_CASES
    cases = cases or order
    lines = [f"**{title}**", "", "| runtime | case | " + " | ".join(cols) + " |",
             "|---|---|" + "---:|" * len(cols)]
    for rt in runtimes:
        for case in cases:
            row = table.get((rt, case))
            if not row or not any(c in row for c in cols):
                continue
            lines.append(f"| {rt} | {case} | " + " | ".join(
                fmt_pmu(c, row[c]) if c in row else "" for c in cols) + " |")
    return "\n".join(lines) + "\n"


def pmu_single_runtime_tables(table, rt):
    """One profiled runtime: per-case rows (matrix cases, then the E2E),
    with the median over the matrix cases as the last row."""
    def one(cols, title):
        lines = [f"**{title}**", "", "| case | " + " | ".join(cols) + " |",
                 "|---|" + "---:|" * len(cols)]
        for case in CASES + E2E_CASES:
            row = table.get((rt, case))
            if row and any(c in row for c in cols):
                lines.append(f"| {case} | " + " | ".join(
                    fmt_pmu(c, row[c]) if c in row else "" for c in cols) + " |")
        rows = [table[(rt, c)] for c in CASES if (rt, c) in table]
        med = {c: statistics.median([r[c] for r in rows if c in r])
               for c in cols if any(c in r for r in rows)}
        lines.append("| **median, matrix cases** | " + " | ".join(
            f"**{fmt_pmu(c, med[c])}**" if c in med else "" for c in cols) + " |")
        return "\n".join(lines) + "\n"

    return (one(BUCKETS + ["IPC", "instr / indir branch", "indir mispred %", "br mispred",
                           "cond mispred", "fetch restart", "e_share min"],
                f"{rt}: pipeline slots (% of slots), IPC and control side (events per 1k "
                "instructions)")
            + "\n" + one(MEM_COLS, f"{rt}: memory side, events per 1k instructions"))


def pmu_runtime_profile(table):
    """Median over the matrix workloads of each column, per runtime (the
    basis of the per-runtime bottleneck hypotheses), plus the E2E rows."""
    cols = BUCKETS + ["IPC"] + MEM_COLS + CTL_COLS
    prof = {}
    lines = ["| runtime | n | " + " | ".join(cols) + " |", "|---|---:|" + "---:|" * len(cols)]
    for rt in RUNTIMES:
        rows = [row for (r, case), row in table.items() if r == rt and case in CASES]
        if not rows:
            continue
        prof[rt] = {c: statistics.median([row[c] for row in rows if c in row])
                    for c in cols if any(c in row for row in rows)}
        n = sum(1 for row in rows if "useful" in row)
        lines.append(f"| {rt} | {n} | " + " | ".join(
            fmt_pmu(c, prof[rt][c]) if c in prof[rt] else "" for c in cols) + " |")
    return "\n".join(lines) + "\n", prof


# ---------------------------------------------------------------------------

def main():
    root, data = sys.argv[1], sys.argv[2]
    os.makedirs(data, exist_ok=True)
    md = []
    sections = defaultdict(list)  # name -> markdown parts, written to tables-<name>.md
    summary = {}
    coverage = []

    def emit(name, text):
        md.append(text)
        sections[name].append(text)

    # m4/ is the pass; m4-*/ are supplementary passes over added cases.
    m4s = sorted(glob.glob(os.path.join(root, "m4*", "m4-matrix.jsonl")))
    if m4s:
        rows = [r for p in m4s for r in load_matrix_jsonl(p)]
        write_csv(os.path.join(data, "m4-matrix.csv"), rows, MATRIX_FIELDS)
        agg = aggregate_matrix(rows)
        coverage.append(("M4", agg))
        summary["m4_matrix"] = {f"{c}/{r}": v for (c, r), v in agg.items()}
        emit("m4", matrix_table(agg, "cpu_ms_per_iter", "CPU time per call, ms (cpu_user / iterations)"))
        emit("m4", matrix_table(agg, "median_ms", "Wall time per call, ms: median [range] over reps"))
        emit("m4", residency_table(agg, "Measured E-core residency and IPC"))
        emit("twins", twin_table(agg, "M4 E-cores"))
        t, common = relative_table(agg, "M4 relative CPU time")
        emit("m4-relative", t)
        t, _ = relative_table(agg, "M4 retired instructions per call", key="insns_per_iter",
                              what="instructions per call ÷ the fewest (1.00 = fewest)")
        emit("m4-instructions", t)
        summary["m4_common_cases"] = common

    # iphonexs/ is the pass; iphonexs-*/ are supplementary runs over added
    # cases. E2E runs (iphonexs-e2e*/) have their own tables.
    devs = [d for d in sorted(glob.glob(os.path.join(root, "iphonexs*")))
            if os.path.isdir(d) and "-e2e" not in os.path.basename(d)]
    if devs:
        rows = load_device_logs(devs)
        write_csv(os.path.join(data, "iphonexs-matrix.csv"), rows, MATRIX_FIELDS)
        agg = aggregate_matrix(rows)
        coverage.append(("iPhone", agg))
        summary["iphonexs_matrix"] = {f"{c}/{r}": v for (c, r), v in agg.items()}
        emit("iphone", matrix_table(agg, "cpu_ms_per_iter", "CPU time per call, ms (cpu_user / iterations)"))
        emit("iphone", matrix_table(agg, "median_ms", "Wall time per call, ms: median [range] over reps"))
        emit("iphone", residency_table(agg, "Measured E-core residency and IPC"))
        emit("twins", twin_table(agg, "iPhone XS E-cores"))
        t, common = relative_table(agg, "iPhone relative CPU time")
        emit("iphone-relative", t)
        t, _ = relative_table(agg, "iPhone XS retired instructions per call", key="insns_per_iter",
                              what="instructions per call ÷ the fewest (1.00 = fewest)")
        emit("iphone-instructions", t)
        summary["iphonexs_common_cases"] = common

    for name, loader, path in [("m4", lambda p: [json.loads(l) for l in open(p)],
                                os.path.join(root, "m4", "m4-e2e.jsonl")),
                               ("iphonexs", load_e2e_device, os.path.join(root, "iphonexs-e2e"))]:
        if os.path.exists(path):
            rows = loader(path)
            for r in rows:
                if r.get("cycles") and "ipc" not in r:
                    r["ipc"] = r.get("ipc")
            write_csv(os.path.join(data, f"{name}-e2e.csv"), rows, E2E_FIELDS)
            with open(os.path.join(data, f"{name}-e2e-frames.jsonl"), "w") as f:
                for r in rows:
                    f.write(json.dumps({k: r.get(k) for k in ("rep", "runtime", "scene", "frame_hashes",
                                                               "frame_verts", "frame_commands")}) + "\n")
            t, s = e2e_tables(rows, {"m4": "M4 Max E-cores", "iphonexs": "iPhone XS E-cores"}[name])
            emit(f"e2e-{name}", t)
            summary[f"{name}_e2e"] = s

    # Supplementary E2E launches (e.g. iphonexs-e2e-baseline/): raw rows
    # only, no table.
    for d in sorted(glob.glob(os.path.join(root, "iphonexs-e2e-*"))):
        if os.path.isdir(d):
            write_csv(os.path.join(data, os.path.basename(d) + ".csv"), load_e2e_device(d), E2E_FIELDS)

    cm = os.path.join(root, "m4", "cm-async", "cm-async.jsonl")
    if os.path.exists(cm):
        rows = [json.loads(l) for l in open(cm)]
        write_csv(os.path.join(data, "cm-async.csv"), rows,
                  ["runtime", "run", "phase", "inner_rep", "n", "total_ns", "ns_per_op"])
        emit("cm", open(os.path.join(root, "m4", "cm-async", "summary.md")).read())

    pmu_dir = os.path.join(root, "pmu")
    if os.path.isdir(pmu_dir):
        table = load_pmu(pmu_dir)
        cols = BUCKETS + ["IPC"] + MEM_COLS + CTL_COLS + ["instr / indir branch", "e_share min",
                                                          "thread cycle %"]
        with open(os.path.join(data, "pmu.csv"), "w", newline="") as f:
            w = csv.writer(f)
            w.writerow(["runtime", "case"] + cols)
            for rt in RUNTIMES:
                for case in CASES + E2E_CASES:
                    if (rt, case) in table:
                        w.writerow([rt, case] + [table[(rt, case)].get(c, "") for c in cols])
        prof_md, prof = pmu_runtime_profile(table)
        summary["pmu_profile"] = prof
        pmu_rts = [rt for rt in RUNTIMES if any(k[0] == rt for k in table)]
        if len(pmu_rts) == 1:
            emit("pmu", pmu_single_runtime_tables(table, pmu_rts[0]))
        else:
            emit("pmu", "Per runtime, median over the matrix workloads (events per 1k instructions; "
                        "buckets in % of slots):\n\n" + prof_md)
            emit("pmu", pmu_case_table(table, BUCKETS + ["IPC", "thread cycle %"] + MEM_COLS[:3] + CTL_COLS,
                                     "femtovg E2E (femtovg-e2e thread; thread cycle % = its share "
                                     "of the process's cycles, the rest is the Metal driver's threads)",
                                     cases=E2E_CASES))
            with open(os.path.join(data, "pmu-tables.md"), "w") as f:
                f.write(pmu_case_table(table, BUCKETS + ["IPC", "e_share min"],
                                       "Bottleneck buckets (% of slots) and IPC"))
                f.write("\n" + pmu_case_table(table, MEM_COLS, "Memory side, per 1k instructions"))
                f.write("\n" + pmu_case_table(table, CTL_COLS, "Control side, per 1k instructions"))

    mem = os.path.join(root, "m4-memory", "memory.jsonl")
    if os.path.exists(mem):
        rows = [json.loads(l) for l in open(mem)]
        write_csv(os.path.join(data, "m4-memory.csv"), rows,
                  MATRIX_FIELDS + ["phys_footprint_bytes", "phys_footprint_peak_bytes"])
        by = {(r["case"], r["runtime"]): r for r in rows}
        rts = [rt for rt in RUNTIMES if any(k[1] == rt for k in by)]
        lines = ["Peak phys_footprint in MB (task_vm_info ledger peak) over load, warmup and a "
                 "2000 ms window, one process per runtime × case; in parentheses the footprint "
                 "still held after the case when it is at least 20 MB. Failed rows are blank.", "",
                 "| case | " + " | ".join(rts) + " |", "|---|" + "---:|" * len(rts)]
        summary["m4_memory"] = {}
        for case in CASES:
            cells = []
            for rt in rts:
                r = by.get((case, rt))
                if r and r.get("ok") and r.get("phys_footprint_peak_bytes"):
                    v = r["phys_footprint_peak_bytes"] / 1e6
                    after = r.get("phys_footprint_bytes", 0) / 1e6
                    summary["m4_memory"][f"{case}/{rt}"] = {"peak_mb": v, "after_mb": after}
                    cell = f"{v:.0f}" if v >= 10 else f"{v:.1f}"
                    if after >= 20:
                        cell += f" ({after:.0f})"
                    cells.append(cell)
                else:
                    cells.append("")
            lines.append(f"| {case} | " + " | ".join(cells) + " |")
        emit("memory", "\n".join(lines) + "\n")

    if coverage:
        emit("coverage", coverage_table(coverage))

    ab = os.path.join(root, "pulley-ab", "ab.jsonl")
    if os.path.exists(ab):
        rows = [json.loads(l) for l in open(ab)]
        write_csv(os.path.join(data, "pulley-dispatch-ab.csv"), rows, ["dispatch"] + MATRIX_FIELDS)
        by = defaultdict(list)
        for r in rows:
            if r.get("ok"):
                by[(r["case"], r["dispatch"])].append(r)
        lines = ["| case | tail (`--cfg=pulley_tail_calls`) CPU ms/call | match loop CPU ms/call | "
                 "match ÷ tail, CPU | match ÷ tail, cycles | tail wall ms | match wall ms |",
                 "|---|---:|---:|---:|---:|---:|---:|"]
        summary["pulley_ab"] = {}
        for case in CASES:
            t, m = by.get((case, "tail")), by.get((case, "match"))
            if not t or not m:
                continue
            ct = med_range([r["cpu_user_ns"] / 1e6 / r["iterations"] for r in t])
            cm = med_range([r["cpu_user_ns"] / 1e6 / r["iterations"] for r in m])
            wt = med_range([r["median_ns"] / 1e6 for r in t])
            wm = med_range([r["median_ns"] / 1e6 for r in m])
            yt = statistics.median(r["cycles"] / r["iterations"] for r in t)
            ym = statistics.median(r["cycles"] / r["iterations"] for r in m)
            summary["pulley_ab"][case] = {"tail_cpu": ct, "match_cpu": cm, "tail_wall": wt, "match_wall": wm,
                                          "cycles_ratio": ym / yt}
            lines.append(f"| {case} | {fmt_sig(ct)} | {fmt_sig(cm)} | {cm['median'] / ct['median']:.2f}× | "
                         f"{ym / yt:.2f}× | {fmt_sig(wt)} | {fmt_sig(wm)} |")
        emit("pulley-ab", "\n".join(lines) + "\n")

    with open(os.path.join(data, "summary.json"), "w") as f:
        json.dump(summary, f, indent=1, default=str)
    with open(os.path.join(data, "tables.md"), "w") as f:
        f.write("\n".join(md))
    for name, parts in sections.items():
        with open(os.path.join(data, f"tables-{name}.md"), "w") as f:
            f.write("\n".join(parts))
    print(f"wrote {data}: " + ", ".join(sorted(os.listdir(data))))


if __name__ == "__main__":
    main()
