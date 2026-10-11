#!/usr/bin/env bash
# smoke-runtime.sh: start llama-server on a random port with a random token and exercise the
# inference contract by hand (health, auth 200/401, SSE stream, JSON-schema call), then kill it.
# Usage: scripts/smoke-runtime.sh [--runs N]     (macOS arm64; needs vendor/ from M01)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

RUNS=1
[ "${1:-}" = "--runs" ] && RUNS="${2:-1}"

BIN="${LLAMA_SERVER_BIN:-vendor/runtime/macos-arm64/llama-server-aarch64-apple-darwin}"
[ -x "$BIN" ] || { echo "smoke: runtime missing: $BIN (run scripts/setup-runtime.sh)" >&2; exit 2; }
MODEL="$(node scripts/fetch-model.mjs --print-default-path)"
[ -f "$MODEL" ] || { echo "smoke: model missing: $MODEL (run node scripts/fetch-model.mjs)" >&2; exit 2; }

LOG="$(mktemp -t nn-smoke.XXXXXX)"
PID=""
cleanup() {
  if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
    kill "$PID" 2>/dev/null || true
    for _ in 1 2 3 4 5 6; do kill -0 "$PID" 2>/dev/null || break; sleep 0.5; done
    kill -9 "$PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

fail() { echo "✖ smoke: $*" >&2; echo "--- server log tail ---" >&2; tail -n 30 "$LOG" >&2; exit 1; }

run_once() {
  local PORT TOKEN BASE
  PORT="$(node -e 'const s=require("net").createServer().listen(0,"127.0.0.1",()=>{console.log(s.address().port);s.close()})')"
  TOKEN="$(openssl rand -hex 24)"
  BASE="http://127.0.0.1:$PORT"

  LLAMA_API_KEY="$TOKEN" "$BIN" -m "$MODEL" --host 127.0.0.1 --port "$PORT" --ctx-size 8192 \
    --parallel 1 --n-gpu-layers all --jinja --reasoning-format deepseek --no-webui --timeout 120 \
    >"$LOG" 2>&1 &
  PID=$!

  echo "▶ waiting for /health (port $PORT)"
  local deadline=$((SECONDS + 90))
  until curl -fsS "$BASE/health" >/dev/null 2>&1; do
    kill -0 "$PID" 2>/dev/null || fail "server exited during startup"
    [ "$SECONDS" -lt "$deadline" ] || fail "no /health within 90s"
    sleep 0.3
  done

  echo "▶ auth: /v1/models with token must be 200, without must be 401"
  curl -fsS -H "Authorization: Bearer $TOKEN" "$BASE/v1/models" >/dev/null || fail "/v1/models with token failed"
  local code
  code="$(curl -s -o /dev/null -w '%{http_code}' "$BASE/v1/models")"
  [ "$code" = "401" ] || fail "expected 401 without token, got $code"

  echo "▶ SSE stream"
  local sse
  sse="$(curl -sN --max-time 60 -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
    -d '{"messages":[{"role":"user","content":"Say hi in 5 words."}],"stream":true,"temperature":0.7,"top_p":0.95,"min_p":0,"chat_template_kwargs":{"enable_thinking":false}}' \
    "$BASE/v1/chat/completions")"
  echo "$sse" | grep -q '^data: \[DONE\]' || fail "SSE stream did not end with [DONE]"
  echo "$sse" | grep -q '"delta"' || fail "SSE stream had no delta chunks"

  echo "▶ JSON-schema call (title.schema.json)"
  local body resp
  body="$(node -e '
    const s = JSON.parse(require("fs").readFileSync("schemas/llm/title.schema.json", "utf8"));
    delete s["$comment"];
    console.log(JSON.stringify({
      messages: [{ role: "user", content: "Give a short title for a chat about baking sourdough bread." }],
      temperature: 0.2, max_tokens: 64, chat_template_kwargs: { enable_thinking: false },
      response_format: { type: "json_schema", json_schema: { name: "title", strict: true, schema: s } },
    }));')"
  resp="$(curl -fsS --max-time 60 -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
    -d "$body" "$BASE/v1/chat/completions")" || fail "schema request failed"
  echo "$resp" | node -e '
    let d = ""; process.stdin.on("data", c => d += c).on("end", () => {
      const out = JSON.parse(JSON.parse(d).choices[0].message.content);
      if (typeof out.title !== "string" || out.title.length < 3 || out.title.length > 48) {
        console.error("bad title:", out); process.exit(1);
      }
      console.log("  title:", out.title);
    });' || fail "schema output invalid"

  echo "▶ shutdown"
  cleanup
  ! kill -0 "$PID" 2>/dev/null || fail "server still running after kill"
  PID=""
}

for i in $(seq 1 "$RUNS"); do
  echo "=== run $i/$RUNS ==="
  run_once
done

if pgrep -f llama-server >/dev/null 2>&1; then
  fail "orphan llama-server process left behind"
fi
echo "✔ smoke passed ($RUNS run(s)), no orphan llama-server"
