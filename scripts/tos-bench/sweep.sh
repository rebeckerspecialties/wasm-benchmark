#!/usr/bin/env bash
# Timing sweep on the E-cores: every variant at every copy count, interleaved, REPS times.
# Prints one line per run to stdout (and $OUT if set).
set -u
B="$(cd "$(dirname "$0")" && pwd)/target/release/tosb"
REPS=${REPS:-3}; DISPATCHES=${DISPATCHES:-100000000}
VARIANTS=${VARIANTS:-"vec sp sp_wt sp_wb tos_flag vec_tos sp_spill sp_wt_pn"}
MS=${MS:-"1 2 4 8 16 32 64"}
for rep in $(seq 1 $REPS); do
  for m in $MS; do
    for v in $VARIANTS; do
      iters=$(( DISPATCHES / 277 ))
      line=$(taskpolicy -b "$B" $v $m $iters 1)
      echo "rep=$rep $line" | tee -a "${OUT:-/dev/null}"
    done
  done
done
