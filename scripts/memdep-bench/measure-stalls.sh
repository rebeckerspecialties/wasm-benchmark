#!/usr/bin/env bash
S="${WORK:?set WORK to a scratch directory holding opts-m4-*.json}"
B="$(cd "$(dirname "$0")" && pwd)/target/release/mdp"; OUT=$S/results2.jsonl
KT="$(getconf DARWIN_USER_TEMP_DIR)"
: > $OUT
run() { # mode kernel k order
  local mode=$1 kern=$2 k=$3 order=$4
  local opts=$S/opts-m4-${mode#*:}.json; [ "$mode" = bottleneck:discarded_sampling ] && opts=$S/opts-m4-ds.json
  local ns=$(taskpolicy -b $B $kern $k $order 20000000 | sed -nE 's/.*: ([0-9.]+) ns\/dispatch.*/\1/p')
  local runs=$(python3 -c "import math;print(max(1,math.ceil(6.0/(1e8*$ns*1e-9))))")
  taskpolicy -b $B $kern $k $order 100000000 $runs > $S/run.log 2>&1 & local pid=$!
  sleep 1.5; rm -rf $S/cap.trace
  xcrun xctrace record --template "CPU Counters" --recording-options $opts --attach $pid --time-limit 3000ms --no-prompt --output $S/cap.trace > /dev/null 2>&1
  wait $pid
  xcrun xctrace export --input $S/cap.trace --xpath '//trace-toc/run/data/table[@schema="MetricAggregationForThread"]' --output $S/cap.xml > /dev/null 2>&1
  local line=$(tail -1 $S/run.log)
  python3 "$(cd "$(dirname "$0")/.." && pwd)/pmu_summarize.py" $S/cap.xml mdp $mode | python3 -c 'import json,sys,re
best=max((json.loads(l) for l in sys.stdin), key=lambda d: d["cycles"])
mode,kern,k,order,line=sys.argv[1:6]
m=re.search(r"([0-9.]+) instr/dispatch, ([0-9.]+) cycles/dispatch", line)
print(json.dumps({"mode":mode,"kernel":kern,"k":int(k),"order":order,"instr_per_dispatch":float(m.group(1)),"cycles_per_dispatch":float(m.group(2)),"cap_cycles":best["cycles"],"counts":best["counts"],"fractions":best["fractions"],"weights":best["weights"]}))' $mode $kern $k $order "$line" >> $OUT
  rm -rf $S/cap.trace $S/cap.xml
  # xctrace leaves its kernel trace behind; leave the ones another process still holds
  for f in "$KT"/instruments*.ktrace; do [ -e "$f" ] && [ -z "$(lsof -t "$f" 2>/dev/null)" ] && rm -f "$f"; done
  echo "done $mode $kern $k $order: $line"
}
for k in 16 20 24 28 32 40; do run bottleneck:discarded_sampling u $k cyclic; done
for o in block8 block64; do run bottleneck:discarded_sampling u 64 $o; done
for cfg in "u 8 cyclic" "u 64 cyclic" "c 64 cyclic"; do run bottleneck:bottlenecks $cfg; run bottleneck:processing $cfg; done
echo "[all done] $(date +%H:%M:%S)"
