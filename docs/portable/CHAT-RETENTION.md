# Chat, history, streaming and settings: retention map (UP.040)

Source audit: 2026-11-02. Source inspection only; nothing was built, run or tested. "Retained" means the
NanoNexus source path exists; it is not runtime-verified. References: `REFE/Portable-Local-Studio-main/app/frontend/src`
(`TextChat.jsx`, `Settings.jsx`, `App.jsx`, `themes.js`) and NanoNexus `service/chat.rs`, `conversations.rs`,
`attachments.rs`, `nexus-llm`, `features/chat`, `features/settings`.

## Rule

NanoNexus keeps one SQLite history, one streaming path and one settings store. Reference behaviour is added
as optional fields or UI on top of them. The reference's browser `localStorage` persistence (conversations
migration, `textSettings`, theme, font size, sidebar) is **not** copied: on macOS the WebView storage is
non-persistent in portable mode (UP.022), so anything worth keeping goes to USB settings (UP.022b).

## Capability map

| Reference capability | NanoNexus status | Decision |
|---|---|---|
| Streaming chat, token-by-token render, stop | `chat_send` / `chat_stop` over Transport stream; `useChatStream.ts` | **Retain as is** |
| Regenerate, edit and resend | `chat_regenerate`, `chat_edit_resend` | **Retain** (reference has no equivalent depth) |
| Persistent conversations (list/save/delete) | `conversations.rs`, SQLite in DIST | **Retain**; reference JSON/localStorage history is not adopted. No import path from reference history is planned (no shared data exists on a fresh DIST) |
| Conversation export | `conversation_export_md` | **Retain** |
| Markdown, code blocks, math | `Markdown.tsx`, `CodeBlock.tsx` | **Retain**. Math (KaTeX in the reference): check `Markdown.tsx` before claiming parity; open gap if absent |
| Thinking / reasoning block with duration, toggle | `ThoughtsBlock.tsx`, `think` flag on send/regenerate, `nexus-llm/think.rs` splits `<think>` spans | **Retain**. Reference's multi-tag parser is covered by llama-server `--reasoning-format deepseek` plus `think.rs`; unusual tag families (`<\|channel\|>thought`) are an unverified compatibility gap |
| Attachments: text/code/PDF documents | `attachments.rs` (`attachments_read`, `attachments_pick`) | **Retain**. Reference PDF.js extraction with a "scanned PDF has no text" message: verify NanoNexus gives an equally clear error |
| Image attachments + vision companion (mmproj) | Models page tracks `mmproj` as a companion role (UP.032). Chat image input is not wired | **Gap, owned by UP.041/045**: image attach only when the loaded chat model has a matching mmproj; otherwise the composer explains why, as the reference does |
| Auto response-token budget from context headroom | Not present; `max_tokens` comes from assistant/request defaults | **Gap, small**: optional "auto" budget derived from `runtime.ctx_size` minus prompt estimate; add only with a unit-testable pure function |
| Sampling settings (temperature, top_p, top_k, min_p, repeat penalty, max tokens) | Fields exist on `ChatRequest`; values come from the assistant TOML (`temperature` clamped by model `max_recommended_temperature`). No user-facing per-model override | **Gap**: per-model/per-chat overrides as *optional* settings with validated ranges. Assistant intent stays the default; do not break prompt_lint or ASSISTANTS contract |
| Context size, threads, GPU layers, flash attention | `runtime.ctx_size`, `runtime.threads`, `runtime.gpu_layers`, `runtime.flash_attn` in Settings, applied to llama-server args | **Retain**; hardware-aware defaults are UP.045/073 |
| Idle unload | `runtime.idle_unload_minutes` | **Retain** (reference lacks it) |
| Per-model saved settings (`/api/llm/model-settings`) | Not present | **Gap**: store per `model_id` in USB settings via the same optional-field route as sampling overrides |
| Tokens/s, token usage, generation stats line | `ChatStatusLine.tsx`, `ui.show_perf_stats` | **Retain**; telemetry display is UP.046 |
| Benchmark history | Not present | **Deferred**: port UI under UP.045; cannot be run under the current no-testing instruction |
| Web search toggle + time filter in chat | `tools.web_enabled` (assistant tools `search_web`, `fetch_url`, ADR-016), off by default | **Retain consent model**; time filter and richer providers are UP.047 |
| Model picker in chat, load progress | Models page + `models.active_model_id`, runtime events | **Retain**; no `localStorage` `selectedLlmModel` |
| Themes (8 presets) | `ui.theme` light/dark/system + `ui.accent` (6 accents) via `lib/theme.ts` tokens | **Adapt in UP.046** as token palettes, not raw hex (AGENTS.md) |
| Font size / sidebar width / section memory | Zoom setting exists; sidebar and section memory are browser-local | **UP.022b** |
| Duo, assistants, consultations, knowledge context in chat | Present, no reference equivalent | **Retain** (UP.048) |

## Differences recorded on purpose

- Reference history lives in browser/server JSON; NanoNexus history is SQLite with journal-governed side effects. The reference's legacy `chat_conversations` localStorage migration is dropped.
- Reference keeps all sampling in the browser and sends it per request. NanoNexus sampling is assistant-defined and clamped by the model catalog; user overrides must layer on top, never replace the clamp silently.
- Reference tool/web behaviour is not permission-gated; NanoNexus tools stay proposal-only and web stays opt-in.

## Follow-up items (not done by UP.040)

1. Per-model and per-chat sampling overrides (optional settings, validated ranges, bound to the model id).
2. Auto response-token budget.
3. Chat image attachments gated on mmproj (with UP.041).
4. Confirm math rendering and unusual reasoning-tag handling in source before claiming parity.

None of these are claimed as working.
