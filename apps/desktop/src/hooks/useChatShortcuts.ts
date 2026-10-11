import { useEffect } from "react";

import { hasMod, type Platform } from "@/lib/platform";

export interface ChatShortcutActions {
  streaming: boolean;
  stop: () => void;
  /** Edit the last user message (only offered when the composer is empty). */
  editLast: (() => void) | undefined;
  copyLastAnswer: (() => void) | undefined;
  composerEmpty: boolean;
}

/** Chat shortcuts: Esc or mod+. stop, mod+Up edit last message (empty composer), mod+Shift+C copy last answer. */
export function useChatShortcuts(platform: Platform, a: ChatShortcutActions) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && a.streaming) {
        e.preventDefault();
        a.stop();
        return;
      }
      if (!hasMod(e, platform)) return;
      if (e.key === "." && a.streaming) {
        e.preventDefault();
        a.stop();
      } else if (e.key === "ArrowUp" && a.composerEmpty && a.editLast) {
        e.preventDefault();
        a.editLast();
      } else if (e.shiftKey && e.key.toLowerCase() === "c" && a.copyLastAnswer) {
        e.preventDefault();
        a.copyLastAnswer();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [platform, a]);
}
