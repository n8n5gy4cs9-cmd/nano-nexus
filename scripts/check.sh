#!/usr/bin/env bash
# check.sh: the full local verification suite (the Definition of Done in docs/PLAN.md §4).
# Skips the parts of the project that don't exist yet, so it's useful from day one.
#
# Usage: scripts/check.sh [--no-rust] [--no-frontend] [--fast]
#   --fast   skip clippy and slow integration tests (fmt + unit tests only)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

RUST=1; FRONTEND=1; FAST=0
for arg in "$@"; do
  case "$arg" in
    --no-rust) RUST=0 ;;
    --no-frontend) FRONTEND=0 ;;
    --fast) FAST=1 ;;
    -h|--help) sed -n '2,7p' "$0"; exit 0 ;;
    *) echo "check.sh: unknown argument: $arg" >&2; exit 2 ;;
  esac
done

step() { printf '\n\033[1m▶ %s\033[0m\n' "$*"; }
skip() { printf '\033[2m  (skipped: %s)\033[0m\n' "$*"; }

# --- Rust ----------------------------------------------------------------------------------
if [ "$RUST" = "1" ]; then
  if [ -f Cargo.toml ]; then
    step "cargo fmt --check"
    cargo fmt --all -- --check
    if [ "$FAST" = "0" ]; then
      step "cargo clippy (-D warnings)"
      cargo clippy --workspace --all-targets -- -D warnings
      step "cargo test"
      cargo test --workspace
    else
      step "cargo test --lib (fast)"
      cargo test --workspace --lib
    fi
  else
    step "Rust"; skip "no Cargo workspace yet (T00.01)"
  fi
fi

# --- Frontend --------------------------------------------------------------------------------
if [ "$FRONTEND" = "1" ]; then
  if [ -f apps/desktop/package.json ]; then
    if [ ! -d apps/desktop/node_modules ] && [ ! -d node_modules ]; then
      step "pnpm install"
      pnpm install
    fi
    step "eslint";    pnpm -C apps/desktop lint
    step "typecheck"; pnpm -C apps/desktop typecheck
    if [ "$FAST" = "0" ]; then
      step "vitest";  pnpm -C apps/desktop test --run
    fi
  else
    step "Frontend"; skip "no apps/desktop yet (T03.01)"
  fi
fi

# --- Data files and scripts (always) --------------------------------------------------------------
step "JSON files parse"
for f in catalog/*.json bundles/*.json schemas/*.json schemas/llm/*.json; do
  [ -f "$f" ] || continue
  node -e 'JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"))' "$f" \
    || { echo "  ✖ invalid JSON: $f" >&2; exit 1; }
done
echo "  ok"

step "Shell scripts parse (bash -n)"
for f in scripts/*.sh; do
  [ -f "$f" ] || continue
  bash -n "$f" || { echo "  ✖ syntax error: $f" >&2; exit 1; }
done
echo "  ok"

step "Node scripts parse (node --check)"
for f in scripts/*.mjs; do
  [ -f "$f" ] || continue
  node --check "$f" || { echo "  ✖ syntax error: $f" >&2; exit 1; }
done
echo "  ok"

step "Node script tests (node --test)"
if ls scripts/*.test.mjs >/dev/null 2>&1; then
  node --test scripts/*.test.mjs >/dev/null || { node --test scripts/*.test.mjs 2>&1 | tail -40; echo "  ✖ node tests failed" >&2; exit 1; }
fi
echo "  ok"

printf '\n\033[1;32m✔ all checks passed\033[0m\n'
