#!/usr/bin/env bash
# setup-runtime.sh: build the pinned llama.cpp `llama-server` used by NanoNexus.
#
# macOS (default): static arm64 binary, Metal + CPU, Metal shaders embedded (no .metallib needed)
#   → vendor/runtime/macos-arm64/llama-server-aarch64-apple-darwin  (+ runtime.lock.json, licenses/)
# Linux (--linux): static-ish CPU build (or --vulkan) for optional Linux builds (M16)
#   → vendor/runtime/linux-<arch>/llama-server-<triple>
# Windows runtimes are NOT built here: see scripts/fetch-runtime-windows.mjs (official pinned binaries).
#
# Usage: scripts/setup-runtime.sh [--clean] [--jobs N] [--linux] [--vulkan] [--force]
#   --clean   delete the build directory first
#   --force   rebuild even if an up-to-date runtime is installed
#
# Pins come from scripts/runtime.pins (tag + full commit SHA). The source is fetched by exact
# commit, so git verifies its integrity. Clone of the Godot project's setup-runtime.sh idea.
# Portable: bash 3.2.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=/dev/null
. "$ROOT/scripts/runtime.pins"

CLEAN=0; FORCE=0; LINUX=0; VULKAN=0
JOBS="$( (sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4) | head -n 1)"
while [ $# -gt 0 ]; do
  case "$1" in
    --clean) CLEAN=1 ;;
    --force) FORCE=1 ;;
    --linux) LINUX=1 ;;
    --vulkan) VULKAN=1 ;;
    --jobs) shift; JOBS="${1:?--jobs needs a number}" ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "setup-runtime.sh: unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

log()  { printf '\033[1m▶ %s\033[0m\n' "$*"; }
die()  { printf '\033[31m✖ %s\033[0m\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "$1 is required ($2)"; }

OS="$(uname -s)"; ARCH="$(uname -m)"
need git "install Xcode Command Line Tools or git"
need cmake "brew install cmake"

if [ "$LINUX" = "1" ]; then
  [ "$OS" = "Linux" ] || die "--linux must run on Linux (use a Linux VM on the Mac; see M16)"
  case "$ARCH" in
    x86_64) TARGET="linux-x64"; TRIPLE="x86_64-unknown-linux-gnu" ;;
    aarch64|arm64) TARGET="linux-arm64"; TRIPLE="aarch64-unknown-linux-gnu" ;;
    *) die "unsupported Linux arch: $ARCH" ;;
  esac
else
  [ "$OS" = "Darwin" ] || die "this builds the macOS runtime; on Linux pass --linux. Windows: node scripts/fetch-runtime-windows.mjs"
  [ "$ARCH" = "arm64" ] || die "Apple Silicon (arm64) required, got $ARCH"
  TARGET="macos-arm64"; TRIPLE="aarch64-apple-darwin"
fi

SRC="$ROOT/vendor/llama.cpp-src"
BUILD="$ROOT/vendor/llama.cpp-build/$TARGET"
OUT="$ROOT/vendor/runtime/$TARGET"
BIN_NAME="llama-server-$TRIPLE"
LOCK="$OUT/runtime.lock.json"

sha256() { if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'; else sha256sum "$1" | awk '{print $1}'; fi; }

# Up to date already?
if [ "$FORCE" = "0" ] && [ -x "$OUT/$BIN_NAME" ] && [ -f "$LOCK" ] && grep -q "\"commit\": \"$LLAMA_CPP_COMMIT\"" "$LOCK"; then
  if grep -q "\"sha256\": \"$(sha256 "$OUT/$BIN_NAME")\"" "$LOCK"; then
    log "Runtime already built for $LLAMA_CPP_TAG ($TARGET) and matches its lock. Use --force to rebuild."
    exit 0
  fi
fi

# 1) Source at the exact pinned commit -----------------------------------------------------
log "Fetching llama.cpp $LLAMA_CPP_TAG ($LLAMA_CPP_COMMIT)"
if [ -d "$SRC/.git" ] && [ "$(git -C "$SRC" rev-parse HEAD 2>/dev/null || true)" = "$LLAMA_CPP_COMMIT" ]; then
  echo "  source already at the pinned commit"
else
  rm -rf "$SRC"; mkdir -p "$SRC"
  git -C "$SRC" init -q
  git -C "$SRC" remote add origin "$LLAMA_CPP_REPO"
  git -C "$SRC" fetch -q --depth 1 origin "$LLAMA_CPP_COMMIT"
  git -C "$SRC" -c advice.detachedHead=false checkout -q FETCH_HEAD
fi
HEAD_SHA="$(git -C "$SRC" rev-parse HEAD)"
[ "$HEAD_SHA" = "$LLAMA_CPP_COMMIT" ] || die "checked-out commit $HEAD_SHA != pinned $LLAMA_CPP_COMMIT"

# 2) Configure -----------------------------------------------------------------------------------
[ "$CLEAN" = "1" ] && rm -rf "$BUILD"
mkdir -p "$BUILD"

FLAGS="-DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DLLAMA_BUILD_SERVER=ON -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_CURL=OFF -DLLAMA_OPENSSL=OFF -DGGML_NATIVE=OFF"
if [ "$LINUX" = "1" ]; then
  if [ "$VULKAN" = "1" ]; then FLAGS="$FLAGS -DGGML_VULKAN=ON"; fi
else
  FLAGS="$FLAGS -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON -DCMAKE_OSX_ARCHITECTURES=arm64 -DCMAKE_OSX_DEPLOYMENT_TARGET=$MACOS_DEPLOYMENT_TARGET"
fi

log "Configuring ($TARGET)"
# shellcheck disable=SC2086
cmake -S "$SRC" -B "$BUILD" $FLAGS -Wno-dev >/dev/null

# 3) Build -------------------------------------------------------------------------------------------
log "Building llama-server with $JOBS jobs (takes a few minutes)"
cmake --build "$BUILD" --config Release --target llama-server -j "$JOBS"

BIN="$BUILD/bin/llama-server"
[ -x "$BIN" ] || die "build finished but $BIN was not found"

# 4) Verify ------------------------------------------------------------------------------------------
log "Verifying binary"
if [ "$LINUX" = "0" ]; then
  ARCHS="$(lipo -archs "$BIN")"
  [ "$ARCHS" = "arm64" ] || die "expected arm64-only binary, got: $ARCHS"
  echo "  arch: $ARCHS"
  BAD_LIBS="$(otool -L "$BIN" | tail -n +2 | awk '{print $1}' | grep -Ev '^(/usr/lib/|/System/Library/)' || true)"
  [ -z "$BAD_LIBS" ] || die "non-system dynamic libraries linked (must be OS-only):
$BAD_LIBS"
  echo "  linkage: system libraries only"
else
  echo "  linkage:"; ldd "$BIN" | sed 's/^/    /' || true
fi
VERSION_OUT="$("$BIN" --version 2>&1 | head -n 3 | tr '\n' ' ' || true)"
echo "  version: $VERSION_OUT"

# 5) Install + lock -----------------------------------------------------------------------------
log "Installing to ${OUT#"$ROOT"/}"
mkdir -p "$OUT/licenses"
cp "$BIN" "$OUT/$BIN_NAME"
chmod 755 "$OUT/$BIN_NAME"
cp "$SRC/LICENSE" "$OUT/licenses/llama.cpp-LICENSE"

SUM="$(sha256 "$OUT/$BIN_NAME")"
BYTES="$(wc -c < "$OUT/$BIN_NAME" | tr -d ' ')"
HOST="$(uname -srm)"; [ "$OS" = "Darwin" ] && HOST="macOS $(sw_vers -productVersion) $(uname -m)"
cat > "$LOCK" <<EOF
{
  "schema": 1,
  "target": "$TARGET",
  "file": "$BIN_NAME",
  "repo": "$LLAMA_CPP_REPO",
  "tag": "$LLAMA_CPP_TAG",
  "commit": "$LLAMA_CPP_COMMIT",
  "sha256": "$SUM",
  "bytes": $BYTES,
  "cmake_flags": "$FLAGS",
  "built_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "host": "$HOST",
  "source": "built-from-source"
}
EOF

log "Done"
echo "  binary : ${OUT#"$ROOT"/}/$BIN_NAME"
echo "  sha256 : $SUM"
echo "  lock   : ${LOCK#"$ROOT"/}"
echo "Next: node scripts/fetch-model.mjs   (then scripts/smoke-runtime.sh once M01 adds it)"
