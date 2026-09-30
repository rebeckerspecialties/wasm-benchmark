Memory-dependence experiments on the M4's efficiency cores (watch report, "Memory-order flushes").

- `model-flushes.jsonl`, `model-threshold-and-stalls.jsonl`: `scripts/memdep-bench`, flushes and
  mispredicts per capture (`counts`), cycles and instructions per dispatch, and for the
  stall runs the bottleneck fractions and processing-mode weights.
- `tinywasm-pair-sweep.jsonl`: a tinywasm loop whose 84 statements cycle through D distinct stack
  binops (`d1.wat` and `d21.wat` are the ends of the sweep; D in 1, 2, 4, 8, 12, 16, 21), on #75
  (`m-next75`) and on #75 with our changes (`m-n75stack`); 422 dispatches per loop iteration.
