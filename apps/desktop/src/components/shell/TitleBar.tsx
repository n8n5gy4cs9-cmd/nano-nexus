import { PanelLeft, PanelRight, Search } from "lucide-react";

import { IconButton, Kbd } from "@/components/ui";
import { cn } from "@/lib/cn";
import { modLabel, type Platform } from "@/lib/platform";
import { useUi } from "@/stores/ui";

/**
 * Top bar of the main column. On macOS it is the overlay title bar (drag region, 44 px); on Windows
 * it is a 40 px toolbar under the native title bar. The pills on the right arrive with their features
 * (runtime pill, share pill, notification bell).
 */
export function TitleBar({ platform, right }: { platform: Platform; right?: React.ReactNode }) {
  const toggleSidebar = useUi((s) => s.toggleSidebar);
  const toggleInspector = useUi((s) => s.toggleInspector);
  const inspectorOpen = useUi((s) => s.inspectorOpen);
  return (
    <header
      data-tauri-drag-region
      className={cn("flex shrink-0 items-center gap-2 border-b border-border px-3", platform === "macos" ? "h-11" : "h-10")}
    >
      <IconButton aria-label="Toggle sidebar" size="sm" onClick={toggleSidebar}>
        <PanelLeft />
      </IconButton>
      <button
        type="button"
        className="mx-auto flex h-7 w-full max-w-sm items-center gap-2 rounded-button border border-border bg-surface-2 px-2.5 text-small text-faint hover:text-muted"
      >
        <Search className="size-3.5" />
        <span className="flex-1 text-left">Search…</span>
        <Kbd>{modLabel(platform)}K</Kbd>
      </button>
      <div className="flex items-center gap-1">{right}</div>
      <IconButton aria-label="Toggle inspector" size="sm" aria-pressed={inspectorOpen} onClick={toggleInspector}>
        <PanelRight />
      </IconButton>
    </header>
  );
}
