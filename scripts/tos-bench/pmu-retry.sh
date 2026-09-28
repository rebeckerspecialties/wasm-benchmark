#!/usr/bin/env bash
# Runs pmu.sh once per config and retries a config whose capture came back empty (another
# tool's recording can make xctrace attach fail), up to three attempts.
#   WORK=... ./pmu-retry.sh <configs...>
S="${WORK:?set WORK}"; HERE="$(cd "$(dirname "$0")" && pwd)"
for cfg in "$@"; do
  for attempt in 1 2 3; do
    "$HERE/pmu.sh" "$cfg"
    if tail -1 "$S/pmu.jsonl" | grep -q '"error"'; then
      sed -i '' '$d' "$S/pmu.jsonl"
      echo "retry $cfg (attempt $attempt failed)"; sleep 30
    else
      break
    fi
  done
done
