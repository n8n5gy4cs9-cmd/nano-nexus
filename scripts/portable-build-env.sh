#!/usr/bin/env bash
# Source this file: `. scripts/portable-build-env.sh <scratch-dir>`.
# Sends every cache and temp file of the build tools into <scratch-dir> (on the external drive) so nothing is
# added to the internal SSD. The only internal paths still used are read-only: ~/.cargo/bin (cargo, cargo-xwin).
# HOME itself is redirected, so tools that write to ~/Library, ~/.cache or ~/.npm land in the scratch folder too.
S="${1:?scratch dir}"
S="$(cd "$S" && pwd)"
case "$S" in /Volumes/*) ;; *) echo "refusing: $S is not on an external volume" >&2; return 1 ;; esac
export REAL_CARGO_BIN="${REAL_CARGO_BIN:-$HOME/.cargo/bin}"
export HOME="$S/home"
export CARGO_HOME="$S/cargo"
export RUSTUP_HOME="$S/rustup"
export CARGO_TARGET_DIR="$S/target"
export TMPDIR="$S/tmp"
export XWIN_CACHE_DIR="$S/xwin"
export XDG_CACHE_HOME="$S/home/.cache"
export npm_config_cache="$S/home/.npm"
export COREPACK_HOME="$S/home/.corepack"
export PATH="$REAL_CARGO_BIN:/opt/homebrew/opt/llvm/bin:$PATH"
mkdir -p "$HOME" "$TMPDIR"
