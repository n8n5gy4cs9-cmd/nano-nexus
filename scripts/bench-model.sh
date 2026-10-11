#!/usr/bin/env bash
# bench-model.sh: S3 benchmark. Runs the prompts in scripts/bench-prompts.txt against one GGUF and
# prints TTFT, tokens/s (from llama-server `timings`) and peak RSS (macOS: /usr/bin/time -l).
# Usage: scripts/bench-model.sh <model.gguf> [--ctx 8192]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MODEL="${1:-}"; [ -f "$MODEL" ] || { echo "usage: $0 <model.gguf> [--ctx N]" >&2; exit 2; }
CTX=8192; [ "${2:-}" = "--ctx" ] && CTX="${3:-8192}"
BIN="${LLAMA_SERVER_BIN:-vendor/runtime/macos-arm64/llama-server-aarch64-apple-darwin}"
[ -x "$BIN" ] || { echo "bench: runtime missing: $BIN" >&2; exit 2; }

PORT="$(node -e 'const s=require("net").createServer().listen(0,"127.0.0.1",()=>{console.log(s.address().port);s.close()})')"
TOKEN="$(openssl rand -hex 24)"
BASE="http://127.0.0.1:$PORT"
LOG="$(mktemp -t nn-bench.XXXXXX)"; TIMEFILE="$(mktemp -t nn-bench-time.XXXXXX)"

/usr/bin/time -l -o "$TIMEFILE" env LLAMA_API_KEY="$TOKEN" "$BIN" -m "$MODEL" --host 127.0.0.1 \
  --port "$PORT" --ctx-size "$CTX" --parallel 1 --n-gpu-layers all --jinja --reasoning-format deepseek \
  --no-webui >"$LOG" 2>&1 &
TPID=$!
trap 'pkill -P "$TPID" 2>/dev/null || true; kill "$TPID" 2>/dev/null || true' EXIT

deadline=$((SECONDS + 120))
until curl -fsS "$BASE/health" >/dev/null 2>&1; do
  [ "$SECONDS" -lt "$deadline" ] || { echo "bench: server did not become healthy" >&2; tail -n 20 "$LOG" >&2; exit 1; }
  sleep 0.5
done

echo "model: $MODEL  ctx: $CTX"
printf '%-4s %-10s %-10s %-8s\n' "#" "ttft_ms" "tok/s" "tokens"
n=0
while IFS= read -r prompt; do
  [ -n "$prompt" ] || continue
  n=$((n + 1))
  body="$(PROMPT="$prompt" node -e 'console.log(JSON.stringify({messages:[{role:"user",content:process.env.PROMPT}],stream:false,max_tokens:256,temperature:0.7,top_p:0.95,min_p:0,chat_template_kwargs:{enable_thinking:false}}))')"
  resp="$(curl -fsS --max-time 120 -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' -d "$body" "$BASE/v1/chat/completions")"
  echo "$resp" | N="$n" node -e '
    let d=""; process.stdin.on("data",c=>d+=c).on("end",()=>{
      const j=JSON.parse(d), t=j.timings||{};
      const f=(x)=>x==null?"-":Number(x).toFixed(1);
      console.log(String(process.env.N).padEnd(4), f(t.prompt_ms).padEnd(10), f(t.predicted_per_second).padEnd(10), String(t.predicted_n??"-").padEnd(8));
    });'
done < scripts/bench-prompts.txt

kill "$TPID" 2>/dev/null || true; pkill -P "$TPID" 2>/dev/null || true; wait "$TPID" 2>/dev/null || true
trap - EXIT
RSS="$(awk '/maximum resident set size/ {print $1}' "$TIMEFILE" | tail -n1)"
[ -n "$RSS" ] && echo "peak RSS: $((RSS / 1048576)) MiB"
echo "(rate answer quality 1-5 by hand and record in docs/SPIKES.md)"
