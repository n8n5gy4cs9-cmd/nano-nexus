# Inference Contract (`crates/nexus-llm`)

This is the Rust clone of Godot's `model_transport.gd` + `strategy_provider.gd`.
**If you change anything here, update the integration tests in `crates/nexus-llm/tests/`.**

---

## 1. Spawn

```text
<llama-server> -m <model.gguf>
  --host 127.0.0.1 --port <N>
  --ctx-size 8192            # setting runtime.ctx_size (4096..32768)
  --parallel 1               # ADR-012: one slot, whole ctx
  --n-gpu-layers all         # or 99; setting runtime.gpu_layers ("all" | 0..999)
  --flash-attn auto
  --jinja                    # default on in b11552; pass explicitly for clarity
  --reasoning-format deepseek  # thoughts → message.reasoning_content
  --no-webui
  --timeout 120              # read/write timeout (s); 30 in Godot was too short for long answers
  --cache-reuse 256          # KV reuse across turns (prompt caching)
  --metrics                  # optional, behind a debug setting
env: LLAMA_API_KEY=<token>   # never in argv (visible in `ps`)
stdout/stderr: piped → rotating llama-server.log + in-memory ring buffer (last 200 lines)
```

Rules:
- **Port:** bind `std::net::TcpListener` on `127.0.0.1:0`, read the port, drop the listener,
  spawn. If the child exits within 1.5 s and its log contains `bind`/`address already in use`,
  pick a new port. **Max 16 attempts**, as in Godot.
- **Token:** 24 random bytes from `getrandom`, hex-encoded (48 chars). Keep it in
  `secrecy::SecretString`. Never log it or send it to the webview.
- **Command:** `tokio::process::Command` with `.kill_on_drop(true)`, `.stdin(Stdio::null())`,
  and `.env_clear()` followed by a minimal env (`PATH`, `HOME`, `TMPDIR`, `LLAMA_API_KEY`), so
  stray `LLAMA_ARG_*` env vars from your shell can't change behavior.
- On macOS, call `process_group(0)` (Unix `CommandExt`) so a kill can target the whole group.
- Write `<data>/runtime/server.json = {pid, port, started_at, exe}` after spawn. Delete it on clean stop.

## 2. Readiness and ownership

```text
loop every 150 ms → 500 ms (backoff), until ready_timeout (90 s default):
  if child.try_wait() is Some(status) → Failed{reason: "exited <status>", log_tail}
  GET /health              (public endpoint)
     503 {"error":{"code":503,"message":"Loading model"}} → state Loading
     200 {"status":"ok"}   → break
GET /v1/models  with Authorization: Bearer <token>
     200 and data.len()==1 → Ready{model_id: data[0].id, ctx_size}
     401/other             → Failed{"port answered but not our server"}; kill child; retry with new port
```

`/health` is unauthenticated, so a stale or foreign server on the same port could answer.
The authenticated `/v1/models` call proves the server is the one we own (same idea as Godot).

## 3. States (published on `runtime:status`)

```rust
#[derive(Clone, Serialize, specta::Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RuntimeState {
    Stopped,
    Starting { attempt: u8 },
    Loading,
    Ready { model_id: String, model_path: String, ctx_size: u32, gpu_layers: String },
    Failed { reason: String, log_tail: Vec<String>, can_retry: bool },
}
```

Supervisor policy:
- On unexpected child exit while `Ready`: emit `Failed`, then **auto-restart once**. If the
  second start fails, stay `Failed` until the user clicks Retry.
- `restart(new_config)`: stop (graceful), then start. Used for model switches and settings changes.
- **Graceful stop:** send SIGTERM (Unix) and wait up to 3 s, then SIGKILL. On Windows, call
  `child.kill()`.
- **Stale reaping on launch:** if `server.json` exists, the PID is alive, and its executable
  path equals our `llama-server` path (`sysinfo` crate), kill it. Never kill an unrelated
  process with a reused PID.
- **App exit:** in the `RunEvent::ExitRequested` / `RunEvent::Exit` handlers, run
  `supervisor.shutdown()` synchronously with `tauri::async_runtime::block_on`. **Don't rely only
  on `kill_on_drop`**, because `std::process::exit` skips destructors.

## 4. HTTP client

`reqwest::Client` built with `.no_proxy()` (loopback must never go through a proxy),
`.timeout(None)` for streaming, a `connect_timeout` of 2 s, and a default `Authorization` header.

### 4.1 Chat request (OpenAI-compatible + llama.cpp extras)

```json
POST /v1/chat/completions
{
  "messages": [{"role":"system","content":"…"}, {"role":"user","content":"…"}],
  "stream": true,
  "temperature": 0.7, "top_p": 0.95, "min_p": 0.0, "top_k": 40,
  "repeat_penalty": 1.0,
  "max_tokens": 1024,
  "cache_prompt": true,
  "chat_template_kwargs": {"enable_thinking": false},
  "stream_options": {"include_usage": true}
}
```

### 4.2 Streaming (SSE)

Lines arrive as `data: {json}\n\n` and end with `data: [DONE]`. Parse with a hand-rolled,
CRLF-tolerant line splitter over `bytes_stream()`. Don't use the browser `EventSource` (it can't
POST). Each chunk:

```json
{"choices":[{"delta":{"content":"Hel"},"finish_reason":null}]}
{"choices":[{"delta":{"reasoning_content":"Let me…"}}]}       // only when thinking is on
{"choices":[{"delta":{},"finish_reason":"stop"}], "usage":{…}, "timings":{"prompt_n":…,"predicted_per_second":…}}
```

Map these to `StreamEvent::{Reasoning(String), Content(String), Done{finish_reason, usage, timings}}`.
**Fallback stripping:** if `content` contains `<think>`…`</think>` (the template ignored the
flag and the server didn't extract it), route that span to `Reasoning`. Unit-test this with a
split-tag stream (`"<thi"`, `"nk>"`).

### 4.3 Structured output (ADR-003)

```json
{
  "messages": [...],
  "stream": false,
  "temperature": 0.3, "top_p": 0.9, "min_p": 0.0,
  "max_tokens": 512,
  "chat_template_kwargs": {"enable_thinking": false},
  "response_format": {
    "type": "json_schema",
    "json_schema": { "name": "rename_proposals", "strict": true, "schema": { … } }
  }
}
```

- llama.cpp converts the schema to a GBNF grammar. Supported and useful: `enum`, `const`,
  `pattern` (e.g. `^[a-z_][a-z0-9_]*$`), `minLength/maxLength`, `minItems/maxItems`,
  `required`, `additionalProperties:false`. **Keep schemas flat and small.** Deep `oneOf` and
  large enums slow sampling.
- Some builds also accept the README shape `{"type":"json_schema","schema":{…}}`. We send the
  OpenAI shape (proven in the Godot project). **Spike S2** verifies it on `b11552`.

```rust
pub struct StructuredSpec<'a> { name: &'a str, schema: &'a serde_json::Value, temperature: f32, max_tokens: u32 }
pub trait Validate { fn validate(&self, ctx: &ValidationCtx) -> Result<(), Vec<String>>; }

pub enum Structured<T> { Valid(T), Fallback { reason: String } }

pub async fn chat_json<T: DeserializeOwned + Validate>(
    sched: &Scheduler, prio: Priority, mut messages: Vec<ChatMessage>,
    spec: StructuredSpec<'_>, vctx: &ValidationCtx, cancel: CancellationToken,
) -> Result<Structured<T>, LlmError> {
    for attempt in 0..2 {
        let temp = if attempt == 0 { spec.temperature } else { (spec.temperature * 0.5).min(0.2) };
        let raw = sched.complete(prio, build_req(&messages, &spec, temp), cancel.clone()).await?;
        match serde_json::from_str::<T>(&raw).map_err(|e| vec![e.to_string()])
              .and_then(|v| v.validate(vctx).map(|_| v)) {
            Ok(v) => return Ok(Structured::Valid(v)),
            Err(errs) if attempt == 0 => {
                messages.push(ChatMessage::assistant(raw));
                messages.push(ChatMessage::user(format!(
                    "Your JSON was invalid:\n- {}\nReturn corrected JSON only.", errs.join("\n- "))));
            }
            Err(errs) => return Ok(Structured::Fallback { reason: errs.join("; ") }),
        }
    }
    unreachable!()
}
```

The caller turns `Fallback` into a **deterministic domain default**:
| Use | Fallback |
|---|---|
| Router decision | `{"action":"answer"}` (answer without tools) |
| Rename proposal for symbol X | keep the old name, status `needs_manual`, reason "model output invalid" |
| Content classification | `other` with confidence 0 |
| Conversation title | first 6 words of the user message, title-cased |
| Tidy plan for file F | leave in place |

## 5. Scheduler (ADR-012)

```rust
pub enum Priority { Interactive, Background }
pub struct Scheduler { tx: mpsc::Sender<Job>, ... }   // one worker task
impl Scheduler {
    pub async fn stream(&self, p: Priority, req: ChatRequest, cancel: CancellationToken)
        -> Result<impl Stream<Item = Result<StreamEvent, LlmError>>, LlmError>;
    pub async fn complete(&self, p: Priority, req: ChatRequest, cancel: CancellationToken)
        -> Result<String, LlmError>;
}
```

- Two queues. The worker always pops Interactive first.
- The background queue is bounded at **8**. Submitting more returns `LlmError::Busy`, and
  pipelines simply await capacity (`send().await`).
- A job holds the lane until its stream ends or is cancelled. Pipelines must split work into
  small requests (≤ 1 chunk each) so chat latency stays low.
- If the runtime isn't `Ready`, return `LlmError::RuntimeUnavailable` immediately. The UI shows
  "Model is loading…".

## 6. Token counting and context budget

`POST /tokenize {"content": "...", "add_special": false}` → `{"tokens":[…]}`. It runs locally
and takes a few milliseconds per KB. Use it for budgeting, with an LRU cache keyed by a content
hash. If it's unavailable, estimate `ceil(chars / 3.2)`.

Default budget for `ctx_size = 8192` (the budget scales proportionally):

| Slice | Tokens |
|---|---|
| System prompt (rendered) | ≤ 600 |
| Tool/attachment context | ≤ 3 000 |
| History (newest first, verbatim) | ≤ 2 400 |
| Rolling summary of older history | ≤ 300 |
| Reserved for the answer (`max_tokens`) | 1 024 (thinking on: 2 048, taken from attachments) |
| Safety margin (template tokens) | ≥ 200 |

## 7. Errors

```rust
#[derive(thiserror::Error, Debug)]
pub enum LlmError {
    #[error("runtime unavailable")] RuntimeUnavailable,
    #[error("busy")] Busy,
    #[error("cancelled")] Cancelled,
    #[error("http {status}: {body}")] Http { status: u16, body: String },
    #[error("transport: {0}")] Transport(String),
    #[error("context overflow: need {need}, have {have}")] ContextOverflow { need: u32, have: u32 },
    #[error("protocol: {0}")] Protocol(String),
}
```

llama-server returns HTTP 400 with `"the request exceeds the available context size"` when the
prompt is too long. Map that to `ContextOverflow`, re-budget more aggressively once, then surface
the error.

## 8. Tests (must exist before M2 is done)

1. **Mock server** (`wiremock` or a small `axum` app in `tests/support`): health 503→200,
   `/v1/models` auth checks, SSE streaming with split chunks, `[DONE]`, error bodies.
2. Supervisor with a **fake binary** (`tests/fixtures/fake-llama-server.sh` that runs the mock or
   sleeps/exits) covers: port retry, early-exit detection, ownership failure, auto-restart once,
   and kill on shutdown with no orphan.
3. SSE parser: CRLF, partial lines, multi-byte UTF-8 split across chunks, `<think>` split tags.
4. `chat_json`: valid first try · invalid → retry valid · invalid twice → Fallback · cancel mid-way.
5. **Real-model smoke test** (`#[ignore]`, run with `cargo test -p nexus-llm -- --ignored` on the
   Mac). Uses the real `vendor/runtime/llama-server` and model: ready < 10 s, one streamed
   reply, one schema-valid JSON.
