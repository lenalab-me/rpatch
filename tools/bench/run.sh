#!/usr/bin/env bash
# Runs tools/bench/prompts/*.md in order, each in a fresh headless Claude Code session
# (bypass mode), in a work directory that must not exist yet. The prompts never
# mention editing tools: the point is to see what the agent picks on its own.
#
# usage: tools/bench/run.sh [work-dir]     (default: ~/project/lab/tasklog)
#        MODEL=claude-sonnet-5-5 by default
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
work=${1:-$HOME/project/lab/tasklog}
model=${MODEL:-claude-sonnet-5-5}
out="$here/out/$(date +%Y%m%d-%H%M%S)"

if [[ -e "$work" ]]; then
  echo "work dir already exists: $work" >&2
  exit 1
fi
mkdir -p "$work" "$out"
echo "work=$work model=$model out=$out" | tee "$out/progress.log"

for prompt in "$here"/prompts/*.md; do
  name=$(basename "$prompt" .md)
  echo "[$(date +%H:%M:%S)] $name" | tee -a "$out/progress.log"
  if ! (cd "$work" && timeout 1800 claude -p "$(cat "$prompt")" \
      --model "$model" --permission-mode bypassPermissions --output-format json) \
      > "$out/$name.json" 2> "$out/$name.err"; then
    echo "  exited with an error" | tee -a "$out/progress.log"
  fi
  jq -r '"  cost=\(.total_cost_usd) turns=\(.num_turns) is_error=\(.is_error)"' \
    "$out/$name.json" 2>/dev/null | tee -a "$out/progress.log" || true
done
echo "[$(date +%H:%M:%S)] done" | tee -a "$out/progress.log"
