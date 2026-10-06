#!/usr/bin/env bash
# Read-only check: is the rpatch binary under test worth driving?
# usage: .agents/skills/verify-rpatch/scripts/doctor.sh
# Exit 0 when the built binary exists, reports the Cargo.toml version, and is newer
# than every source file; 1 otherwise. Prints what it found either way.
set -uo pipefail

repo=$(cd "$(dirname "$0")/../../../.." && pwd)
bin=${RPATCH_BIN:-$repo/target/debug/rpatch}
ok=0

want=$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/Cargo.toml" | head -1)
echo "repo:      $repo"
echo "binary:    $bin"
if [[ ! -x "$bin" ]]; then
  echo "FAIL: binary missing; run: cargo build (in $repo)"
  exit 1
fi

got=$("$bin" -V 2>&1 | awk '{print $2}')
echo "version:   $got (Cargo.toml: $want)"
[[ "$got" == "$want" ]] || { echo "FAIL: version differs from Cargo.toml; rebuild"; ok=1; }

newest=$(find "$repo/src" "$repo/Cargo.toml" -type f -newer "$bin" | head -3)
if [[ -n "$newest" ]]; then
  echo "FAIL: sources newer than the binary (stale build); run: cargo build"
  while IFS= read -r f; do echo "  $f"; done <<< "$newest"
  ok=1
else
  echo "build:     up to date with src/"
fi

# Informational: the rpatch agents get from PATH may be another build.
if path_bin=$(command -v rpatch); then
  echo "PATH:      $path_bin ($(rpatch -V 2>&1))"
fi
[[ $ok -eq 0 ]] && echo "OK"
exit $ok
