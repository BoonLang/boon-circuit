#!/usr/bin/env bash
# Peak RSS via /usr/bin/time -v for `check` (diagnostics-equivalent) and
# `dump-plan --out` (verified-equivalent), HEAD binary, 3 runs per fixture.
set -euo pipefail
REPO=/home/martinkavik/repos/boon-circuit
RAW=$REPO/docs/plans/compiler_rewrite_notes/review/measurements/raw/baselines
BIN=$REPO/target/release/boon_cli
PLAN=/tmp/claude-1000/-home-martinkavik-repos-boon-circuit/19403ab8-10d9-4589-8161-fdcdd61d4820/scratchpad/measure/plan.bin
declare -A FIX=( [counter]=examples/counter.bn [todomvc]=examples/todo_mvc_physical/RUN.bn [novywave]=examples/novywave/RUN.bn )
cd "$REPO"; mkdir -p "$RAW"
LOG=$RAW/commands_rss.log
{ echo "# $(date -Is) rss series"; echo "uptime_before: $(uptime)"; } >> "$LOG"
for fixture in counter todomvc novywave; do
  src=${FIX[$fixture]}
  for i in 0 1 2; do
    out=$RAW/time_check_${fixture}_${i}.txt
    echo "/usr/bin/time -v $BIN check $src > $out.stdout 2> $out" >> "$LOG"
    /usr/bin/time -v "$BIN" check "$src" > "$out.stdout" 2> "$out" || echo "check exit=$? ($fixture $i)" >> "$LOG"
    out=$RAW/time_dump-plan_${fixture}_${i}.txt
    echo "/usr/bin/time -v $BIN dump-plan $src --out $PLAN > $out.stdout 2> $out" >> "$LOG"
    /usr/bin/time -v "$BIN" dump-plan "$src" --out "$PLAN" > "$out.stdout" 2> "$out" || echo "dump-plan exit=$? ($fixture $i)" >> "$LOG"
    ls -l "$PLAN" >> "$LOG"
  done
done
echo "uptime_after: $(uptime)" >> "$LOG"
