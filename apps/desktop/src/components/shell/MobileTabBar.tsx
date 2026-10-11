import { History, MessageSquare, MoreHorizontal, Sparkles, FileText } from "lucide-react";
import { NavLink } from "react-router-dom";

import { cn } from "@/lib/cn";
import { useUi } from "@/stores/ui";

const base =
  "flex min-h-11 min-w-11 flex-1 flex-col items-center justify-center gap-0.5 text-caption text-muted";

/** Phone navigation: Chat · Assistants · History · Reports · More (hidden from 768 px up). */
export function MobileTabBar() {
  const toggleSidebar = useUi((s) => s.toggleSidebar);
  const link = ({ isActive }: { isActive: boolean }) => cn(base, isActive && "text-text");
  return (
    <nav
      aria-label="Tabs"
      className="flex shrink-0 border-t border-border bg-surface pb-[env(safe-area-inset-bottom)] md:hidden"
    >
      <NavLink to="/" end className={link}>
        <MessageSquare className="size-5" />
        Chat
      </NavLink>
      <NavLink to="/assistants" className={link}>
        <Sparkles className="size-5" />
        Assistants
      </NavLink>
      <button type="button" className={base} onClick={toggleSidebar}>
        <History className="size-5" />
        History
      </button>
      <NavLink to="/analyzer" className={link}>
        <FileText className="size-5" />
        Reports
      </NavLink>
      <NavLink to="/settings" className={link}>
        <MoreHorizontal className="size-5" />
        More
      </NavLink>
    </nav>
  );
}
