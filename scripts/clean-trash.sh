#!/usr/bin/env bash
# clean-trash.sh: free disk space by deleting rebuildable bundle leftovers. Never touches dist-bundles/,
# your original .gguf files, or source code.
#
#   scripts/clean-trash.sh              staged payloads + leftover app copies (safe, asks first)
#   scripts/clean-trash.sh --models     also vendor/models (imported/downloaded models; re-import later)
#   scripts/clean-trash.sh --all        also target/ (all Rust build output; next build is slow)
#   scripts/clean-trash.sh --yes        do not ask
#   scripts/clean-trash.sh --dry-run    only show what would go
set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

MODELS=0; ALL=0; YES=0; DRY=0
for a in "$@"; do
  case "$a" in
    --models) MODELS=1 ;;
    --all) ALL=1; MODELS=1 ;;
    --yes|-y) YES=1 ;;
    --dry-run) DRY=1 ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "unknown option: $a" >&2; exit 2 ;;
  esac
done

TARGETS=(vendor/bundle apps/desktop/src-tauri/gen apps/desktop/dist-web)
for d in target/release/bundle target/*/release/bundle; do [ -d "$d" ] && TARGETS+=("$d"); done
[ "$MODELS" = 1 ] && TARGETS+=(vendor/models)
[ "$ALL" = 1 ] && TARGETS+=(target)

size() { du -sh "$1" 2>/dev/null | cut -f1; }
LIST=()
echo "Will delete:"
for t in "${TARGETS[@]}"; do
  if [ -e "$t" ]; then printf '  %8s  %s\n' "$(size "$t")" "$t"; LIST+=("$t"); fi
done
[ ${#LIST[@]} -eq 0 ] && { echo "  (nothing to clean)"; exit 0; }
echo "Kept: dist-bundles/, your original .gguf files, source code."
[ "$MODELS" = 1 ] && echo "Note: vendor/models holds hard links; space is only freed once no other copy exists."
[ "$DRY" = 1 ] && exit 0

if [ "$YES" != 1 ]; then
  read -r -p "Delete these? [y/N] " ans
  case "$ans" in y|Y|yes) ;; *) echo "Cancelled."; exit 0 ;; esac
fi
for t in "${LIST[@]}"; do rm -rf "$t"; done
echo "Done."
