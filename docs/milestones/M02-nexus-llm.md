# M02: Inference Crate `nexus-llm`

**Goal:** a tested Rust library that owns the `llama-server` process and talks to it: spawn,
health, ownership, streaming, structured JSON, scheduling, shutdown.

**Contract:** [`docs/architecture/INFERENCE-CONTRACT.md`](../architecture/INFERENCE-CONTRACT.md) (normative).
**Prereqs:** M01 (real binary + model for the ignored smoke tests).
**Exit demo:** `cargo run -p nexus-llm --example chat -- "Explain Rust ownership in 3 bullets"`
streams tokens. `cargo run -p nexus-llm --example json` prints a validated struct.

## Module layout
```
crates/nexus-llm/
  src/lib.rs           pub use …
  src/config.rs        RuntimeConfig (+ Default from Settings), to_args()
  src/port.rs          find_free_port(), is_bind_error(log_line)
  src/token.rs         new_token() -> SecretString
  src/process.rs       ServerProcess::spawn(cfg, port, token) ; stderr pump → LogRing + log file
  src/supervisor.rs    Supervisor { start, stop, restart, state() watch, client() } ; stale reaping (server.json)
  src/client.rs        LlmClient { health, models, tokenize, chat, chat_stream }
  src/sse.rs           SseDecoder (bytes → events), ThinkSplitter (<think> fallback)
  src/types.rs         ChatMessage, ChatRequest, ResponseFormat, StreamEvent, Usage, Timings, ModelsResponse
  src/scheduler.rs     Scheduler, Priority, JobHandle
  src/structured.rs    chat_json, Validate, ValidationCtx, Structured<T>
  src/error.rs         LlmError → nexus_core::NexusError mapping
  examples/chat.rs, examples/json.rs
  tests/support/mock_server.rs (axum)   tests/fixtures/fake-llama-server.sh
  tests/supervisor.rs  tests/client.rs  tests/sse.rs  tests/structured.rs  tests/real_model.rs (#[ignore])
```

## Key implementation notes
- **reqwest:** `Client::builder().no_proxy().connect_timeout(2s).pool_idle_timeout(30s)`. Add the
  bearer header per request (the token can change after a restart). Streaming:
  `resp.bytes_stream()` → `SseDecoder`.
- **SseDecoder:** buffer bytes, split on `\n` (strip a trailing `\r`), collect `data:` lines until a
  blank line, and emit the joined payload. `[DONE]` ends the stream. Decode UTF-8 only on complete
  lines (never per chunk) to avoid splitting multi-byte characters.
- **ThinkSplitter:** a state machine over content deltas that holds back up to 8 chars when the
  tail could be the start of `<think>`/`</think>`. Unit-test it with every split position.
- **Supervisor state:** `tokio::sync::watch::Sender<RuntimeState>`. A monitor task does `child.wait()`.
  On exit while Ready, it sends `Failed`, then auto-restarts once (`restart_budget` resets after
  10 minutes of healthy uptime).
- **Fake binary for tests:** `fake-llama-server.sh` reads `--port` from argv and `LLAMA_API_KEY`
  from env, then `exec`s a tiny test-only Rust bin (`tests/bin/mock_llama.rs`, declared as
  `[[bin]] name="mock-llama" required-features=["test-mock"]`) that serves the mock API. Modes
  are selected via the env var `MOCK_MODE` = `ok | slow_load | exit_early | wrong_token | bind_fail`.
- **Scheduler:** worker loop with `tokio::select!` biased toward the interactive queue. Each job
  carries a `oneshot` for results or an `mpsc` for stream events, plus a `CancellationToken`.
  `JobHandle::cancel()` triggers the token, and the worker drops the response stream.
- **structured::chat_json:** as specified in the contract. `ValidationCtx` carries things like
  allowed ids, regex, and reserved names. Domain validators live in the crates that own the
  domain (e.g. the rename validator in nexus-analyze), implementing `Validate`.

## Steps
1. Types and SSE decoder with exhaustive unit tests.
2. Client against the mock server (health, models auth, chat non-stream, stream, tokenize, error mapping).
3. Port, token, and process spawn, plus a LogRing.
4. Supervisor with the fake binary: happy path, early exit, bind retry, ownership failure,
   auto-restart, shutdown kills the process (assert with `sysinfo` that the PID is gone).
5. Stale reaping via `server.json`, verifying the exe path with `sysinfo` before killing.
6. Scheduler: priority ordering test (enqueue 3 background, then 1 interactive → interactive runs
   second, right after the in-flight job), cancellation test, bounded queue test.
7. `chat_json` with the retry and fallback matrix.
8. Examples and the real-model ignored tests. Run them on the Mac and paste the timings into `docs/SPIKES.md`.

## Exit criteria
- [ ] All non-ignored tests pass on macOS and Linux.
- [ ] `cargo test -p nexus-llm -- --ignored` passes on the Mac (ready < 10 s, stream ok, JSON ok).
- [ ] No orphan processes after the test suite (`pgrep -f llama-server` and `pgrep -f mock-llama` are empty).
- [ ] Public API documented with `///` (purpose + example for `Supervisor` and `chat_json`).

## Pitfalls
- `kill_on_drop` doesn't run on `std::process::exit`. The app must call `shutdown()` explicitly (M03).
- Never log request bodies at info level (they contain your files). Use the `debug` level, and keep it off by default.
- llama-server `--timeout` applies to idle read/write. A long prompt prefill can exceed 30 s on
  big contexts, which is why we use 120.
