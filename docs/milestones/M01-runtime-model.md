# M01: Runtime and Model Pipeline

**Goal:** on your Mac, produce `vendor/runtime/llama-server-aarch64-apple-darwin` (pinned,
static, Metal) and `vendor/models/model.gguf` (pinned, SHA-256 verified), and answer spikes
S1–S3 with real measurements.

**Prereqs:** M00. Xcode Command Line Tools, CMake ≥ 3.21, git, Node 22.
**Exit demo:** `scripts/smoke-runtime.sh` starts the server with a random port and token, gets a
streamed answer and a schema-valid JSON answer, then shuts it down cleanly.

## Scope
- Run and harden the already-written `scripts/setup-runtime.sh` and `scripts/fetch-model.mjs` on macOS.
- Write `scripts/smoke-runtime.sh` (bash + curl) mirroring the inference contract by hand.
- Write `scripts/bench-model.sh` for S3.
- Record results in `docs/SPIKES.md`.

## Design notes

### `scripts/smoke-runtime.sh` (what it must do, in order)
```bash
PORT=$(python3 - <<'EOF'   # or node -e; pick a free port
import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()
EOF
)
TOKEN=$(openssl rand -hex 24)
LLAMA_API_KEY=$TOKEN vendor/runtime/llama-server-aarch64-apple-darwin -m vendor/models/model.gguf \
  --host 127.0.0.1 --port $PORT --ctx-size 8192 --parallel 1 --n-gpu-layers all --jinja \
  --reasoning-format deepseek --no-webui --timeout 120 > /tmp/nn-smoke.log 2>&1 &
PID=$!; trap 'kill $PID 2>/dev/null' EXIT
until curl -fsS http://127.0.0.1:$PORT/health >/dev/null; do sleep 0.3; done        # with a 90s timeout
curl -fsS -H "Authorization: Bearer $TOKEN" http://127.0.0.1:$PORT/v1/models           # must succeed
curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:$PORT/v1/models                # must be 401
curl -N -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"messages":[{"role":"user","content":"Say hi in 5 words."}],"stream":true,"temperature":0.7,"top_p":0.95,"min_p":0,"chat_template_kwargs":{"enable_thinking":false}}' \
  http://127.0.0.1:$PORT/v1/chat/completions                                           # prints SSE
# JSON schema call using schemas/llm/title.schema.json → parse with node/python, assert keys
```

### Spike protocols
- **S1 (thinking toggle):** ask "What is 17×23? Answer briefly." 5× with `enable_thinking:false` and
  5× with `true`. Record whether `reasoning_content` is present, whether `<think>` leaks into
  `content`, and latency. If `false` doesn't suppress thinking, also try the server flag
  `--reasoning off` and record it.
- **S2 (schema):** send `schemas/llm/rename-proposals.schema.json` (with a filled `enum` of ids and
  the snake_case `pattern`) with 5 bad names, 20 runs. Record the % valid JSON, % pattern-valid,
  and whether the `{"type":"json_schema","json_schema":{…}}` vs `{"type":"json_schema","schema":{…}}`
  shapes both work. Repeat 5 runs with thinking on.
- **S3 (quant):** `scripts/bench-model.sh <gguf>` runs 5 fixed prompts (in `scripts/bench-prompts.txt`)
  and records TTFT, tok/s (from `timings`), and peak RSS (`/usr/bin/time -l`). Compare V2 Q8_0 and
  V1 Q4_K_M (and the official MiniCPM5-1B Q8_0 if time allows). Rate quality 1–5 by hand.

### `docs/SPIKES.md` entry format
```
## S1: thinking toggle (2026-10-xx, b11552, <model file>)
Result: ✅/❌ … Evidence: (short table) … Decision: …
```

## Steps
1. `scripts/doctor.sh` → fix anything red.
2. `scripts/setup-runtime.sh` → expect ~3–6 min. Check `vendor/runtime/runtime.lock.json`.
3. `node scripts/fetch-model.mjs` → ~1.1 GB. Check `vendor/models/lock.json`. Then run
   `node scripts/fetch-model.mjs --verify`.
4. Write `scripts/smoke-runtime.sh`, make it pass, and commit it.
5. Run S1 and S2 with curl (or small scripts) and record them in `docs/SPIKES.md`.
6. Write `scripts/bench-model.sh` + `scripts/bench-prompts.txt`, run S3, and record. If the
   decision changes the default, update `catalog/models.json` `isDefault` and ADR-008.

## Exit criteria
- [ ] The runtime binary is arm64-only and `otool -L` shows only system libraries.
- [ ] The model lock exists and `--verify` passes.
- [ ] The smoke script passes 3 times in a row, with no orphan `llama-server` afterwards (`pgrep -f llama-server` is empty).
- [ ] S1, S2, and S3 are recorded with decisions.

## Pitfalls
- `-DGGML_METAL_EMBED_LIBRARY=ON` compiles shaders at first load (~1 s). That's expected.
- If CMake complains about the Metal toolchain on Xcode 26+, you're probably building precompiled
  metallibs. Keep EMBED=ON (no `metal` CLI needed).
- An HF 401/403 means the repo is gated → set `HF_TOKEN`.
- Use `min_p 0` for MiniCPM5. The llama.cpp default of 0.05 causes repetition (OpenBMB note).
