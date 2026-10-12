#!/usr/bin/env bash
# build-windows.sh: Windows installer, portable ZIP or offline kit, cross-compiled on the Mac.
#
# Usage: scripts/build-windows.sh --profile <name|path> [--arch x64|arm64] [--format installer|portable|kit|auto]
#                                [--skip-doctor] [--skip-fetch] [--dry-run]
# See scripts/build-windows.mjs for what each format contains. Needs Node 22+.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
case "${1:-}" in -h|--help) sed -n '2,6p' "$0" ;; esac
exec node "$ROOT/scripts/build-windows.mjs" "$@"
