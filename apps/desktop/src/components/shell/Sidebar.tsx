import { Plus } from "lucide-react";
import { NavLink } from "react-router-dom";

import { cn } from "@/lib/cn";
import { modLabel, type Platform } from "@/lib/platform";
import { NAV, SETTINGS_SECTION, type NavSection } from "@/routes";

function NavItem({ section, platform }: { section: NavSection; platform: Platform }) {
  const Icon = section.icon;
  return (
    <NavLink
      to={section.path}
      end={section.path === "/"}
      className={({ isActive }) =>
        cn(
          "relative flex h-8 items-center gap-2.5 rounded-button px-2.5 text-body text-muted transition-colors duration-[var(--dur-hover)] hover:bg-surface-2 hover:text-text",
          isActive &&
            "bg-surface-3 text-text before:absolute before:inset-y-1.5 before:left-0 before:w-0.5 before:rounded-full before:bg-[image:var(--nexus-gradient)]",
        )
      }
    >
      <Icon className="size-4" />
      <span className="flex-1">{section.label}</span>
      {section.digit ? (
        <span className="text-caption text-faint">
          {modLabel(platform)}
          {section.digit}
        </span>
      ) : null}
    </NavLink>
  );
}

export function Sidebar({ platform }: { platform: Platform }) {
  return (
    <aside className="flex w-[248px] shrink-0 flex-col border-r border-border bg-sidebar" aria-label="Sidebar">
      {/* On macOS the traffic lights sit here, so this strip is also a window drag region. */}
      <div data-tauri-drag-region className={cn("flex h-11 shrink-0 items-center gap-2 px-3", platform === "macos" && "pl-[78px]")}>
        <span aria-hidden className="size-4 rounded-[5px] bg-[image:var(--nexus-gradient)]" />
        <span className="text-body font-semibold">NanoNexus</span>
      </div>
      <div className="px-2 pb-2">
        <button
          type="button"
          className="flex h-8 w-full items-center gap-2 rounded-button border border-border bg-surface px-2.5 text-body text-text shadow-1 transition-colors hover:bg-surface-2"
        >
          <Plus className="size-4" />
          <span className="flex-1 text-left">New chat</span>
          <span className="text-caption text-faint">{modLabel(platform)}N</span>
        </button>
      </div>
      <nav className="flex flex-col gap-0.5 px-2" aria-label="Main">
        {NAV.map((s) => (
          <NavItem key={s.path} section={s} platform={platform} />
        ))}
      </nav>
      <div className="mt-3 flex-1 overflow-y-auto px-4 py-2 text-caption text-faint">Conversations will appear here.</div>
      <div className="border-t border-border p-2">
        <NavItem section={SETTINGS_SECTION} platform={platform} />
      </div>
    </aside>
  );
}
