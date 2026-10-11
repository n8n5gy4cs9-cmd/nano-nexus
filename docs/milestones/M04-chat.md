# M04: Chat MVP ★ (first daily-usable build)

**Goal:** a delightful streaming chat with the local model, with persistent conversations. At
this stage it uses the **Nexus** assistant with a plain `chat` workflow. Tools and other
assistants come in M06.

**Specs:** UI-DESIGN §4.1, DATA-AND-IPC (`conversations`, `messages`, `attachments`, `ChatEvent`),
INFERENCE-CONTRACT §4–6, ASSISTANTS §2 (preamble) and §4 (budget).
**Prereqs:** M03. **Exit demo:** ask a coding question, watch it stream at ≥ 40 tok/s, stop
mid-answer, regenerate, toggle Think harder and see Thoughts, restart the app, and find the
conversation (searchable) with an auto-generated title.

## Scope
- `nexus-store`: migration 0001 + repositories for settings, conversations, messages (+ FTS), attachments.
- `nexus-agent` (minimal): `PromptContext`, `render_system_prompt`, `ContextBudget`, a
  `TurnEngine` with the **chat workflow only**, the title job, and history summarization.
  The built-in `nexus.toml` is loaded (full registry in M06).
- Commands: `chat_send`, `chat_stop`, `chat_regenerate`, `chat_edit_resend`, `conversations_*`, `conversation_export_md`.
- UI: conversation list (grouped, search, pin, rename, delete), chat view, composer,
  markdown renderer, thoughts block, stats, empty state, drag-drop **text files** as attachments
  (folders come in M06 with permissions, so read only files the user dropped; dropping counts as
  intent, read once, no grant).

## Design notes

### Architecture rules introduced here (needed by LAN Share, M14)
- **Service layer:** business logic lives in `src-tauri/src/service/*.rs` as
  `async fn send(ctx: &AppCtx, caller: Caller, args: ChatSendArgs, sink: impl Sink<ChatEvent>)`.
  The Tauri command only adapts: `service::chat::send(&ctx, Caller::Desktop, args, ChannelSink(on_event))`.
- **Frontend Transport:** feature code calls `api.chat.send(args, onEvent)` from `src/lib/api.ts`,
  which uses `src/lib/transport/tauriTransport.ts` now and `wsTransport.ts` in M14. **Never import
  `invoke` in feature code** (an ESLint `no-restricted-imports` rule enforces this).
- **Responsive from day one:** the chat view and composer must work at 390 px width (mobile
  breakpoint), even though the desktop window min-width is 900 px.

### Rust flow for `chat_send`
```rust
#[tauri::command] #[specta::specta]
async fn chat_send(state: State<'_, AppState>, args: ChatSendArgs, on_event: Channel<ChatEvent>) -> Result<ChatSendResult, IpcError> {
    let conv = state.store.conversations().get_or_create(args.conversation_id, &args.assistant_id)?;
    let user = state.store.messages().insert_user(&conv.id, &args.text, &args.attachments)?;
    let asst = state.store.messages().insert_assistant_placeholder(&conv.id, &args.assistant_id)?; // status=streaming
    let cancel = state.jobs.register_chat(&asst.id);                 // for chat_stop
    let engine = state.engine.clone();
    tauri::async_runtime::spawn(async move {
        let result = engine.run_turn(TurnInput { conv, user, think: args.think, … }, ChannelSink::new(on_event), cancel).await;
        // persist final content/reasoning/stats or error/stopped status; maybe enqueue title job; maybe notify
    });
    Ok(ChatSendResult { conversation_id, user_message_id, assistant_message_id })
}
```
- Persist content every 500 ms while streaming (crash safety) and once more at the end.
- `chat_stop` → `cancel.cancel()` → stream dropped → status `stopped` (partial text kept).
- `chat_regenerate` → delete the old assistant message (keep it as `superseded` if you want
  history later; v1 deletes it) → re-run with the same user message.
- **Reasoning is never sent back** in history (MiniCPM5 and Qwen-style guidance: strip thoughts
  from prior turns).

### Title job
After the first assistant message completes, enqueue a background `chat_json` with
`prompts/internal/title.md` and `schemas/llm/title.schema.json`. Fallback: the first 6 words of
the user message. The UI updates via query invalidation on `conversation:updated`.

### Frontend pieces
```
features/chat/
  ChatView.tsx            header (assistant chip, title, menu) + MessageList + Composer
  MessageList.tsx         virtualized when > 200 messages; auto-stick to bottom unless user scrolled up ("↓ New messages" pill)
  Message.tsx             user bubble / assistant block (avatar, tool chips, ThoughtsBlock, Markdown, toolbar, stats)
  Markdown.tsx            react-markdown + remark-gfm; code → <CodeBlock> (Shiki, lazy highlighter singleton)
  CodeBlock.tsx           header (lang, copy, open in editor [disabled until M10])
  ThoughtsBlock.tsx       collapsed by default, live timer while streaming
  Composer.tsx            auto-grow textarea, Enter=send / Shift+Enter=newline, attachments chips, Think toggle, ctx meter, Send/Stop
  useChatStream.ts        wraps invoke chat_send + Channel; accumulates deltas in a ref, flushes to state with requestAnimationFrame (60 fps)
  ConversationList.tsx    grouped by date, search box (FTS), context menu (rename, pin, export, delete)
```
- **Streaming performance:** keep deltas in a mutable ref, render at most once per animation
  frame, and split markdown rendering so completed blocks are memoized (split on blank lines
  outside code fences) and only the last block re-renders.
- Shiki: create one highlighter on first use with the languages that appear (dynamic
  `loadLanguage`). Until it's ready, show plain monospace.

## Steps
1. Store: migration 0001 (DATA-AND-IPC §1) + repos + tests (CRUD, FTS search, cascade delete).
2. Agent (minimal): preamble + `nexus.toml` prompt rendering (minijinja), budget with `/tokenize`
   and cache, chat workflow, title job, history summary job. Unit tests with a mock `Scheduler` trait.
3. Commands + ChatEvent Channel + persistence + stop/regenerate/edit-resend.
4. UI: list, view, composer, markdown, thoughts, stats, empty state with starters from `nexus.toml`.
5. Attachments v1: drop or pick text files (≤ 256 KB, otherwise truncated with a visible note).
6. Export conversation to Markdown (save dialog).
7. Polish pass: scroll behavior, focus management, keyboard shortcuts ⌘N ⌘. ⌘↑ ⌘⇧C.

## Exit criteria
- [ ] TTFT < 1 s, smooth 60 fps streaming (check with the Performance panel), stop < 200 ms.
- [ ] Thinking toggle works per spike S1 (or is hidden when unsupported).
- [ ] 50-turn conversation: the context meter is correct, older turns are summarized, no overflow errors.
- [ ] Restart keeps everything. Search finds words in old messages.
- [ ] Tests: store repos, budget, SSE→ChatEvent mapping, title fallback.

## Pitfalls
- Tauri `Channel` messages are ordered, but don't send one per token if the UI can't keep up. Coalesce deltas in Rust every 16 ms or 32 chars.
- Markdown injection: never render raw HTML from the model (`react-markdown` default `skipHtml`). Open links via the opener plugin only.
