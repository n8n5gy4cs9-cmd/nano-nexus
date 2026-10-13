# Plugins (simple add-ons for the app view)

A plugin adds one small, simple thing to the app. Today there is one place a plugin can show something:
the quiet line under the chat box. The first two plugins are **Model name** and **Speed**
(tokens per second of the last answer).

Users turn plugins on or off in **Settings → Plugins**. Off plugins are stored in the setting
`plugins.disabled` (a comma-separated list of ids).

## Add a plugin in 3 steps
1. Open `apps/desktop/src/plugins/builtin.ts` (or make a new file next to it) and write:

```ts
export const contextPlugin: Plugin = {
  id: "context-size",                       // kebab-case, never change it later
  name: "Context size",                     // shown in Settings → Plugins
  description: "Shows the context window.", // one sentence
  footer: (ctx) => (ctx.lastTps ? "ctx 8192" : null), // short text, or null to show nothing
};
```

2. Add it to the `PLUGINS` list in `apps/desktop/src/plugins/index.ts`. The order there is the order on screen.
3. Need more information than `modelPath` and `lastTps`? Add a field to `ChatFooterContext` in
   `plugins/types.ts` and fill it in `features/chat/ChatView.tsx`.

That is all: the Settings toggle and the footer line are generated from the list.

## Rules
- A plugin is a plain function. It gets no network, no files, and no ability to run commands.
- Return `null` to show nothing. A plugin that throws is skipped; it never breaks the chat.
- Plugins are part of the app source (added at build time), not downloaded or loaded at runtime. That keeps
  them safe and simple. A runtime plugin folder would need its own permission design (see
  `docs/architecture/PERMISSIONS.md`), so it is not built.

Tests: `apps/desktop/src/plugins/plugins.test.ts`.
