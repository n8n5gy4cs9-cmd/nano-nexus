/** What a chat-footer plugin can see. Add fields here when a new plugin needs more. */
export interface ChatFooterContext {
  /** Path of the model that is running, or undefined when none is ready. */
  modelPath: string | undefined;
  /** Tokens per second of the last answer, or null. */
  lastTps: number | null;
}

/**
 * A plugin is a small add-on that adds one simple thing to the app view. Today there is one slot, the
 * quiet line under the chat box: `footer` returns a short piece of text, or null to show nothing.
 * Plugins are plain functions (no network, no file access), so they are safe to add.
 */
export interface Plugin {
  /** Stable id, kebab-case. Stored in `plugins.disabled` when the user turns the plugin off. */
  id: string;
  name: string;
  /** One sentence shown in Settings → Plugins. */
  description: string;
  footer: (ctx: ChatFooterContext) => string | null;
}
