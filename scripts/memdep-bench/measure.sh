#!/usr/bin/env bash
# For each config: run mdp on the E-cores for ~6 s, attach a 3 s discarded_sampling capture, reduce it.
S="${WORK:?set WORK to a scratch directory holding opts-m4-*.json}"
B="$(cd "$(dirname "$0")" && pwd)/target/release/mdp"; OUT=$S/results.jsonl; OPTS=$S/opts-m4-ds.json
KT="$(getconf DARWIN_USER_TEMP_DIR)"
: > $OUT
run() { # kernel k order
  local kern=$1 k=$2 order=$3
  # calibrate: ns per dispatch over 2e7
  local ns=$(taskpolicy -b $B $kern $k $order 20000000 | sed -nE 's/.*: ([0-9.]+) ns\/dispatch.*/\1/p')
  local runs=$(python3 -c "import math;print(max(1,math.ceil(6.0/(1e8*$ns*1e-9))))")
  taskpolicy -b $B $kern $k $order 100000000 $runs > $S/run.log 2>&1 & local pid=$!
  sleep 1.5
  rm -rf $S/cap.trace
  xcrun xctrace record --template "CPU Counters" --recording-options $OPTS --attach $pid --time-limit 3000ms --no-prompt --output $S/cap.trace > /dev/null 2>&1
  wait $pid
  xcrun xctrace export --input $S/cap.trace --xpath '//trace-toc/run/data/table[@schema="MetricAggregationForThread"]' --output $S/cap.xml > /dev/null 2>&1
  local line=$(tail -1 $S/run.log)
  python3 "$(cd "$(dirname "$0")/.." && pwd)/pmu_summarize.py" $S/cap.xml mdp bottleneck:discarded_sampling \
    | python3 -c 'import json,sys
best=max((json.loads(l) for l in sys.stdin), key=lambda d: d["cycles"])
kern,k,order,line=sys.argv[1:5]
import re
m=re.search(r"([0-9.]+) instr/dispatch, ([0-9.]+) cycles/dispatch", line)
print(json.dumps({"kernel":kern,"k":int(k),"order":order,"instr_per_dispatch":float(m.group(1)),"cycles_per_dispatch":float(m.group(2)),"cap_cycles":best["cycles"],"counts":best["counts"]}))' $kern $k $order "$line" >> $OUT
  rm -rf $S/cap.trace $S/cap.xml "$KT"/instruments*.ktrace
  echo "done $kern $k $order: $line"
}
for k in 1 2 4 8 16 32 64 128 256 512; do run u $k cyclic; done
for k in 2 8 64 512; do run u $k random; done
for k in 1 4 16 64 256; do run p $k cyclic; done
for k in 1 64 512; do run c $k cyclic; done
run c 64 random
echo "[all done] $(date +%H:%M:%S)"
