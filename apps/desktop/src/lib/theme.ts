export type ThemePreference = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";
export const ACCENTS = ["violet", "cyan", "emerald", "amber", "rose", "graphite"] as const;
export type Accent = (typeof ACCENTS)[number];

export function isThemePreference(v: unknown): v is ThemePreference {
  return v === "light" || v === "dark" || v === "system";
}

export function isAccent(v: unknown): v is Accent {
  return typeof v === "string" && (ACCENTS as readonly string[]).includes(v);
}

/** Resolve "system" against the OS preference. */
export function resolveTheme(pref: ThemePreference, systemPrefersDark: boolean): ResolvedTheme {
  if (pref === "system") return systemPrefersDark ? "dark" : "light";
  return pref;
}

/** Write the resolved theme and accent onto <html> (tokens.css keys off these attributes). */
export function applyTheme(root: HTMLElement, theme: ResolvedTheme, accent: Accent): void {
  root.dataset.theme = theme;
  root.dataset.accent = accent;
}

export function systemPrefersDark(): boolean {
  return typeof window !== "undefined" && typeof window.matchMedia === "function" && window.matchMedia("(prefers-color-scheme: dark)").matches;
}
