# Archived IC-branch tip SHAs

The one-entry call_indirect IC investigation lives on two branches in
`wasmtime/`. These branches were deleted after the
`vtable_dispatch.wasm` PMU evidence (see PATH-A-RESULTS.md +
this directory's PMU traces) showed the IC's back-end savings are
precisely cancelled by added front-end / mispredict pressure on
Apple Silicon E-cores — making the IC wallclock-neutral at best and
regression-prone on bimodal sites.

To recover either branch (e.g., as a starting point for a future
2-way IC or poisoning-extended variant):

```
cd wasmtime
git fetch fork  # if fork copy still exists, otherwise use the SHA directly
git checkout -b ic-archived 566e4690cc2a18d312033113901dadf853124c82  # seqlock variant
# or
git checkout -b ic-archived 9cf9f8e8a659c6a1184685da69847e36f32f6ae6  # no-seqlock variant
```

| branch | tip SHA | shape |
|---|---|---|
| `pulley-call-indirect-ic` | `566e4690cc2a18d312033113901dadf853124c82` | seqlock + cacheline-aligned VMContext IC, on top of `table-mutability-tracking` |
| `pulley-call-indirect-ic-noseqlock` | `9cf9f8e8a659c6a1184685da69847e36f32f6ae6` | seqlock removed (per Chris Fallin's correct observation that `&mut Store` makes vmctx IC single-threaded), cacheline-aligned, on top of `table-mutability-tracking` |

PMU evidence from the no-seqlock variant on `vtable_dispatch` is the
load-bearing closeout — see `pmu-n2/iphone12-vt-{off,on}.xml` and
`PATH-A-RESULTS.md`.
