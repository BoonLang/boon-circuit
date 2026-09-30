#!/usr/bin/env bash
# Cold compiler baselines: one process per observation, HEAD and sep28 binaries
# interleaved (A/B/A) per sample index. Usage: run_baselines.sh <mode> <samples> <fixtures...>
#   mode: fresh-process | empty-session
set -euo pipefail
REPO=/home/martinkavik/repos/boon-circuit
RAW=$REPO/docs/plans/compiler_rewrite_notes/review/measurements/raw/baselines
HEAD_BIN=$REPO/target/release/boon_cli
OLD_BIN=/tmp/claude-1000/-home-martinkavik-repos-boon-circuit/19403ab8-10d9-4589-8161-fdcdd61d4820/scratchpad/bin/boon_cli.sep28
MODE=$1; SAMPLES=$2; shift 2
declare -A FIX=( [counter]=examples/counter.bn [todomvc]=examples/todo_mvc_physical/RUN.bn [novywave]=examples/novywave/RUN.bn )
cd "$REPO"; mkdir -p "$RAW"
LOG=$RAW/commands_${MODE}.log
{ echo "# $(date -Is) mode=$MODE samples=$SAMPLES fixtures=$*"; echo "uptime_before: $(uptime)"; } >> "$LOG"
for fixture in "$@"; do
  src=${FIX[$fixture]}
  for intent in diagnostics verified; do
    for i in $(seq -f '%02g' 0 $((SAMPLES-1))); do
      for lane in head old; do
        if [ "$lane" = head ]; then bin=$HEAD_BIN; else bin=$OLD_BIN; fi
        out=$RAW/${lane}_${fixture}_${intent}_${MODE}_${i}.json
        echo "$bin compiler-sample $src --intent $intent --mode $MODE --samples 1 > $out" >> "$LOG"
        "$bin" compiler-sample "$src" --intent "$intent" --mode "$MODE" --samples 1 > "$out"
      done
    done
  done
done
echo "uptime_after: $(uptime)" >> "$LOG"
echo "done: $(date -Is)" >> "$LOG"
