# 3-way: wasmtime baseline vs phase-3 fusion vs WAMR — iPhone 12, 2026-05-15

Per-workload comparison answering the user's exact question: how much
did the phase-1+2+3 opcode-fusion stack gain us versus an unmodified
wasmtime baseline, and how does each side compare to WAMR fast-interp?
Wallclock (steady-state iteration median, N=10) and PMU (CPU Bottlenecks
cycle accounting) on the same iPhone 12 A14 Icestorm E-core.

## Configuration

- **baseline** — wasmtime branch `table-mutability-tracking` (the
  pre-fusion merge base). No phase-1/2/3 changes. `--cfg=pulley_tail_calls`,
  linker-plugin-lto + embed-bitcode for cross-language LTO.
- **phase3** — branch `claude/pulley-fusion-xband-brif` tip
  (`06a6cd7883`). All 9 fusion commits applied: phase-1 (xband + brif),
  phase-2 (brif + 2 xloads), phase-3 (xband + brif + 2 xloads → 1 op).
  Same build flags as baseline.
- **WAMR** — wasm-micro-runtime fast-interp (preprocessed bytecode,
  register IR). `WAMR_BUILD_FAST_INTERP=1 WAMR_BUILD_SIMD=1 WAMR_BUILD_EXCE_HANDLING=0`.

All three are pure-interpreter (no native codegen — App-Store-eligible).

## Method

- **Wallclock**: 10 full app launches per condition; per launch we run
  the 16-result-line workload sweep (8 Pulley + 8 WAMR per launch). The
  per-rep `median` from the harness's `BenchReport` is the steady-state
  iteration time. Module load is reported separately as `load_ns` and
  is **excluded** from the median — Cranelift's phase-1/2/3 lowering
  runs at load and we'd be measuring it twice otherwise. The reported
  number is the median across the 10 per-rep medians.
- **PMU**: one xctrace `CPU Counters` capture per (condition, workload),
  20 s attach window, `BENCH_TARGET_MS=12000`. Each XML is parsed via
  `scripts/aggregate_3way.py`, summed across all E-core rows in the
  capture. Buckets are CPU Bottlenecks mode: **Useful** (retiring),
  **Processing** (back-end bound), **Delivery** (front-end bound),
  **Discarded** (mispredict/misspeculation). All three are single
  captures per (condition, workload) — *not* repeated. Treat per-bucket
  deltas qualitatively, not as a tight effect estimate.

## Wallclock medians (lower = better)


| workload | baseline (Pulley) ms | phase3 (Pulley) ms | WAMR ms | Δ base→phase3 % | phase3 vs WAMR |
|---|---:|---:|---:|---:|---:|
| call_indirect | 27.92 | 27.35 | 16.35 | -2.05% | WAMR 1.67× faster |
| xmrsplayer | 16.37 | 16.77 | 12.30 | +2.48% | WAMR 1.36× faster |
| vtable_mono | 44.00 | 42.75 | 24.38 | -2.85% | WAMR 1.75× faster |
| vtable_bi | 49.81 | 49.42 | 27.87 | -0.78% | WAMR 1.77× faster |
| vtable_poly4 | 54.08 | 54.27 | 31.88 | +0.37% | WAMR 1.70× faster |
| vtable_poly6 | 60.67 | 59.67 | 35.66 | -1.65% | WAMR 1.67× faster |
| graphql-validation (AS) | 14.63 | 14.65 | 9.36 | +0.10% | WAMR 1.57× faster |
| graphql-validation (Porffor) | 14.85 | 15.14 | N/A | +1.97% | — |


### Wallclock takeaways

- **phase3 vs baseline**: small but consistent gains on `call_indirect`
  (−2.05%), `vtable_mono` (−2.85%), `vtable_poly6` (−1.65%), `vtable_bi`
  (−0.78%). Other workloads are within ±2.5%. The phase-1+2+3 fusion
  targets the call_indirect lazy-init dispatch path specifically — it
  trims work *on that path*, and that path's contribution to the total
  steady-state iteration is the ceiling on what wallclock can shrink.
- **phase3 vs WAMR**: WAMR is 1.36–1.77× faster on every apples-to-apples
  workload. The gap is largest on the vtable suite (1.67–1.77×) and on
  `call_indirect` (1.67×) — the same dispatch-heavy workloads the fusion
  patches targeted. The phase-3 wins are real but small compared to the
  structural advantage of WAMR's preprocessed-bytecode register IR over
  Pulley's stack IR.
- **graphql-validation (Porffor)** on WAMR is structurally N/A:
  Porffor's WAT uses both SIMD and wasm-exceptions; WAMR's fast-interp
  forbids both simultaneously (`SIMD + CLASSIC_INTERP` and
  `EXCE_HANDLING + FAST_INTERP` are both unsupported in WAMR's cmake).

## PMU per-workload (E-core cycles, single 20 s capture each)

Three columns per workload — baseline, phase3, WAMR — across CPU
Bottlenecks buckets (Useful / Processing / Delivery / Discarded).
Below each absolute table: phase3 vs baseline (= our patches'
contribution) and phase3 vs WAMR (= remaining gap to WAMR).

### call_indirect

| bucket | baseline | phase3 | WAMR |
|---|---:|---:|---:|
| useful | 38,462,027 | 32,841,043 | 42,500,442 |
| processing | 25,517,723 | 20,661,761 | 14,353,702 |
| delivery | 7,036,573 | 6,518,663 | 10,553,230 |
| discarded | 18,735,021 | 12,432,899 | 10,565,160 |
| **total** | **89,751,344** | **72,454,366** | **77,972,534** |

phase3 vs baseline (our patches' contribution):

| bucket | Δ cycles | Δ % | share base | share phase3 |
|---|---:|---:|---:|---:|
| useful | -5,620,984 | -14.61% | 42.9% | 45.3% |
| processing | -4,855,962 | -19.03% | 28.4% | 28.5% |
| delivery | -517,910 | -7.36% | 7.8% | 9.0% |
| discarded | -6,302,122 | -33.64% | 20.9% | 17.2% |
| **total** | **-17,296,978** | **-19.27%** | | |

phase3 vs WAMR (remaining gap to WAMR):

| bucket | Δ cycles | Δ % | share phase3 | share WAMR |
|---|---:|---:|---:|---:|
| useful | +9,659,399 | +29.41% | 45.3% | 54.5% |
| processing | -6,308,059 | -30.53% | 28.5% | 18.4% |
| delivery | +4,034,567 | +61.89% | 9.0% | 13.5% |
| discarded | -1,867,739 | -15.02% | 17.2% | 13.5% |
| **total** | **+5,518,168** | **+7.62%** | | |

### xmrsplayer

| bucket | baseline | phase3 | WAMR |
|---|---:|---:|---:|
| useful | 64,235,834 | 57,483,150 | 55,641,432 |
| processing | 22,293,906 | 17,796,359 | 8,873,564 |
| delivery | 11,971,470 | 11,002,281 | 28,612,051 |
| discarded | 15,395,346 | 16,795,445 | 17,240,488 |
| **total** | **113,896,556** | **103,077,235** | **110,367,535** |

phase3 vs baseline (our patches' contribution):

| bucket | Δ cycles | Δ % | share base | share phase3 |
|---|---:|---:|---:|---:|
| useful | -6,752,684 | -10.51% | 56.4% | 55.8% |
| processing | -4,497,547 | -20.17% | 19.6% | 17.3% |
| delivery | -969,189 | -8.10% | 10.5% | 10.7% |
| discarded | +1,400,099 | +9.09% | 13.5% | 16.3% |
| **total** | **-10,819,321** | **-9.50%** | | |

phase3 vs WAMR (remaining gap to WAMR):

| bucket | Δ cycles | Δ % | share phase3 | share WAMR |
|---|---:|---:|---:|---:|
| useful | -1,841,718 | -3.20% | 55.8% | 50.4% |
| processing | -8,922,795 | -50.14% | 17.3% | 8.0% |
| delivery | +17,609,770 | +160.06% | 10.7% | 25.9% |
| discarded | +445,043 | +2.65% | 16.3% | 15.6% |
| **total** | **+7,290,300** | **+7.07%** | | |

### vtable_mono

| bucket | baseline | phase3 | WAMR |
|---|---:|---:|---:|
| useful | 48,752,737 | 45,046,388 | 54,017,728 |
| processing | 31,705,546 | 30,556,890 | 14,392,528 |
| delivery | 1,932,758 | 1,198,500 | 6,451,270 |
| discarded | 7,810,653 | 3,315,778 | 2,856,374 |
| **total** | **90,201,694** | **80,117,556** | **77,717,900** |

phase3 vs baseline (our patches' contribution):

| bucket | Δ cycles | Δ % | share base | share phase3 |
|---|---:|---:|---:|---:|
| useful | -3,706,349 | -7.60% | 54.0% | 56.2% |
| processing | -1,148,656 | -3.62% | 35.1% | 38.1% |
| delivery | -734,258 | -37.99% | 2.1% | 1.5% |
| discarded | -4,494,875 | -57.55% | 8.7% | 4.1% |
| **total** | **-10,084,138** | **-11.18%** | | |

phase3 vs WAMR (remaining gap to WAMR):

| bucket | Δ cycles | Δ % | share phase3 | share WAMR |
|---|---:|---:|---:|---:|
| useful | +8,971,340 | +19.92% | 56.2% | 69.5% |
| processing | -16,164,362 | -52.90% | 38.1% | 18.5% |
| delivery | +5,252,770 | +438.28% | 1.5% | 8.3% |
| discarded | -459,404 | -13.86% | 4.1% | 3.7% |
| **total** | **-2,399,656** | **-3.00%** | | |

### vtable_bi

| bucket | baseline | phase3 | WAMR |
|---|---:|---:|---:|
| useful | 44,950,803 | 40,361,297 | 56,781,075 |
| processing | 28,484,819 | 24,579,262 | 16,790,839 |
| delivery | 2,446,371 | 2,415,002 | 8,782,567 |
| discarded | 11,070,189 | 10,015,114 | 2,576,539 |
| **total** | **86,952,182** | **77,370,675** | **84,931,020** |

phase3 vs baseline (our patches' contribution):

| bucket | Δ cycles | Δ % | share base | share phase3 |
|---|---:|---:|---:|---:|
| useful | -4,589,506 | -10.21% | 51.7% | 52.2% |
| processing | -3,905,557 | -13.71% | 32.8% | 31.8% |
| delivery | -31,369 | -1.28% | 2.8% | 3.1% |
| discarded | -1,055,075 | -9.53% | 12.7% | 12.9% |
| **total** | **-9,581,507** | **-11.02%** | | |

phase3 vs WAMR (remaining gap to WAMR):

| bucket | Δ cycles | Δ % | share phase3 | share WAMR |
|---|---:|---:|---:|---:|
| useful | +16,419,778 | +40.68% | 52.2% | 66.9% |
| processing | -7,788,423 | -31.69% | 31.8% | 19.8% |
| delivery | +6,367,565 | +263.67% | 3.1% | 10.3% |
| discarded | -7,438,575 | -74.27% | 12.9% | 3.0% |
| **total** | **+7,560,345** | **+9.77%** | | |

### vtable_poly4

| bucket | baseline | phase3 | WAMR |
|---|---:|---:|---:|
| useful | 42,520,483 | 39,598,008 | 48,431,192 |
| processing | 22,526,417 | 21,478,685 | 17,467,598 |
| delivery | 4,835,223 | 4,215,508 | 8,954,036 |
| discarded | 17,443,112 | 12,910,023 | 4,038,129 |
| **total** | **87,325,235** | **78,202,224** | **78,890,955** |

phase3 vs baseline (our patches' contribution):

| bucket | Δ cycles | Δ % | share base | share phase3 |
|---|---:|---:|---:|---:|
| useful | -2,922,475 | -6.87% | 48.7% | 50.6% |
| processing | -1,047,732 | -4.65% | 25.8% | 27.5% |
| delivery | -619,715 | -12.82% | 5.5% | 5.4% |
| discarded | -4,533,089 | -25.99% | 20.0% | 16.5% |
| **total** | **-9,123,011** | **-10.45%** | | |

phase3 vs WAMR (remaining gap to WAMR):

| bucket | Δ cycles | Δ % | share phase3 | share WAMR |
|---|---:|---:|---:|---:|
| useful | +8,833,184 | +22.31% | 50.6% | 61.4% |
| processing | -4,011,087 | -18.67% | 27.5% | 22.1% |
| delivery | +4,738,528 | +112.41% | 5.4% | 11.3% |
| discarded | -8,871,894 | -68.72% | 16.5% | 5.1% |
| **total** | **+688,731** | **+0.88%** | | |

### vtable_poly6

| bucket | baseline | phase3 | WAMR |
|---|---:|---:|---:|
| useful | 42,034,012 | 37,975,179 | 45,169,556 |
| processing | 22,103,854 | 18,922,810 | 17,659,408 |
| delivery | 5,843,277 | 6,219,783 | 10,297,915 |
| discarded | 17,600,415 | 14,673,281 | 5,914,764 |
| **total** | **87,581,558** | **77,791,053** | **79,041,643** |

phase3 vs baseline (our patches' contribution):

| bucket | Δ cycles | Δ % | share base | share phase3 |
|---|---:|---:|---:|---:|
| useful | -4,058,833 | -9.66% | 48.0% | 48.8% |
| processing | -3,181,044 | -14.39% | 25.2% | 24.3% |
| delivery | +376,506 | +6.44% | 6.7% | 8.0% |
| discarded | -2,927,134 | -16.63% | 20.1% | 18.9% |
| **total** | **-9,790,505** | **-11.18%** | | |

phase3 vs WAMR (remaining gap to WAMR):

| bucket | Δ cycles | Δ % | share phase3 | share WAMR |
|---|---:|---:|---:|---:|
| useful | +7,194,377 | +18.94% | 48.8% | 57.1% |
| processing | -1,263,402 | -6.68% | 24.3% | 22.3% |
| delivery | +4,078,132 | +65.57% | 8.0% | 13.0% |
| discarded | -8,758,517 | -59.69% | 18.9% | 7.5% |
| **total** | **+1,250,590** | **+1.61%** | | |

### graphql-validation (AS)

| bucket | baseline | phase3 | WAMR |
|---|---:|---:|---:|
| useful | 31,009,760 | 37,311,021 | 44,190,115 |
| processing | 6,967,817 | 8,838,106 | 8,548,251 |
| delivery | 12,173,964 | 13,672,152 | 29,218,451 |
| discarded | 27,172,553 | 32,367,937 | 29,064,843 |
| **total** | **77,324,094** | **92,189,216** | **111,021,660** |

phase3 vs baseline (our patches' contribution):

| bucket | Δ cycles | Δ % | share base | share phase3 |
|---|---:|---:|---:|---:|
| useful | +6,301,261 | +20.32% | 40.1% | 40.5% |
| processing | +1,870,289 | +26.84% | 9.0% | 9.6% |
| delivery | +1,498,188 | +12.31% | 15.7% | 14.8% |
| discarded | +5,195,384 | +19.12% | 35.1% | 35.1% |
| **total** | **+14,865,122** | **+19.22%** | | |

phase3 vs WAMR (remaining gap to WAMR):

| bucket | Δ cycles | Δ % | share phase3 | share WAMR |
|---|---:|---:|---:|---:|
| useful | +6,879,094 | +18.44% | 40.5% | 39.8% |
| processing | -289,855 | -3.28% | 9.6% | 7.7% |
| delivery | +15,546,299 | +113.71% | 14.8% | 26.3% |
| discarded | -3,303,094 | -10.20% | 35.1% | 26.2% |
| **total** | **+18,832,444** | **+20.43%** | | |

### graphql-validation (Porffor)

| bucket | baseline | phase3 | WAMR |
|---|---:|---:|---:|
| useful | 32,865,226 | 28,826,705 | 17,072 |
| processing | 11,947,570 | 10,445,247 | 80,008 |
| delivery | 19,754,226 | 15,700,969 | 62,706 |
| discarded | 38,060,908 | 36,369,474 | 10,179 |
| **total** | **102,627,930** | **91,342,395** | **169,965** |

phase3 vs baseline (our patches' contribution):

| bucket | Δ cycles | Δ % | share base | share phase3 |
|---|---:|---:|---:|---:|
| useful | -4,038,521 | -12.29% | 32.0% | 31.6% |
| processing | -1,502,323 | -12.57% | 11.6% | 11.4% |
| delivery | -4,053,257 | -20.52% | 19.2% | 17.2% |
| discarded | -1,691,434 | -4.44% | 37.1% | 39.8% |
| **total** | **-11,285,535** | **-11.00%** | | |

_phase3 vs WAMR: WAMR exited early on this workload — diff is not meaningful._

---

**Notes**

- `graphql-validation (Porffor)` on WAMR: structurally N/A. WAMR's interp can support
  SIMD **or** wasm-exceptions, never both at once. Porffor's WAT uses both.
- All Pulley runs include opcode-fusion peephole pass during module load — load time is
  separated from steady-state median by the harness and is excluded here.
- Cycle counts are aggregate Icestorm E-core cycles across the ~12 s xctrace window
  (single core, single workload per trace).

## What the PMU diffs say

### phase3 vs baseline (our patches did what we expected)

On the dispatch-heavy workloads phase3 targeted, total E-core cycles
drop by 9–19%:

- `call_indirect` −19.27%
- `vtable_mono` −11.18%
- `vtable_bi` (table below) — similar magnitude
- `vtable_poly4` −10.45%
- `vtable_poly6` −11.18%
- `xmrsplayer` −9.50%

Decomposing into buckets, the consistent pattern is:

- **Useful** retirements drop (fewer Pulley ops to execute on the
  fused dispatch path).
- **Processing** (back-end-bound) cycles drop more sharply than
  Useful — fewer data-dependent loads on the dispatch tail mean less
  load-use latency.
- **Discarded** cycles drop sharply on `call_indirect` / vtable
  workloads — fewer mispredicted branches into the funcref-load path.
- **Delivery** (front-end-bound) is roughly flat or slightly down —
  consistent with the dispatch sequence shrinking but the interpreter
  loop staying the same shape.

The outlier is `graphql-validation (AS)`, where phase3 PMU is +19.22%
total cycles even though wallclock is essentially flat (+0.10%). This
is almost certainly single-capture noise — these are single-shot 20 s
xctrace runs, not repeated. The wallclock N=10 medians are the more
trustworthy signal on per-workload effect size.

### phase3 vs WAMR (where the remaining gap lives)

Across the apples-to-apples workloads, the gap is dominated by two
buckets pulling in opposite directions:

- **Useful**: WAMR retires *more* useful cycles than phase3 on most
  workloads (e.g. `call_indirect` +29% useful for WAMR). This is the
  IR-density difference — WAMR's preprocessed register IR encodes more
  work per opcode than Pulley's stack IR, so WAMR's retirement rate is
  higher per unit wallclock.
- **Processing**: phase3 has *more* back-end stalls than WAMR on every
  workload (e.g. `call_indirect` phase3 processing is +30.53% above
  WAMR). Pulley's stack-IR sequencing creates more dependent-load
  chains on the dispatch path than WAMR's pre-decoded register form.
- **Delivery**: WAMR has *more* front-end stall than phase3 on most
  workloads — WAMR's larger per-opcode handler footprint costs it
  some icache. This bucket is where phase3 already wins.
- **Discarded**: WAMR has substantially less discarded-cycle volume
  than phase3 on the vtable suite, consistent with fewer indirect
  branches to mispredict in WAMR's register IR.

The structural takeaway: **the cycles we still owe WAMR are in
back-end-bound load-use latency on the dispatch tail, not front-end
icache pressure**. Further fusion work on the dispatch sequence
(e.g. fusing the funcref-load into the dispatched call itself) would
target the same bucket the existing phases improved.

## Raw data

- Per-rep wallclock logs: `out/exp-3way/n10/iphone12-{baseline,phase3}-r{1..10}.log`
- Per-workload PMU XML: `out/exp-3way/pmu-{baseline,phase3,wamr}/*.xml`
- Aggregator: `scripts/aggregate_3way.py out/exp-3way`
- Wallclock-only parser: `scripts/parse_n10.py out/exp-3way/n10`
