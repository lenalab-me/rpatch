#!/usr/bin/env bash
# Drive rpatch once, the way an agent calls it from Bash, in a throwaway copy of a fixture.
# usage: .agents/skills/verify-rpatch/scripts/drive.sh <name> <fixture-dir> [rpatch args...] < input
#   <name>         short label for the evidence directory
#   <fixture-dir>  files to edit; copied, never touched (fixtures/basic ships with the skill)
#   stdin          the rpatch input (blocks or JSON), exactly as an agent would pass it
# Evidence goes to target/verify-rpatch/<UTC time>-<name>/ and survives cleanup:
#   input.txt args.txt stdout.txt stderr.txt exit_code before/ after/ diff.txt summary.txt
# The scratch copy is deleted on exit. Env: RPATCH_BIN (default target/debug/rpatch),
# BEFORE_RUN (shell snippet run in the work dir right before rpatch, e.g. chmod 444 a.txt).
set -euo pipefail

if [[ $# -lt 2 ]]; then
  sed -n '3,11p' "$0" >&2
  exit 2
fi
name=$1 fixture=$2
shift 2
repo=$(cd "$(dirname "$0")/../../../.." && pwd)
bin=${RPATCH_BIN:-$repo/target/debug/rpatch}
[[ -x "$bin" ]] || { echo "binary missing: $bin (run cargo build)" >&2; exit 2; }
[[ -d "$fixture" ]] || { echo "fixture is not a directory: $fixture" >&2; exit 2; }
fixture=$(cd "$fixture" && pwd)

evidence="$repo/target/verify-rpatch/$(date -u +%Y%m%dT%H%M%SZ)-$name"
scratch=$(mktemp -d)
trap 'chmod -R u+w "$scratch" 2>/dev/null; rm -rf "$scratch"' EXIT
mkdir -p "$evidence"
cp -R "$fixture/." "$scratch/work"
cat > "$evidence/input.txt"
printf '%s\n' "$@" > "$evidence/args.txt"

(cd "$scratch/work" && eval "${BEFORE_RUN:-:}")
cp -Rp "$scratch/work" "$evidence/before"
set +e
(cd "$scratch/work" && "$bin" "$@" < "$evidence/input.txt") > "$evidence/stdout.txt" 2> "$evidence/stderr.txt"
code=$?
set -e
echo "$code" > "$evidence/exit_code"
cp -Rp "$scratch/work" "$evidence/after"
(cd "$evidence" && diff -ru before after) > "$evidence/diff.txt" || true

{
  echo "rpatch:    $("$bin" -V) ($bin)"
  echo "args:      $*"
  echo "exit code: $code"
  echo "changed:   $(grep -c '^diff -ru' "$evidence/diff.txt" || true) file(s)"
  echo "--- stdout"; cat "$evidence/stdout.txt"
  echo "--- stderr"; cat "$evidence/stderr.txt"
  echo "--- diff"; cat "$evidence/diff.txt"
} > "$evidence/summary.txt"
cat "$evidence/summary.txt"
echo "evidence:  $evidence"
