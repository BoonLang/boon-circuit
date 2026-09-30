#!/usr/bin/env bash
# Runs the standalone parser prototype (docs/plans/compiler_rewrite_notes/drafts/fe_proto.rs,
# compiled in the scratchpad) RUNS times per file set and appends every process's full
# stdout to raw/proto/bench_<tag>_<set>.txt, one "### run N: <command>" header per run.
#
# Usage: proto_run.sh <tag> <binary>
#   tag     orig  -> unmodified prototype (prints best-of-30 per process)
#           iter  -> patched copy that also prints every iteration ("iter lex_ms=.. all_ms=..")
#   binary  absolute path of the compiled prototype
# Env: RUNS (default 20), SETS (default: all seven sets).
set -euo pipefail
REPO=/home/martinkavik/repos/boon-circuit
RAW=$REPO/docs/plans/compiler_rewrite_notes/review/measurements/raw/proto
TAG=${1:?tag}
BIN=${2:?binary}
RUNS=${RUNS:-20}
SETS=${SETS:-"counter todo_run todo novywave_run novywave examples all_tracked"}
cd "$REPO"
uptime | tee "$RAW/uptime_${TAG}_before.txt"
for set in $SETS; do
  mapfile -t files < "$RAW/files_$set.txt"
  out="$RAW/bench_${TAG}_${set}.txt"
  : > "$out"
  for i in $(seq 1 "$RUNS"); do
    echo "### run $i: $BIN bench ${files[*]}" >> "$out"
    "$BIN" bench "${files[@]}" >> "$out"
  done
  echo "$set: $RUNS runs -> $out"
done
uptime | tee "$RAW/uptime_${TAG}_after.txt"
