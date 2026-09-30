#!/usr/bin/env bash
# Run the warm compiler-interactions benchmark once, time-boxed to 25 minutes,
# with uptime recorded before and after. Run from anywhere; it cds to the repo.
#
# The report and the log are written to the scratchpad first and copied into
# the review directory afterwards: the xtask re-validates the workspace
# identity after writing the report, and that identity hashes the contents of
# every untracked file, so a report written straight into the (untracked)
# review directory makes the tool report "identity is stale" after a
# successful collection (observed 2026-09-29 23:53:57, exit 1, report intact).
#
# Usage: tools/run_warm.sh [setup] [scored]
set -u
cd "$(dirname "$0")/../../../../../.." || exit 1
SETUP="${1:-0}"
SCORED="${2:-1}"
OUT=docs/plans/compiler_rewrite_notes/review/measurements/raw/warm
SCRATCH=/tmp/claude-1000/-home-martinkavik-repos-boon-circuit/19403ab8-10d9-4589-8161-fdcdd61d4820/scratchpad/measure/warm
mkdir -p "$OUT" "$SCRATCH"
NAME="compiler-interactions-setup${SETUP}-scored${SCORED}"
REPORT="$SCRATCH/$NAME.json"
LOG="$SCRATCH/run-setup${SETUP}-scored${SCORED}.log"
{
  echo "cwd: $(pwd)"
  echo "started: $(date '+%F %T %Z')"
  echo "uptime before: $(uptime)"
  echo "producers:"; sha256sum target/release/boon_cli target/release/boon_cli_evidence
  echo "xtask: $(ls -la --time-style=full-iso target/debug/xtask)"
  echo "cmd: timeout 1500 cargo xtask verify-compiler-interactions --setup-samples $SETUP --scored-samples $SCORED --report $REPORT"
  /usr/bin/time -f 'benchmark wall=%es user=%Us sys=%Ss maxrss=%MKiB' \
    timeout 1500 cargo xtask verify-compiler-interactions \
      --setup-samples "$SETUP" --scored-samples "$SCORED" --report "$REPORT"
  echo "exit=$?"
  echo "finished: $(date '+%F %T %Z')"
  echo "uptime after: $(uptime)"
} > "$LOG" 2>&1
cat "$LOG"
cp -f "$REPORT" "$OUT/$NAME.json" 2>/dev/null
cp -f "$LOG" "$OUT/run-setup${SETUP}-scored${SCORED}.log"
