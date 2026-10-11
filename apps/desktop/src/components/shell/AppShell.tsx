import { Outlet } from "react-router-dom";

import { useShortcuts } from "@/hooks/useShortcuts";
import { usePlatform } from "@/hooks/usePlatform";
import { useUi } from "@/stores/ui";

import { Inspector } from "./Inspector";
import { RuntimePill } from "./RuntimePill";
import { Sidebar } from "./Sidebar";
import { TitleBar } from "./TitleBar";

/** Window layout: sidebar | (title bar + page) | optional inspector. */
export function AppShell() {
  const platform = usePlatform();
  const sidebarOpen = useUi((s) => s.sidebarOpen);
  const inspectorOpen = useUi((s) => s.inspectorOpen);
  useShortcuts(platform);
  return (
    <div className="flex h-full bg-bg text-text">
      {sidebarOpen ? <Sidebar platform={platform} /> : null}
      <div className="flex min-w-0 flex-1 flex-col">
        <TitleBar platform={platform} right={<RuntimePill />} />
        <main className="min-h-0 flex-1 overflow-auto">
          <Outlet />
        </main>
      </div>
      {inspectorOpen ? <Inspector /> : null}
    </div>
  );
}
