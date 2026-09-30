#!/usr/bin/env bash
# Warm-path measurement series for the plan review (2026-09-29).
#   1. the xtask benchmark at its smallest sample count (setup 0, scored 1)
#   2. a direct 3+30 warm session on the HEAD product binary
#   3. a direct 3+30 warm session on the sep28 binary the plan probably used
# Runs are sequential (never concurrent) and uptime is recorded before and
# after the series. The direct runs use exactly the arguments and environment
# the xtask uses for its product lane (RAYON_NUM_THREADS=1, no
# BOON_KERNEL_EXPERIMENTAL_PARALLEL), see crates/xtask/src/compiler_interactions.rs
# run_warm_batch. Output goes only under the review measurements directory
# and the scratchpad.
set -u
cd "$(dirname "$0")/../../../../../.." || exit 1
TOOLS=docs/plans/compiler_rewrite_notes/review/measurements/tools
OUT=docs/plans/compiler_rewrite_notes/review/measurements/raw/warm
SEP28=/tmp/claude-1000/-home-martinkavik-repos-boon-circuit/19403ab8-10d9-4589-8161-fdcdd61d4820/scratchpad/bin/boon_cli.sep28
mkdir -p "$OUT"
SERIES_LOG="$OUT/series.log"
{
  echo "series started: $(date '+%F %T %Z')"
  echo "uptime before series: $(uptime)"
} | tee "$SERIES_LOG"

echo "== step 1: xtask verify-compiler-interactions --setup-samples 0 --scored-samples 1" | tee -a "$SERIES_LOG"
"$TOOLS/run_warm.sh" 0 1 | tail -6 | tee -a "$SERIES_LOG"

direct_warm() {
  local label="$1" bin="$2" setup="$3" scored="$4"
  local json="$OUT/warm-session-${label}-${setup}-${scored}.json"
  local log="$OUT/warm-session-${label}-${setup}-${scored}.log"
  {
    echo "label: $label"
    echo "binary: $bin ($(sha256sum "$bin" | cut -c1-16)…)"
    echo "started: $(date '+%F %T %Z')"
    echo "uptime before: $(uptime)"
    echo "cmd: env -u BOON_KERNEL_EXPERIMENTAL_PARALLEL RAYON_NUM_THREADS=1 $bin compiler-sample warm-session examples/todo_mvc_physical/RUN.bn --switch-source examples/counter.bn --edit-unit examples/todo_mvc_physical/RUN.bn --edit-from 'TEXT { Walk the dog }' --edit-to 'TEXT { Walk the dogs }' --setup-samples $setup --scored-samples $scored > $json"
    /usr/bin/time -f 'warm-session wall=%es user=%Us sys=%Ss maxrss=%MKiB' \
      env -u BOON_KERNEL_EXPERIMENTAL_PARALLEL RAYON_NUM_THREADS=1 \
      timeout 900 "$bin" compiler-sample warm-session examples/todo_mvc_physical/RUN.bn \
        --switch-source examples/counter.bn \
        --edit-unit examples/todo_mvc_physical/RUN.bn \
        --edit-from 'TEXT { Walk the dog }' --edit-to 'TEXT { Walk the dogs }' \
        --setup-samples "$setup" --scored-samples "$scored" > "$json"
    echo "exit=$?"
    echo "finished: $(date '+%F %T %Z')"
    echo "uptime after: $(uptime)"
  } > "$log" 2>&1
  cat "$log" | tee -a "$SERIES_LOG"
}

echo "== step 2: direct warm-session HEAD 3+30" | tee -a "$SERIES_LOG"
direct_warm head target/release/boon_cli 3 30
echo "== step 3: direct warm-session sep28 3+30" | tee -a "$SERIES_LOG"
direct_warm sep28 "$SEP28" 3 30

{
  echo "uptime after series: $(uptime)"
  echo "series finished: $(date '+%F %T %Z')"
} | tee -a "$SERIES_LOG"
