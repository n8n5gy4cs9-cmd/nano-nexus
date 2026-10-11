import { useEffect } from "react";
import { useNavigate } from "react-router-dom";

import { focusComposer } from "@/lib/focus";
import { hasMod, type Platform } from "@/lib/platform";
import { NAV, SETTINGS_SECTION } from "@/routes";
import { useAssistantChoice } from "@/stores/assistant";
import { useUi } from "@/stores/ui";

/** Global shortcuts: ⌘N new chat, ⌘1…⌘8 sections, ⌘, settings, ⌘/ assistants, ⌘\ sidebar, ⌘I inspector (Ctrl on Windows/Linux). */
export function useShortcuts(platform: Platform) {
  const navigate = useNavigate();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!hasMod(e, platform) || e.altKey || e.shiftKey) return;
      if (e.key.toLowerCase() === "n") {
        e.preventDefault();
        void navigate("/");
        focusComposer();
        return;
      }
      const ui = useUi.getState();
      const section = NAV.find((s) => s.digit !== undefined && String(s.digit) === e.key);
      if (section) {
        e.preventDefault();
        void navigate(section.path);
      } else if (e.key === ",") {
        e.preventDefault();
        void navigate(SETTINGS_SECTION.path);
      } else if (e.key === "/") {
        e.preventDefault();
        useAssistantChoice.getState().setSwitcherOpen(true);
      } else if (e.key === "\\") {
        e.preventDefault();
        ui.toggleSidebar();
      } else if (e.key.toLowerCase() === "i") {
        e.preventDefault();
        ui.toggleInspector();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [navigate, platform]);
}
