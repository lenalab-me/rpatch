#!/usr/bin/env bash
# Paired CLI/MCP runs in Codex or Claude Code. Never requires an editor.
# Usage: bench/compare.sh [new-output-dir] [seed-dir] [prompt-dir]
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/.." && pwd)
output=${1:-$here/out/compare-$(date +%Y%m%d-%H%M%S)}
seed=${2:-$here/fixtures/editor-sample}
prompts=${3:-$here/compare-prompts}
binary=${RPATCH_BIN:-$repo/target/release/rpatch}
engine=${ENGINE:-claude}
case "$engine" in
  codex) default_model=gpt-6.1-sol ;;
  claude) default_model=claude-sonnet-5-5 ;;
  *) echo "unknown ENGINE: $engine" >&2; exit 1 ;;
esac
model=${MODEL:-$default_model}
trials=${TRIALS:-1}
limit=${LIMIT:-0}
seconds=${TIMEOUT_SECONDS:-300}
budget=${MAX_BUDGET_USD:-1}
printf -v default_verify 'node %q' "$here/verify-sample.mjs"
verify=${VERIFY_COMMAND:-$default_verify}

for dependency in "$engine" jq python3 node tar timeout; do
  command -v "$dependency" >/dev/null || { echo "missing: $dependency" >&2; exit 1; }
done
[[ "$trials" =~ ^[1-9][0-9]*$ && "$limit" =~ ^[0-9]+$ && "$seconds" =~ ^[1-9][0-9]*$ ]] || {
  echo 'TRIALS and TIMEOUT_SECONDS must be positive integers; LIMIT must be nonnegative' >&2; exit 1;
}
[[ -d "$seed" && -d "$prompts" && -x "$binary" ]] || {
  echo 'seed/prompt directories and release binary are required' >&2; exit 1;
}
[[ ! -e "$output" ]] || { echo "output already exists: $output" >&2; exit 1; }
seed=$(cd "$seed" && pwd)
prompts=$(cd "$prompts" && pwd)
binary=$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")
mkdir -p "$output"
output=$(cd "$output" && pwd)

# Remove installed rpatch directories from PATH; only the CLI arm adds its own.
# The MCP server uses an absolute command, independent of the agent's PATH.
IFS=: read -r -a path_parts <<< "$PATH"
base_path=''
for part in "${path_parts[@]}"; do
  [[ -n "$part" && ! -x "$part/rpatch" && "$part" != */codex-path ]] || continue
  base_path=${base_path:+$base_path:}$part
done
[[ -n "$base_path" ]] || { echo 'empty benchmark PATH' >&2; exit 1; }
mkdir -p "$output/cli-bin"
ln -s "$binary" "$output/cli-bin/rpatch"
"$binary" --version > "$output/rpatch-version.txt"
"$engine" --version > "$output/$engine-version.txt"
isolation=()
if [[ "$engine" == codex ]]; then
  python3 "$here/codex_overrides.py" > "$output/codex-overrides.txt"
  while IFS= read -r override; do isolation+=(-c "$override"); done < "$output/codex-overrides.txt"
fi
jq -n --arg engine "$engine" --arg model "$model" --arg seed "$seed" --arg prompts "$prompts" \
  --arg binary "$binary" --arg verify "$verify" --arg budget "$budget" \
  --argjson trials "$trials" --argjson limit "$limit" --argjson timeout "$seconds" \
  '{engine:$engine,model:$model,seed:$seed,prompts:$prompts,binary:$binary,verify:$verify,budget_per_prompt_usd:(if $engine=="claude" then $budget else null end),trials:$trials,limit:$limit,timeout_seconds:$timeout}' \
  > "$output/experiment.json"
# Save the actual inputs, so a later change to a source fixture cannot change the run.
tar --exclude=.git -cf "$output/seed.tar" -C "$seed" .
cp -R "$prompts" "$output/prompts"

for ((trial=1; trial<=trials; trial++)); do
  arms=(cli mcp)
  if ((trial % 2 == 0)); then arms=(mcp cli); fi
  for arm in "${arms[@]}"; do
    run="$output/trial-$trial/$arm"
    work="$run/work"
    mkdir -p "$work" "$run/shell-config"
    tar -xf "$output/seed.tar" -C "$work"
    git -C "$work" init -q
    # Avoid global identity/hooks/signing settings in disposable repositories.
    git -C "$work" config user.name 'rpatch experiment'
    git -C "$work" config user.email 'experiment@example.invalid'
    git -C "$work" config core.hooksPath /dev/null
    git -C "$work" config commit.gpgsign false
    git -C "$work" add .
    git -C "$work" commit -qm 'Identical experiment seed'
    context=''
    run_path=$base_path
    if [[ "$arm" == cli ]]; then
      run_path="$output/cli-bin:$base_path"
      context='Additional available CLI: rpatch, an exact-match batch text editor. Its input format and options are documented by rpatch --help.'
      printf '{"mcpServers":{}}\n' > "$run/mcp.json"
    else
      jq -n --arg command "$binary" --arg cwd "$work" \
        '{mcpServers:{rpatch:{type:"stdio",command:$command,args:["mcp","--cwd",$cwd]}}}' > "$run/mcp.json"
    fi
    n=0
    for prompt in "$output/prompts"/*.md; do
      [[ -f "$prompt" ]] || { echo 'no prompt files' >&2; exit 1; }
      n=$((n+1))
      if ((limit > 0 && n > limit)); then break; fi
      name=$(basename "$prompt" .md)
      echo "trial=$trial arm=$arm prompt=$name"
      start=$(date +%s)
      code=0
      if [[ "$engine" == codex ]]; then
        mcp_override='mcp_servers.rpatch={command="rpatch-disabled",enabled=false}'
        if [[ "$arm" == mcp ]]; then
          mcp_override=$(jq -nr --arg command "$binary" --arg cwd "$work" \
            '"mcp_servers.rpatch={command=\($command|tojson),args=[\"mcp\"],cwd=\($cwd|tojson),enabled=true,required=true}"')
        fi
        context_override=$(jq -nr --arg context "$context" '"developer_instructions=\($context|tojson)"')
        path_override=$(jq -nr --arg path "$run_path" --arg zdotdir "$run/shell-config" \
          '"shell_environment_policy.set={PATH=\($path|tojson),ZDOTDIR=\($zdotdir|tojson)}"')
        (cd "$work" && env PATH="$run_path" timeout "$seconds" codex exec \
          --ignore-user-config --ignore-rules --ephemeral --json --skip-git-repo-check \
          --model "$model" --sandbox danger-full-access \
          -c 'approval_policy="never"' -c 'project_doc_max_bytes=0' \
          -c 'features.apps=false' -c 'features.hooks=false' -c 'features.memories=false' \
          -c 'features.multi_agent=false' -c 'web_search="disabled"' \
          -c 'features.shell_snapshot=false' -c 'allow_login_shell=false' \
          -c 'shell_environment_policy.experimental_use_profile=false' \
          "${isolation[@]}" -c "$mcp_override" -c "$context_override" -c "$path_override" \
          - < "$prompt") > "$run/$name.jsonl" 2> "$run/$name.err" || code=$?
      else
        (cd "$work" && env PATH="$run_path" CLAUDE_CODE_DISABLE_CLAUDE_MDS=1 ENABLE_TOOL_SEARCH=false \
          timeout "$seconds" claude -p "$(cat "$prompt")" --model "$model" \
          --permission-mode bypassPermissions --output-format stream-json --verbose \
          --setting-sources '' --settings '{"disableClaudeAiConnectors":true}' \
          --disable-slash-commands --strict-mcp-config --mcp-config "$run/mcp.json" \
          --no-session-persistence --max-budget-usd "$budget" \
          --append-system-prompt "$context") \
          > "$run/$name.jsonl" 2> "$run/$name.err" || code=$?
      fi
      elapsed=$(( $(date +%s) - start ))
      verify_code=0
      (cd "$work" && RPATCH_BENCH_CASE="$name" timeout "$seconds" bash -c "$verify") \
        > "$run/$name.verify.log" 2>&1 || verify_code=$?
      git -C "$work" diff HEAD > "$run/$name.uncommitted.diff"
      git -C "$work" log --oneline > "$run/$name.commits.txt"
      jq -n --arg engine "$engine" --arg arm "$arm" \
        --argjson code "$code" --argjson elapsed "$elapsed" --argjson verify_code "$verify_code" \
        '{engine:$engine,required_mcp:($engine=="codex" and $arm=="mcp"),runner_exit_code:$code,elapsed_seconds:$elapsed,verify_exit_code:$verify_code}' > "$run/$name.run.json"
      # Stop the sequence if the run failed, rather than comparing divergent tasks.
      if ((code != 0 || verify_code != 0)); then break; fi
    done
  done
done
python3 "$here/compare_metrics.py" "$output" > "$output/summary.json"
echo "results: $output/summary.json"
if ! jq -e '.paired_comparison.cli.paired_eligible_runs > 0' "$output/summary.json" >/dev/null; then
  echo 'no valid paired runs; inspect the saved traces for API, connection, or verification failures' >&2
  exit 1
fi
