#!/usr/bin/env bash
# e2e.sh: browser end-to-end tests of LAN Share (needs Playwright's Chromium; see GOTCHAS).
#   builds the web UI (embedded in the server), builds the fake-backed share server, runs scripts/e2e/*.mjs
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
[ "$(uname -s)" = "Darwin" ] || export TAURI_CONFIG="${TAURI_CONFIG:-{\"app\":{\"macOSPrivateApi\":true}}}"
echo "▶ web UI"; pnpm -C apps/desktop build:web >/dev/null
echo "▶ share server (fake backend)"; cargo build -q -p nexus-share --example e2e_server
echo "▶ browser tests"; node --test scripts/e2e/*.mjs
