#!/usr/bin/env bash
# eval-assistants.sh: golden-set evaluation of the chat assistants against the default (or --endpoint) model.
# Usage: scripts/eval-assistants.sh [--endpoint URL] [--assistant ID] [--no-fail] [--json FILE]
# Needs the model runtime (macOS: scripts/setup-runtime.sh; model: node scripts/fetch-model.mjs). See docs/SPIKES.md §evals.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec node "$ROOT/scripts/eval-assistants.mjs" "$@"
