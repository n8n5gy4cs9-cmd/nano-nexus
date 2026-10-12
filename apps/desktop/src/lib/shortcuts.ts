import type { Platform } from "./platform";

/** One keyboard shortcut as the Settings → Shortcuts viewer lists it (UI-DESIGN §5). */
export interface ShortcutInfo {
  /** Keys after the primary modifier, for example `Shift+T`; a leading `!` marks a key without it. */
  keys: string;
  action: string;
  /** Where it works. */
  where: "Anywhere" | "Chat" | "Editor";
  /** The key is the bare key, with no primary modifier (for example Esc). */
  bare?: boolean;
}

export const SHORTCUTS: ShortcutInfo[] = [
  { keys: "K", action: "Search and commands", where: "Anywhere" },
  { keys: "N", action: "New chat", where: "Anywhere" },
  { keys: "/", action: "Switch assistant", where: "Anywhere" },
  { keys: "1…9", action: "Go to Chat, Assistants, Models, Analyzer, Rename Forge, Editor, Changes, Permissions, Library", where: "Anywhere" },
  { keys: ",", action: "Settings", where: "Anywhere" },
  { keys: "\\", action: "Show or hide the sidebar", where: "Anywhere" },
  { keys: "I", action: "Show or hide the inspector", where: "Anywhere" },
  { keys: "J", action: "Show or hide the activity console", where: "Anywhere" },
  { keys: "Shift+T", action: "Turn “Think harder” on or off", where: "Chat" },
  { keys: ".", action: "Stop the answer", where: "Chat" },
  { keys: "Esc", action: "Stop the answer", where: "Chat", bare: true },
  { keys: "↑", action: "Edit your last message (empty message box)", where: "Chat" },
  { keys: "Shift+C", action: "Copy the last answer", where: "Chat" },
  { keys: "Enter", action: "Send the message", where: "Chat", bare: true },
  { keys: "Shift+Enter", action: "New line in the message", where: "Chat", bare: true },
  { keys: "O", action: "Open a file", where: "Editor" },
  { keys: "W", action: "Close the tab", where: "Editor" },
  { keys: "Shift+T", action: "Reopen the closed tab", where: "Editor" },
];

/** "⌘K" on a Mac, "Ctrl+K" elsewhere. */
export function shortcutLabel(s: ShortcutInfo, platform: Platform): string {
  if (s.bare) return s.keys;
  return platform === "macos" ? `⌘${s.keys.replace("Shift+", "⇧")}` : `Ctrl+${s.keys}`;
}
