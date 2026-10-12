#!/usr/bin/env bash
# build-app.sh: one command to a verified, signed NanoNexus.app (macOS, Apple Silicon). BUNDLING §10.
#
# Usage: scripts/build-app.sh --profile <name|path> [--target macos-arm64] [--skip-doctor] [--dry-run]
#   --profile   a bundle profile: bundles/<name>.json, bundles/local/<name>.json, or a path (default: basic)
#   --dry-run   print every step instead of running it (works on any OS)
# Env: NEXUS_SIGN_IDENTITY  code-signing identity (default: ad-hoc "-", which makes macOS re-ask for
#                           folder access after each rebuild; see BUNDLING §10 for a stable local identity)
# Steps: doctor -> fetch + verify models -> runtime lock check -> stage -> web UI -> tauri build ->
#        verify-payload -> codesign -> zip + SHA256SUMS.txt in dist/<profile>/.
# Portable: bash 3.2.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PROFILE="basic"; TARGET="macos-arm64"; DRY=0; DOCTOR=1
while [ $# -gt 0 ]; do
  case "$1" in
    --profile) PROFILE="${2:?--profile needs a value}"; shift ;;
    --target) TARGET="${2:?--target needs a value}"; shift ;;
    --skip-doctor) DOCTOR=0 ;;
    --dry-run) DRY=1 ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "build-app.sh: unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done
[ "$TARGET" = "macos-arm64" ] || { echo "build-app.sh builds macOS only (got --target $TARGET). Windows: scripts/build-windows.sh" >&2; exit 2; }

step() { printf '\n\033[1m▶ %s\033[0m\n' "$*"; }
die()  { printf '\033[31m✖ %s\033[0m\n' "$*" >&2; exit 1; }
run()  { if [ "$DRY" = "1" ]; then printf '  + %s\n' "$*"; else printf '  $ %s\n' "$*"; "$@"; fi; }

if [ "$DRY" = "0" ]; then
  [ "$(uname -s)" = "Darwin" ] || die "macOS apps can only be built on a Mac (use --dry-run to see the steps)."
  [ "$(uname -m)" = "arm64" ] || die "this build targets Apple Silicon; run it on an arm64 Mac."
fi

# Profile name and app version.
NAME="$(node -e 'import("./scripts/fetch-model.mjs").then(m => console.log(m.resolveProfile(process.argv[1]).profile.name))' "$PROFILE")" \
  || die "profile not found: $PROFILE"
VERSION="$(node -p 'require("./apps/desktop/src-tauri/tauri.conf.json").version')"
STAGE="vendor/bundle/$NAME/$TARGET"
OVERLAY="apps/desktop/src-tauri/gen/tauri.bundle.$NAME.$TARGET.json"
APP="target/release/bundle/macos/NanoNexus.app"
OUT="dist/$NAME"
ZIP="NanoNexus-$VERSION-$NAME-$TARGET.zip"
echo "Profile $NAME · version $VERSION · $TARGET"

if [ "$DOCTOR" = "1" ]; then
  step "1/9 Checking the machine"
  run scripts/doctor.sh --quiet
fi

step "2/9 Models: download missing, verify hashes"
run node scripts/fetch-model.mjs --profile "$PROFILE"
run node scripts/fetch-model.mjs --verify

step "3/9 Runtime: build if needed, check it against its lock"
if [ "$DRY" = "1" ] || [ ! -f vendor/runtime/macos-arm64/runtime.lock.json ]; then
  run scripts/setup-runtime.sh
fi
if [ "$DRY" = "0" ]; then
  LOCKED="$(node -p 'require("./vendor/runtime/macos-arm64/runtime.lock.json").sha256')"
  ACTUAL="$(shasum -a 256 vendor/runtime/macos-arm64/llama-server-aarch64-apple-darwin | awk '{print $1}')"
  [ "$LOCKED" = "$ACTUAL" ] || die "llama-server does not match runtime.lock.json; run scripts/setup-runtime.sh --force"
fi

step "4/9 Staging the payload"
run node scripts/stage-bundle.mjs --profile "$PROFILE" --target "$TARGET"

step "5/9 Web UI (embedded for LAN Share)"
run pnpm -C apps/desktop build:web

step "6/9 tauri build"
run pnpm -C apps/desktop tauri build --bundles app --config "$ROOT/$OVERLAY"

step "7/9 Verifying the built app"
run node scripts/verify-payload.mjs "$APP" --manifest "$STAGE/models/manifest.json" --runtime-lock vendor/runtime/macos-arm64/runtime.lock.json

step "8/9 Signing"
IDENTITY="${NEXUS_SIGN_IDENTITY:--}"
if [ "$IDENTITY" = "-" ]; then
  echo "  (ad-hoc signing: macOS will re-ask for Documents/Desktop access after each rebuild; set NEXUS_SIGN_IDENTITY to avoid that)"
fi
run codesign --force --deep --options runtime --entitlements apps/desktop/src-tauri/entitlements.plist --sign "$IDENTITY" "$APP"
run codesign --verify --deep --strict "$APP"

step "9/9 Zip + checksums"
run mkdir -p "$OUT"
run rm -f "$OUT/$ZIP"
run ditto -c -k --keepParent "$APP" "$OUT/$ZIP"
if [ "$DRY" = "1" ]; then
  printf '  + (cd %s && shasum -a 256 %s > SHA256SUMS.txt)\n' "$OUT" "$ZIP"
else
  (cd "$OUT" && shasum -a 256 "$ZIP" > SHA256SUMS.txt)
  SIZE="$(du -sh "$APP" | awk '{print $1}')"
  printf '\n\033[32m✔ %s (%s)\033[0m\n  %s\n  %s\n' "$APP" "$SIZE" "$OUT/$ZIP" "$OUT/SHA256SUMS.txt"
  echo "  On another Mac: xattr -dr com.apple.quarantine NanoNexus.app"
fi
