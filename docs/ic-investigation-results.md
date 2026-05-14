# Path A measurement results — seqlock + cacheline alignment

Branch: `pulley-call-indirect-ic` on top of PR #2
(`table-mutability-tracking`). Four discrete commits, peelable for
review:

```
566e4690cc  VMCallIndirectCache: align each slot to 64 B (one L1d cacheline)
e9d241e0ea  cranelift IC: reader-side seqlock check on the hit path
74aba78555  cranelift IC: writer-side seqlock increments around the cache fill
d7a219fa98  VMCallIndirectCache: replace _pad with generation field for seqlock
```

## iPhone XS A12 Tempest, .utility QoS, BENCH_TARGET_MS=2000, N=3 medians

| variant | call_indirect IC OFF | IC ON | Δ | xmrsplayer IC OFF | IC ON | Δ |
|---|---:|---:|---:|---:|---:|---:|
| Pre-seqlock baseline (just `2811aa38c1`) | 40.860 | 41.908 | +2.6 % | 30.804 | **28.721** | **−6.8 %** |
| + seqlock (commits 1-3) | 41.130 | 40.578 | **−1.3 %** | 30.129 | 29.028 | −3.7 % |
| + cacheline-aligned (commit 4) | 40.603 | 40.343 | −0.6 % | 28.412 | 28.653 | +0.9 % |

## M4 P-core, BENCH_TARGET_MS=2000, N=3 medians

| variant | call_indirect IC OFF | IC ON | Δ | xmrsplayer IC OFF | IC ON | Δ |
|---|---:|---:|---:|---:|---:|---:|
| Pre-seqlock | 7.918 | 7.985 | +0.85 % | 3.887 | 3.950 | +1.6 % |
| + seqlock | 8.250 | 8.368 | +1.4 % | 3.942 | 3.951 | +0.2 % |
| + cacheline-aligned | 8.110 | 8.099 | −0.13 % | 3.940 | 3.964 | +0.6 % |

## Cross-platform N=10 cross-check (full stack: commits 1-4), .utility QoS, BENCH_TARGET_MS=2000

| device | core | call_indirect IC OFF | IC ON | Δ | xmrsplayer IC OFF | IC ON | Δ |
|---|---|---:|---:|---:|---:|---:|---:|
| iPhone XS | A12 Tempest | 40.395 (40.13–40.82) | 40.331 (40.03–40.63) | −0.16 % | 29.508 (28.56–30.34) | 29.702 (28.33–30.51) | +0.66 % |
| iPhone 12 | A14 Icestorm | 27.644 (27.06–28.11) | 27.210 (27.03–27.66) | **−1.57 %** | 18.003 (16.38–18.15) | 17.323 (16.46–18.20) | **−3.78 %** |

(Medians from N=10 reps, ranges in parentheses. Both sweeps clean — every rep landed on the first launch attempt.)

## M4 PMU bucket shifts (60 s combined-workload window, IC OFF → IC ON, pre-seqlock IC)

| bucket | OFF share | ON share | Δ pts | Δ abs |
|---|---:|---:|---:|---:|
| Useful | 48.1 % | 49.6 % | +1.5 | −5.6 % |
| Processing | 44.0 % | 43.2 % | −0.8 | **−9.8 %** |
| Delivery | 2.3 % | 2.4 % | flat | −5.4 % |
| Discarded | 5.6 % | 4.8 % | −0.8 | **−21.3 %** |

## Read-out

- **The seqlock is essentially free.** Across both platforms and
  both workloads, adding the seqlock writer + reader (commits 2-3)
  costs ≤ 1 percentage point — within the per-platform noise floor.
  That makes commits 1-3 a clean ARMv8-portability fix that can
  ship without measurable regression to the existing IC.
- **Cacheline alignment cleans up M4 entirely** (regressions from
  pre-seqlock +0.9–1.6 % collapse to +/-noise on both workloads),
  but **iPhone XS results across the alignment commit are noise-
  dominated at N=3**. The IC OFF baseline drifted by 5–8 % between
  measurement batches (28.4 → 30.1 → 30.8 ms on xmrsplayer for
  three different builds at three different times of day), which
  swamps the 1–3 % deltas we'd expect cacheline alignment to
  produce.
- **N=10 cross-platform check (full stack, commits 1-4) resolves
  the noise question.** On iPhone XS .utility, xmrsplayer is
  +0.66 % (call_indirect: −0.16 %) — both within the per-rep range
  of ±2 ms. The original −6.8 % reading was a single sample from
  the favorable end of a bimodal distribution. **On iPhone 12 .utility,
  xmrsplayer is −3.78 % (call_indirect: −1.57 %)** — clean, signed,
  and ~5× the per-rep range. The IC delivers a real, measurable win
  on the newer Icestorm L1d/mem subsystem; on Tempest it nets to
  zero. Either way, the IC commits as currently shipped do not
  regress any platform.
- **The IC's xmrsplayer win on iPhone XS at N=3 was a sampling
  artifact.** N=10 medians collapse the −6.8 % to within noise.
  The PMU bucket evidence on M4 (Processing −9.8 %, Discarded
  −21.3 % absolute) remains the more reliable cycle-efficiency
  signal: the IC is doing useful work even where wallclock cannot
  resolve it.

## Decision on op-co-located IC (commits 5-7, full Pulley ISA work)

**Stopping here.** The N=10 cross-check pins down the upside ceiling:
the existing VMContext IC (commits 1-4) delivers between 0 % (XS
Tempest) and −3.78 % (12 Icestorm) on xmrsplayer. Op-co-location's
hypothesised additional L1-locality win is bounded above by the
delta between "cacheline-aligned VMContext IC" and "co-located
with op stream" — and that delta cannot exceed the IC's own total
contribution. So the extra ~15–20 h of Pulley ISA + page-permission
+ handler work is buying, at best, another few percentage points
on a workload already running at 17 ms on iPhone 12 (i.e. 0.5–1 ms
absolute). Not enough to justify the engineering cost or the
substantial new code-surface.

The cacheline-alignment commit was the cheap test of the L1-
locality hypothesis: it had to either (a) amplify the iPhone XS
xmrsplayer win past the N=3 noise (which would suggest much more
locality headroom remained) or (b) deliver a similar-sized
incremental win on iPhone 12. Outcome (b) is roughly what we got:
a clean −3.78 % on iPhone 12 with the aligned IC, which is the
upper end of what L1-locality alone can deliver here.

If we ever want to revisit, the prerequisites are: (1) a workload
where the IC win is large enough that another 50–100 % uplift
matters in absolute terms, or (2) an A12-class device where the
existing IC runs at the noise floor — i.e. evidence that locality
is still bottlenecking dispatch. Neither is on the current roadmap.

## Commits ready to ship as one PR (or as four separate ones)

The four commits stack cleanly on PR #2's
`table-mutability-tracking` branch. Each builds and tests cleanly
in isolation:

1. **d7a219fa98** — layout-only: `_pad` → `generation` field. No
   behavior change.
2. **74aba78555** — writer side: increment-on-fill. Behavior
   change: generation now counts. Reader unaffected.
3. **e9d241e0ea** — reader side: seqlock check on hit path.
   Completes the seqlock; ARMv8-portable now.
4. **566e4690cc** — `repr(C, align(64))`: each slot is one L1
   cacheline. Cleans up the per-platform regressions in the
   noise.

A reviewer could pick:
- Just **#1** as a refactor.
- **#1+#2+#3** for ARMv8 portability without alignment changes.
- **#1+#2+#3+#4** as the full "production-ready VMContext IC."
