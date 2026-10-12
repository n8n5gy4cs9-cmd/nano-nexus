#!/usr/bin/env bash
# clean-trash.sh: free disk space by deleting rebuildable bundle leftovers. Never touches dist-bundles/,
# your original .gguf files, or source code.
#
#   scripts/clean-trash.sh              staged payloads + leftover app copies (safe, asks first)
#   scripts/clean-trash.sh --models     also the extra models in vendor/models (imported/downloaded ones);
#                                       KEEPS the default model that scripts/run.sh downloaded
#   scripts/clean-trash.sh --all        --models + target/ (all Rust build output; next build is slow)
#   scripts/clean-trash.sh --default-model   also remove that default model (run.sh re-downloads it)
#   scripts/clean-trash.sh --yes        do not ask
#   scripts/clean-trash.sh --dry-run    only show what would go
set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

MODELS=0; ALL=0; YES=0; DRY=0; DEFAULT=0
for a in "$@"; do
  case "$a" in
    --models) MODELS=1 ;;
    --all) ALL=1; MODELS=1 ;;
    --default-model) MODELS=1; DEFAULT=1 ;;
    --yes|-y) YES=1 ;;
    --dry-run) DRY=1 ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "unknown option: $a" >&2; exit 2 ;;
  esac
done

TARGETS=(vendor/bundle apps/desktop/src-tauri/gen apps/desktop/dist-web)
for d in target/release/bundle target/*/release/bundle; do [ -d "$d" ] && TARGETS+=("$d"); done
# Models: keep the default (index.json "default", fetched by run.sh) unless --default-model.
KEEPID=""
if [ "$MODELS" = 1 ] && [ "$DEFAULT" = 0 ] && [ -f vendor/models/index.json ]; then
  KEEPID="$(node -e 'try{const i=JSON.parse(require("fs").readFileSync("vendor/models/index.json","utf8"));process.stdout.write(i.default||"")}catch{}' 2>/dev/null || true)"
fi
if [ "$MODELS" = 1 ]; then
  if [ -z "$KEEPID" ]; then
    TARGETS+=(vendor/models)
  else
    for d in vendor/models/*/; do
      d="${d%/}"; [ -d "$d" ] || continue
      [ "$(basename "$d")" = "$KEEPID" ] || TARGETS+=("$d")
    done
  fi
fi
[ "$ALL" = 1 ] && TARGETS+=(target)

size() { du -sh "$1" 2>/dev/null | cut -f1; }
LIST=()
echo "Will delete:"
for t in "${TARGETS[@]}"; do
  if [ -e "$t" ]; then printf '  %8s  %s\n' "$(size "$t")" "$t"; LIST+=("$t"); fi
done
[ ${#LIST[@]} -eq 0 ] && { echo "  (nothing to clean)"; exit 0; }
echo "Kept: dist-bundles/, your original .gguf files, source code."
[ -n "$KEEPID" ] && echo "Kept default model: $KEEPID (vendor/models/$KEEPID)"
[ "$MODELS" = 1 ] && echo "Note: vendor/models holds hard links; space is only freed once no other copy exists."
[ "$DRY" = 1 ] && exit 0

if [ "$YES" != 1 ]; then
  read -r -p "Delete these? [y/N] " ans
  case "$ans" in y|Y|yes) ;; *) echo "Cancelled."; exit 0 ;; esac
fi
for t in "${LIST[@]}"; do rm -rf "$t"; done
# Drop removed models from the index so fetch-model/run.sh stay consistent.
if [ "$MODELS" = 1 ] && [ -n "$KEEPID" ] && [ -f vendor/models/index.json ]; then
  KEEPID="$KEEPID" node -e 'const fs=require("fs"),p="vendor/models/index.json",i=JSON.parse(fs.readFileSync(p,"utf8"));for(const k of Object.keys(i.models||{}))if(k!==process.env.KEEPID)delete i.models[k];fs.writeFileSync(p,JSON.stringify(i,null,2)+"\n")'
fi
echo "Done."
