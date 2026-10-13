import { modelNamePlugin, speedPlugin } from "./builtin";
import type { ChatFooterContext, Plugin } from "./types";

export type { ChatFooterContext, Plugin } from "./types";

/**
 * Every plugin the app knows. To add one: write a `Plugin` (see builtin.ts) and add it to this list.
 * The order here is the order on screen. See docs/PLUGINS.md.
 */
export const PLUGINS: readonly Plugin[] = [modelNamePlugin, speedPlugin];

/** The `plugins.disabled` setting is a comma-separated list of ids. */
export function parseDisabled(value: unknown): Set<string> {
  if (typeof value !== "string") return new Set();
  return new Set(value.split(",").map((s) => s.trim()).filter(Boolean));
}

export function serializeDisabled(ids: Iterable<string>): string {
  return [...new Set(ids)].sort().join(",");
}

/** Footer text from every enabled plugin, in order. A plugin that throws is skipped, never crashes the chat. */
export function footerParts(ctx: ChatFooterContext, disabled: ReadonlySet<string>, plugins: readonly Plugin[] = PLUGINS): string[] {
  const out: string[] = [];
  for (const p of plugins) {
    if (disabled.has(p.id)) continue;
    try {
      const text = p.footer(ctx);
      if (text) out.push(text);
    } catch {
      /* a broken plugin must not break the chat */
    }
  }
  return out;
}
