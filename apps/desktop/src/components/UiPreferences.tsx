import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { toast } from "sonner";

import { api } from "@/lib/api";
import { IS_WEB } from "@/lib/target";
import { isAccent, isThemePreference } from "@/lib/theme";
import { parseZoom } from "@/lib/zoom";
import { useAssistantChoice } from "@/stores/assistant";
import { useUi } from "@/stores/ui";

type Preference = "ui.theme" | "ui.accent" | "ui.zoom" | "ui.default_assistant";
let started = false;

/** One desktop-lifetime subscription, including under React StrictMode remounts. */
function start(client: QueryClient): void {
  if (started || IS_WEB) return;
  started = true;
  const dirty = new Set<Preference>();
  let hydrating = false;
  // Reads precede writes; do not overwrite changes made while the initial read is pending.
  let queue: Promise<unknown> = client.fetchQuery({
    queryKey: ["settings"], queryFn: api.settings.get, retry: false,
  }).then((settings) => {
    const saved: Record<string, unknown> = { ...settings };
    const patch: Partial<Pick<ReturnType<typeof useUi.getState>, "theme" | "accent" | "zoom">> = {};
    const theme = saved["ui.theme"];
    const accent = saved["ui.accent"];
    if (!dirty.has("ui.theme") && isThemePreference(theme)) patch.theme = theme;
    if (!dirty.has("ui.accent") && isAccent(accent)) patch.accent = accent;
    if (!dirty.has("ui.zoom")) patch.zoom = parseZoom(saved["ui.zoom"]);
    hydrating = true;
    try {
      useUi.setState(patch);
      const id = saved["ui.default_assistant"];
      if (!dirty.has("ui.default_assistant") && typeof id === "string" && id.length > 0 && id.length <= 128) {
        useAssistantChoice.setState({ selectedId: id });
      }
    } finally {
      hydrating = false;
    }
  }).catch(() => {
    toast.error("Saved UI preferences could not be loaded. Current choices remain available for this session.");
  });

  const save = (key: Preference, value: string | number) => {
    if (hydrating) return;
    dirty.add(key);
    // Serialize rapid changes so a slower earlier request cannot overwrite the latest choice.
    queue = queue.then(async () => {
      await api.settings.set(key, value);
      await client.invalidateQueries({ queryKey: ["settings"] });
    }).catch(() => {
      toast.error("UI preference was not saved. Change it again to retry; this session keeps your choice.");
    });
  };
  useUi.subscribe((next, previous) => {
    if (next.theme !== previous.theme) save("ui.theme", next.theme);
    if (next.accent !== previous.accent) save("ui.accent", next.accent);
    if (next.zoom !== previous.zoom) save("ui.zoom", next.zoom);
  });
  useAssistantChoice.subscribe((next, previous) => {
    if (next.selectedId !== previous.selectedId) save("ui.default_assistant", next.selectedId);
  });
}

/** Desktop preferences persist in SQLite; LAN clients retain their own browser preferences. */
export function UiPreferences() {
  const client = useQueryClient();
  useEffect(() => start(client), [client]);
  return null;
}
