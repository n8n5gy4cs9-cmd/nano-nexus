import type { HighlighterGeneric } from "shiki";

import { normalizeLanguage } from "./markdown-blocks";

type Highlighter = HighlighterGeneric<string, string>;

const THEMES = { dark: "github-dark-default", light: "github-light-default" } as const;
let highlighter: Promise<Highlighter> | null = null;
const failed = new Set<string>();

/** One lazy Shiki highlighter for the whole app; the bundle loads on first use. */
async function getHighlighter(): Promise<Highlighter> {
  highlighter ??= import("shiki").then((s) =>
    s.createHighlighter({ themes: [THEMES.dark, THEMES.light], langs: [] }),
  ) as Promise<Highlighter>;
  return highlighter;
}

/** Highlighted HTML for `code`, or `null` when the language is unknown or Shiki is unavailable. */
export async function highlight(code: string, lang: string | undefined, theme: "dark" | "light"): Promise<string | null> {
  const id = normalizeLanguage(lang);
  if (id === "text" || failed.has(id)) return null;
  try {
    const h = await getHighlighter();
    if (!h.getLoadedLanguages().includes(id)) await h.loadLanguage(id);
    return h.codeToHtml(code, { lang: id, theme: THEMES[theme] });
  } catch {
    failed.add(id);
    return null;
  }
}
