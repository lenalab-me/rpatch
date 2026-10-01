#!/usr/bin/env bash
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
export ENGINE=codex
exec "$here/compare.sh" "$@"
