#!/usr/bin/env bash
# progress.sh: summarize tasks.md (done/undone per milestone, next tasks, blocked tasks).
#
# Usage:
#   scripts/progress.sh            summary: totals, per-milestone bars, next 5 tasks, blocked
#   scripts/progress.sh --next     print only the next unchecked task ("ID title")
#   scripts/progress.sh --all      list every unchecked task instead of the next 5
#   scripts/progress.sh --write    also refresh the <!-- progress:start/end --> block in tasks.md
#
# Portable: bash 3.2 (macOS default) + POSIX awk (BSD awk, mawk, gawk).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TASKS="$ROOT/tasks.md"
MODE="summary"
WRITE=0
SHOW=5

for arg in "$@"; do
  case "$arg" in
    --write) WRITE=1 ;;
    --next) MODE="next" ;;
    --all) SHOW=100000 ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "progress.sh: unknown argument: $arg" >&2; exit 2 ;;
  esac
done

[ -f "$TASKS" ] || { echo "progress.sh: tasks.md not found at $TASKS" >&2; exit 1; }

# run_awk <mode> <show>
run_awk() {
  awk -v mode="$1" -v show="$2" '
    function trim(s) { sub(/^[ \t]+/, "", s); sub(/[ \t]+$/, "", s); return s }
    /^## / {
      ms = $0; sub(/^## /, "", ms); sub(/ ·.*$/, "", ms)
      order[++nm] = ms; next
    }
    /^- \[[ x]\] \*\*/ {
      isdone = (substr($0, 4, 1) == "x")
      line = $0; sub(/^- \[[ x]\] \*\*/, "", line)
      id = line; sub(/\*\*.*$/, "", id)
      title = line; sub(/^[^*]*\*\*/, "", title); title = trim(title)
      tot[ms]++; total++
      if (isdone) { dn[ms]++; ndone++ }
      else if (index(title, "⛔") > 0) { blocked[++nb] = id " " title }
      else if (nn < show) { nxt[++nn] = id " " title }
    }
    END {
      pct = total ? int(ndone * 100 / total) : 0
      if (mode == "next") { if (nn > 0) print nxt[1]; else print "All tasks done."; exit }
      if (mode == "counts") { printf "%d %d %d\n", ndone, total, pct; exit }
      printf "Progress: %d/%d done (%d%%)\n\n", ndone, total, pct
      for (i = 1; i <= nm; i++) {
        m = order[i]; if (!tot[m]) continue
        d = dn[m] + 0; t = tot[m]; w = 20; f = int(d * w / t); bar = ""
        for (j = 0; j < w; j++) bar = bar (j < f ? "#" : ".")
        mark = (d == t) ? " done" : ""
        printf "  %-52s [%s] %3d/%-3d%s\n", substr(m, 1, 52), bar, d, t, mark
      }
      if (nn > 0) { printf "\nNext:\n"; for (i = 1; i <= nn; i++) printf "  %s\n", nxt[i] }
      if (nb > 0) { printf "\nBlocked (see docs/QUESTIONS.md):\n"; for (i = 1; i <= nb; i++) printf "  %s\n", blocked[i] }
    }' "$TASKS"
}

if [ "$MODE" = "next" ]; then
  run_awk next 1
else
  run_awk summary "$SHOW"
fi

if [ "$WRITE" = "1" ]; then
  counts="$(run_awk counts 1)"
  # shellcheck disable=SC2086
  set -- $counts
  next_task="$(run_awk next 1)"
  new_line="**Progress:** $1/$2 tasks done ($3%) · **Next:** $next_task"
  tmp="$(mktemp "$TASKS.XXXXXX")"
  NEW_LINE="$new_line" awk '
    /<!-- progress:start -->/ { print; print ENVIRON["NEW_LINE"]; skip = 1; next }
    /<!-- progress:end -->/ { skip = 0 }
    !skip { print }' "$TASKS" > "$tmp"
  if grep -q '<!-- progress:start -->' "$tmp"; then
    mv "$tmp" "$TASKS"
    echo
    echo "Updated progress block in tasks.md"
  else
    rm -f "$tmp"
    echo "progress.sh: no <!-- progress:start --> marker in tasks.md; nothing written" >&2
    exit 1
  fi
fi
