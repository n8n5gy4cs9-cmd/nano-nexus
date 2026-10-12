#!/usr/bin/env bash
# doctor.sh: check that this machine can build NanoNexus.
#
# Usage: scripts/doctor.sh [--windows [--arch x64|arm64] [--no-nsis]] [--quiet]
#   --windows  also check the Windows cross-build toolchain (cargo-xwin, NSIS, LLVM; built on the Mac)
#   --arch     check only one Windows architecture (default: both)
#   --no-nsis  the build makes no installer (portable ZIP), so NSIS is not needed
#   --quiet    only print problems; exit code 1 if a required check fails
#
# Portable: bash 3.2 (macOS default). No GNU-only flags.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WINDOWS=0; QUIET=0; WARCH=both; NONSIS=0
for arg in "$@"; do
  case "$arg" in
    --windows) WINDOWS=1 ;;
    --no-nsis) NONSIS=1 ;;
    --arch) ;;
    x64|arm64) WARCH="$arg" ;;
    --quiet) QUIET=1 ;;
    -h|--help) sed -n '2,8p' "$0"; exit 0 ;;
    *) echo "doctor.sh: unknown argument: $arg" >&2; exit 2 ;;
  esac
done

FAILS=0; WARNS=0
ok()   { [ "$QUIET" = "1" ] || printf '  \033[32m✔\033[0m %s\n' "$*"; }
warn() { WARNS=$((WARNS + 1)); printf '  \033[33m!\033[0m %s\n' "$*"; }
bad()  { FAILS=$((FAILS + 1)); printf '  \033[31m✖\033[0m %s\n' "$*"; }
hdr()  { [ "$QUIET" = "1" ] || printf '\n\033[1m%s\033[0m\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }

# ver_ge A B: true if version A >= version B (numeric, dot-separated, up to 3 parts)
ver_ge() {
  local a1 a2 a3 b1 b2 b3
  IFS=. read -r a1 a2 a3 <<EOF
$1
EOF
  IFS=. read -r b1 b2 b3 <<EOF
$2
EOF
  a1=${a1//[!0-9]/}; a2=${a2//[!0-9]/}; a3=${a3//[!0-9]/}
  b1=${b1//[!0-9]/}; b2=${b2//[!0-9]/}; b3=${b3//[!0-9]/}
  a1=${a1:-0}; a2=${a2:-0}; a3=${a3:-0}; b1=${b1:-0}; b2=${b2:-0}; b3=${b3:-0}
  if [ "$((10#$a1))" -ne "$((10#$b1))" ]; then [ "$((10#$a1))" -gt "$((10#$b1))" ]; return; fi
  if [ "$((10#$a2))" -ne "$((10#$b2))" ]; then [ "$((10#$a2))" -gt "$((10#$b2))" ]; return; fi
  [ "$((10#$a3))" -ge "$((10#$b3))" ]
}

# need <cmd> <min-version> <version-command...>: required tool with a minimum version
need() {
  local cmd="$1" min="$2"; shift 2
  if ! have "$cmd"; then bad "$cmd not found (need ≥ $min)"; return; fi
  local out ver
  out="$("$@" 2>&1 | head -n 1)"
  ver="$(printf '%s' "$out" | grep -Eo '[0-9]+\.[0-9]+(\.[0-9]+)?' | head -n 1)"
  if [ -z "$ver" ]; then warn "$cmd: couldn't parse its version from: $out"; return; fi
  if ver_ge "$ver" "$min"; then ok "$cmd $ver"; else bad "$cmd $ver is too old (need ≥ $min)"; fi
}

OS="$(uname -s)"; ARCH="$(uname -m)"

hdr "System"
if [ "$OS" = "Darwin" ]; then
  MACV="$(sw_vers -productVersion 2>/dev/null || echo 0)"
  if ver_ge "$MACV" "13.0"; then ok "macOS $MACV"; else bad "macOS $MACV (need ≥ 13)"; fi
  if [ "$ARCH" = "arm64" ]; then ok "Apple Silicon (arm64)"; else warn "arch $ARCH: Apple Silicon is the primary target"; fi
  if xcode-select -p >/dev/null 2>&1; then ok "Xcode Command Line Tools ($(xcode-select -p))"; else bad "Xcode Command Line Tools missing: run: xcode-select --install"; fi
  MEM_BYTES="$(sysctl -n hw.memsize 2>/dev/null || echo 0)"
  ok "RAM $((MEM_BYTES / 1073741824)) GB"
else
  warn "$OS/$ARCH: the primary dev platform is macOS on Apple Silicon (Linux works for non-🍎 tasks)"
fi

hdr "Toolchain"
need git 2.30 git --version
need cmake 3.21 cmake --version
need rustc 1.90 rustc --version
need cargo 1.90 cargo --version
need node 22.0 node --version
if have pnpm; then ok "pnpm $(pnpm --version 2>/dev/null)"; else bad "pnpm not found: run: corepack enable && corepack prepare pnpm@latest --activate"; fi
if have rustup; then
  if rustup component list --installed 2>/dev/null | grep -q '^clippy'; then ok "clippy installed"; else warn "clippy missing: run: rustup component add clippy rustfmt"; fi
else
  warn "rustup not found (recommended to manage toolchains and targets)"
fi
if have cargo && cargo tauri --version >/dev/null 2>&1; then ok "tauri-cli $(cargo tauri --version 2>/dev/null | head -n1)"; else ok "tauri-cli will come from apps/desktop devDependencies (@tauri-apps/cli 2.x)"; fi

hdr "Disk"
AVAIL_KB="$(df -Pk "$ROOT" 2>/dev/null | awk 'NR==2 {print $4}')"
AVAIL_GB=$(( ${AVAIL_KB:-0} / 1048576 ))
if [ "$AVAIL_GB" -ge 20 ]; then ok "$AVAIL_GB GB free"; elif [ "$AVAIL_GB" -ge 8 ]; then warn "$AVAIL_GB GB free (multi-model bundles need 2× model size + 5 GB)"; else bad "only $AVAIL_GB GB free (need ≥ 8 GB)"; fi

hdr "Local AI payload (vendor/)"
RT="$ROOT/vendor/runtime/macos-arm64/llama-server-aarch64-apple-darwin"
if [ -x "$RT" ]; then ok "runtime: $(basename "$RT")"; else warn "runtime not built yet: run: scripts/setup-runtime.sh"; fi
if [ -f "$ROOT/vendor/models/index.json" ]; then
  if have node; then
    DEF="$(node -e 'const i=require(process.argv[1]); const d=i.default; console.log(d ? d + " (" + Object.keys(i.models||{}).length + " model(s))" : "none")' "$ROOT/vendor/models/index.json" 2>/dev/null)"
    ok "models: default = $DEF"
  else
    ok "models: vendor/models/index.json present"
  fi
else
  warn "no models fetched yet: run: node scripts/fetch-model.mjs"
fi

if [ "$OS" = "Darwin" ]; then
  hdr "Signing (keeps macOS privacy grants stable across rebuilds)"
  ID="${NEXUS_SIGN_IDENTITY:-}"
  if [ -z "$ID" ]; then
    warn "NEXUS_SIGN_IDENTITY not set: create a 'NanoNexus Local Signing' certificate (see guide.html → One-time setup)"
  elif security find-identity -v -p codesigning 2>/dev/null | grep -q "$ID"; then
    ok "signing identity '$ID' found"
  else
    bad "signing identity '$ID' not found in the keychain"
  fi
fi

hdr "Optional tools"
if have pdftotext; then ok "pdftotext found (PDF fallback)"; else ok "pdftotext not found (optional)"; fi

if [ "$WINDOWS" = "1" ]; then
  hdr "Windows cross-build toolchain (built on this Mac)"
  if have rustup; then
    case "$WARCH" in
      x64) TARGETS="x86_64-pc-windows-msvc" ;;
      arm64) TARGETS="aarch64-pc-windows-msvc" ;;
      *) TARGETS="x86_64-pc-windows-msvc aarch64-pc-windows-msvc" ;;
    esac
    for t in $TARGETS; do
      if rustup target list --installed 2>/dev/null | grep -qx "$t"; then ok "rust target $t"; else bad "rust target $t missing: run: rustup target add $t"; fi
    done
  else
    bad "rustup is required to add Windows targets"
  fi
  if have cargo-xwin || cargo xwin --version >/dev/null 2>&1; then ok "cargo-xwin"; else bad "cargo-xwin missing: run: cargo install --locked cargo-xwin"; fi
  if [ "$NONSIS" = "1" ]; then ok "NSIS not needed (no installer in this build)";
  elif have makensis; then ok "NSIS $(makensis -VERSION 2>/dev/null)"; else bad "makensis missing: run: brew install nsis"; fi
  for t in clang-cl lld-link llvm-rc; do
    if have "$t"; then
      # Run it: a Homebrew lld/llvm version mismatch makes the binary exist but abort (dyld: Symbol not found).
      if "$t" --version >/dev/null 2>&1; then ok "$t"; else bad "$t is installed but crashes when run (usually mismatched Homebrew lld/llvm versions). Fix: brew update && brew upgrade llvm lld (or: brew reinstall lld llvm), then check: $t --version"; fi
    else bad "$t missing: run: brew install llvm, then add \"\$(brew --prefix llvm)/bin\" to PATH"; fi
  done
  if [ -n "${XWIN_CACHE_DIR:-}" ]; then ok "XWIN_CACHE_DIR=$XWIN_CACHE_DIR"; else warn "XWIN_CACHE_DIR not set (recommended: export XWIN_CACHE_DIR=~/.cache/xwin)"; fi
  for a in x64 arm64; do
    if [ "$WARCH" != "both" ] && [ "$WARCH" != "$a" ]; then continue; fi
    if [ -f "$ROOT/vendor/runtime/windows-$a/llama-server.exe" ]; then ok "Windows runtime ($a)"; else warn "Windows runtime ($a) not fetched yet: run: node scripts/fetch-runtime-windows.mjs --arch $a (M13)"; fi
  done
fi

echo
if [ "$FAILS" -gt 0 ]; then
  printf '\033[31m%d problem(s)\033[0m, %d warning(s). Fix the ✖ items above.\n' "$FAILS" "$WARNS"
  exit 1
fi
[ "$QUIET" = "1" ] || printf '\033[32mAll required checks passed\033[0m (%d warning(s)).\n' "$WARNS"
exit 0
