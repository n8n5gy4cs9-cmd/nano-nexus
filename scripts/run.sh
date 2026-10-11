#!/usr/bin/env bash
# run.sh: one-command dev startup — ensure the local runtime + default model + deps exist, then
# launch the app. Collapses the README quick start into a single command.
#
# Usage: scripts/run.sh [--web] [--no-setup] [-- <tauri dev args...>]
#   --web       run the frontend only (Vite dev server; no Tauri, runtime, or model needed)
#   --no-setup  skip the runtime/model/dependency checks and launch immediately
#
# Examples:
#   scripts/run.sh                    build missing bits, then `tauri dev`
#   scripts/run.sh --web              UI-only dev server
#   scripts/run.sh -- --release       pass --release through to `tauri dev`
# Portable: bash 3.2 (macOS default). No GNU-only flags.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

WEB=0; SETUP=1
while [ $# -gt 0 ]; do
  case "$1" in
    --web) WEB=1; shift ;;
    --no-setup) SETUP=0; shift ;;
    --) shift; break ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "run.sh: unknown argument: $1 (use -- to pass args to tauri dev)" >&2; exit 2 ;;
  esac
done

step() { printf '\033[1m▶ %s\033[0m\n' "$*"; }

if [ "$SETUP" = "1" ] && [ "$WEB" = "0" ]; then
  # 1) Local runtime (llama-server). `tauri dev` needs the externalBin to exist.
  OS="$(uname -s)"
  if [ "$OS" = "Darwin" ]; then
    RT="vendor/runtime/macos-arm64/llama-server-aarch64-apple-darwin"
    [ -x "$RT" ] || { step "Building local runtime (llama-server)"; scripts/setup-runtime.sh; }
  elif [ "$OS" = "Linux" ]; then
    ls vendor/runtime/linux-*/llama-server-* >/dev/null 2>&1 || { step "Building local runtime (llama-server)"; scripts/setup-runtime.sh --linux; }
  fi

  # 2) Default model. fetch-model hits the network even when up to date, so only run when absent.
  if [ ! -f vendor/models/index.json ]; then
    step "Fetching default model"
    node scripts/fetch-model.mjs
  fi
fi

# 3) Dependencies (either the workspace or app-local node_modules counts).
if [ ! -d apps/desktop/node_modules ] && [ ! -d node_modules ]; then
  step "pnpm install"
  pnpm install
fi

# 4) Launch.
if [ "$WEB" = "1" ]; then
  exec pnpm -C apps/desktop dev:web
else
  exec pnpm -C apps/desktop tauri dev "$@"
fi
