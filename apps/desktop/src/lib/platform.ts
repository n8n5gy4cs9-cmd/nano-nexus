export type Platform = "macos" | "windows" | "linux";

/** Map Rust's `std::env::consts::OS` (or a user-agent string) to a platform. */
export function toPlatform(osOrUserAgent: string): Platform {
  const s = osOrUserAgent.toLowerCase();
  if (s.includes("mac") || s.includes("darwin")) return "macos";
  if (s.includes("win")) return "windows";
  return "linux";
}

export function detectPlatform(): Platform {
  return toPlatform(typeof navigator === "undefined" ? "" : navigator.userAgent);
}

/** Label of the primary modifier key: ⌘ on macOS, Ctrl elsewhere. */
export function modLabel(p: Platform): string {
  return p === "macos" ? "⌘" : "Ctrl";
}

/** Does this key event carry the platform's primary modifier? */
export function hasMod(e: Pick<KeyboardEvent, "metaKey" | "ctrlKey">, p: Platform): boolean {
  return p === "macos" ? e.metaKey : e.ctrlKey;
}
