#!/usr/bin/env bash
# PMU on the E-cores: run tosb for ~6 s under taskpolicy -b, attach a 3 s CPU Counters capture in
# a guided mode, reduce it with scripts/pmu_summarize.py, append a JSON line to $WORK/pmu.jsonl.
#   WORK=<dir with opts-m4-ds.json, opts-m4-bottlenecks.json, opts-m4-processing.json> ./pmu.sh [configs...]
# A config is mode:variant:copies (mode: ds | bottlenecks | processing).
S="${WORK:?set WORK to a directory holding opts-m4-*.json}"
HERE="$(cd "$(dirname "$0")" && pwd)"; B=$HERE/target/release/tosb; OUT=$S/pmu.jsonl
KT="$(getconf DARWIN_USER_TEMP_DIR)"
free_gb() { df -g /System/Volumes/Data | awk 'NR==2 {print $4}'; }
# Other tools' xctrace runs also leave kernel traces behind: before each capture, delete the ones no
# process holds, and wait while the disk is still short.
make_room() {
  local f
  for f in "$KT"/instruments*.ktrace; do
    [ -e "$f" ] && [ -z "$(lsof -t "$f" 2>/dev/null)" ] && rm -f "$f"
  done
  local waited=0
  while [ "$(free_gb)" -lt "${MIN_FREE_GB:-4}" ] && [ $waited -lt 600 ]; do sleep 10; waited=$((waited + 10)); done
  [ "$(free_gb)" -ge "${MIN_FREE_GB:-4}" ]
}
run() { # mode variant m
  local mode=$1 v=$2 m=$3
  make_room || { echo "skip $mode $v $m: disk full"; return; }
  local gmode=bottleneck:$([ $mode = ds ] && echo discarded_sampling || echo $mode)
  local ns=$(taskpolicy -b $B $v $m 100000 1 | sed -nE 's/.*: ([0-9.]+) ns\/dispatch.*/\1/p')
  local iters=$(python3 -c "print(max(1, int(6.5 / ($ns * 1e-9) / 277)))")
  taskpolicy -b $B $v $m $iters 1 > $S/run.log 2>&1 & local pid=$!
  sleep 1.5; rm -rf $S/cap.trace
  local before=$(ls "$KT"/instruments*.ktrace 2>/dev/null)
  xcrun xctrace record --template "CPU Counters" --recording-options $S/opts-m4-$mode.json --attach $pid --time-limit 3000ms --no-prompt --output $S/cap.trace > $S/xctrace.log 2>&1
  wait $pid
  xcrun xctrace export --input $S/cap.trace --xpath '//trace-toc/run/data/table[@schema="MetricAggregationForThread"]' --output $S/cap.xml > /dev/null 2>&1
  local line=$(tail -1 $S/run.log)
  python3 "$HERE/../pmu_summarize.py" $S/cap.xml tosb $gmode | python3 -c 'import json,sys,re
rows=[json.loads(l) for l in sys.stdin]
mode,v,m,line=sys.argv[1:5]
if not rows: print(json.dumps({"mode":mode,"variant":v,"m":int(m),"error":"no capture"})); sys.exit()
best=max(rows, key=lambda d: d["cycles"])
x=re.search(r"([0-9.]+) instr/dispatch, ([0-9.]+) cycles/dispatch, (\d+) handlers, (\d+) pairs", line)
print(json.dumps({"mode":mode,"variant":v,"m":int(m),"handlers":int(x.group(3)),"pairs":int(x.group(4)),"instr_per_dispatch":float(x.group(1)),"cycles_per_dispatch":float(x.group(2)),"cap_cycles":best["cycles"],"counts":best["counts"],"fractions":best["fractions"],"weights":best["weights"]}))' $mode $v $m "$line" >> $OUT
  rm -rf $S/cap.trace $S/cap.xml
  # xctrace leaves its raw kernel trace behind: delete this capture's, never one another process holds
  for f in "$KT"/instruments*.ktrace; do
    [ -e "$f" ] || continue
    echo "$before" | grep -qxF "$f" && continue
    [ -z "$(lsof -t "$f" 2>/dev/null)" ] && rm -f "$f"
  done
  echo "done $mode $v $m: $(tail -1 $OUT | cut -c1-160)"
}
for cfg in "$@"; do IFS=: read -r mode v m <<< "$cfg"; run $mode $v $m; done
