import { useEffect } from "react";
import { Outlet } from "react-router-dom";

import { AssistantSwitcher } from "@/features/assistants/AssistantSwitcher";
import { PermissionModal } from "@/features/permissions/PermissionModal";
import { useShortcuts } from "@/hooks/useShortcuts";
import { usePlatform } from "@/hooks/usePlatform";
import { useActivityBridge } from "@/hooks/useActivityBridge";
import { useActivity } from "@/stores/activity";
import { useUi } from "@/stores/ui";
import { Terminal } from "lucide-react";
import { useLocation } from "react-router-dom";
import { IconButton } from "@/components/ui";

import { IS_WEB } from "@/lib/target";

import { ConsolePanel } from "./ConsolePanel";
import { Inspector } from "./Inspector";
import { MobileTabBar } from "./MobileTabBar";
import { NotificationBell } from "./NotificationBell";
import { RuntimePill } from "./RuntimePill";
import { SharePill } from "@/features/share/SharePill";
import { Sidebar } from "./Sidebar";
import { TitleBar } from "./TitleBar";

/** Window layout: sidebar | (title bar + page) | optional inspector. */
export function AppShell() {
  const platform = usePlatform();
  const sidebarOpen = useUi((s) => s.sidebarOpen);
  const inspectorOpen = useUi((s) => s.inspectorOpen);
  useShortcuts(platform);
  useActivityBridge();
  const consoleOpen = useActivity((s) => s.open);
  const location = useLocation();
  // On narrow windows the sidebar is a drawer: close it after navigating.
  useEffect(() => {
    if (window.innerWidth < 768) useUi.setState({ sidebarOpen: false });
  }, [location.pathname]);
  // CSS keys off this (macOS: transparent window so the native vibrancy shows through the sidebar).
  useEffect(() => {
    document.documentElement.dataset.platform = platform;
  }, [platform]);
  return (
    <div className="flex h-full text-text">
      {sidebarOpen ? (
        <>
          <button
            type="button"
            aria-label="Close sidebar"
            className="fixed inset-0 z-30 bg-black/40 md:hidden"
            onClick={() => useUi.setState({ sidebarOpen: false })}
          />
          <Sidebar platform={platform} />
        </>
      ) : null}
      <div className="flex min-w-0 flex-1 flex-col bg-bg">
        <TitleBar platform={platform} right={
            <>
              <IconButton aria-label="Toggle activity console" size="sm" aria-pressed={consoleOpen} onClick={() => useActivity.getState().toggle()}>
                <Terminal />
              </IconButton>
              <NotificationBell />
              {IS_WEB ? null : <SharePill />}
              <RuntimePill />
            </>
          } />
        <main className="min-h-0 flex-1 overflow-auto">
          <Outlet />
        </main>
        {consoleOpen ? <ConsolePanel /> : null}
        <MobileTabBar />
      </div>
      {inspectorOpen ? <Inspector /> : null}
      <PermissionModal />
      <AssistantSwitcher />
    </div>
  );
}
