# memdep-bench

Tests whether Apple cores' memory-dependence predictor runs out of room when every
interpreter handler inlines its own copy of the value-stack code (watch report,
"Memory-order flushes").

`gen.py` writes `src/main.rs`: 512 copies each of three handler kinds, built like
tinywasm's nightly tail-call dispatch (`become` through a table, a `Vec` stack
reached through an executor and a store):

- `u`: update the stack top in place (tinywasm's `BinOpStackConst32`)
- `p`: push a constant / pop two and push one (`Const32`, `I32Add`)
- `c`: control, the same update on a slot of its own, so consecutive handlers
  never alias

Each copy adds its own constant, so LLVM cannot merge the copies.
`mdp <u|p|c> <k> <cyclic|random|blockN> <dispatches> [runs]` dispatches over `k` copies
in the given order and prints instructions and cycles per dispatch.

    python3 gen.py && rustup run nightly-2026-07-05 cargo build --release
    WORK=/tmp/memdep ./measure.sh          # flushes and mispredicts per dispatch
    WORK=/tmp/memdep ./measure-stalls.sh   # adds the bottleneck and processing modes

The measure scripts run each config on the E-cores (`taskpolicy -b`), attach a 3 s
CPU Counters capture, and reduce it with `scripts/pmu_summarize.py`. They need the
recording-option files `opts-m4-ds.json`, `opts-m4-bottlenecks.json` and
`opts-m4-processing.json` in `$WORK` (see `scripts/run-m4-pmu-pass.sh` for how to make them).
