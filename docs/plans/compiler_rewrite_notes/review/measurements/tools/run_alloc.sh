#!/usr/bin/env bash
# Allocation evidence via target/release/boon_cli_evidence (protocol evidence_producer),
# fresh-process, one process per observation.
set -euo pipefail
REPO=/home/martinkavik/repos/boon-circuit
RAW=$REPO/docs/plans/compiler_rewrite_notes/review/measurements/raw/baselines
BIN=$REPO/target/release/boon_cli_evidence
declare -A FIX=( [counter]=examples/counter.bn [todomvc]=examples/todo_mvc_physical/RUN.bn [novywave]=examples/novywave/RUN.bn )
cd "$REPO"; mkdir -p "$RAW"
LOG=$RAW/commands_alloc.log
{ echo "# $(date -Is) allocation evidence series"; echo "uptime_before: $(uptime)"; } >> "$LOG"
for fixture in counter todomvc novywave; do
  src=${FIX[$fixture]}
  for intent in diagnostics verified; do
    for i in 0 1 2; do
      out=$RAW/evidence_${fixture}_${intent}_fresh-process_${i}.json
      echo "$BIN compiler-sample $src --intent $intent --mode fresh-process --samples 1 > $out" >> "$LOG"
      "$BIN" compiler-sample "$src" --intent "$intent" --mode fresh-process --samples 1 > "$out"
    done
  done
done
echo "uptime_after: $(uptime)" >> "$LOG"
