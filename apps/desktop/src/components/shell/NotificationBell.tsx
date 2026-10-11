import { useMutation, useQueryClient } from "@tanstack/react-query";
import { AlertCircle, Bell, CheckCircle2, Info, TriangleAlert } from "lucide-react";
import { useState } from "react";
import { useNavigate } from "react-router-dom";

import type { NotificationLevel, NotificationPayload } from "@/bindings";
import { Button, IconButton, Popover, PopoverContent, PopoverTrigger } from "@/components/ui";
import { NOTIFICATIONS_KEY, useNotifications } from "@/hooks/useNotifications";
import { api } from "@/lib/api";
import { cn } from "@/lib/cn";
import { relativeTime } from "@/lib/format";

const LEVEL = {
  info: { Icon: Info, tone: "text-info" },
  success: { Icon: CheckCircle2, tone: "text-success" },
  warning: { Icon: TriangleAlert, tone: "text-warning" },
  error: { Icon: AlertCircle, tone: "text-danger" },
} satisfies Record<NotificationLevel, { Icon: typeof Info; tone: string }>;

function Item({ n, onOpen }: { n: NotificationPayload; onOpen: (n: NotificationPayload) => void }) {
  const { Icon, tone } = LEVEL[n.level];
  return (
    <li className={cn("flex gap-3 rounded-card px-3 py-2.5", !n.read && "bg-surface-2")}>
      <Icon className={cn("mt-0.5 size-4 shrink-0", tone)} />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-2">
          <span className="truncate text-body font-medium">{n.title}</span>
          <span className="shrink-0 text-caption text-faint">{relativeTime(n.created_at)}</span>
        </div>
        {n.body ? <p className="line-clamp-2 text-small text-muted">{n.body}</p> : null}
        {n.action ? (
          <button type="button" className="mt-1 text-small font-medium text-accent hover:underline" onClick={() => onOpen(n)}>
            {n.action.label}
          </button>
        ) : null}
      </div>
    </li>
  );
}

/** Bell with an unread badge; the popover lists notifications (UI-DESIGN §4.9). */
export function NotificationBell() {
  const { items, unread } = useNotifications();
  const qc = useQueryClient();
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const refresh = () => void qc.invalidateQueries({ queryKey: NOTIFICATIONS_KEY });
  const markAll = useMutation({ mutationFn: () => api.notifications.markRead([]), onSuccess: refresh });
  const clear = useMutation({ mutationFn: api.notifications.clear, onSuccess: refresh });
  const sendTest = useMutation({ mutationFn: api.notifications.sendTest });

  const openItem = (n: NotificationPayload) => {
    if (!n.action) return;
    void api.notifications.markRead([n.id]).then(refresh);
    setOpen(false);
    void navigate(n.action.route);
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <IconButton aria-label={unread > 0 ? `Notifications, ${unread} unread` : "Notifications"} size="sm" className="relative">
          <Bell />
          {unread > 0 ? (
            <span className="absolute -top-0.5 -right-0.5 grid min-w-4 place-items-center rounded-full bg-accent px-1 text-[10px] leading-4 font-semibold text-accent-fg">
              {unread > 99 ? "99+" : unread}
            </span>
          ) : null}
        </IconButton>
      </PopoverTrigger>
      <PopoverContent className="w-[360px] p-0">
        <div className="flex items-center justify-between border-b border-border px-3 py-2">
          <span className="text-body font-semibold">Notifications</span>
          <div className="flex gap-1">
            <Button size="sm" variant="ghost" disabled={unread === 0} onClick={() => markAll.mutate()}>
              Mark all read
            </Button>
            <Button size="sm" variant="ghost" disabled={items.length === 0} onClick={() => clear.mutate()}>
              Clear
            </Button>
          </div>
        </div>
        {items.length === 0 ? (
          <p className="p-6 text-center text-small text-muted">You're all caught up.</p>
        ) : (
          <ul className="max-h-96 overflow-y-auto p-1">
            {items.map((n) => (
              <Item key={n.id} n={n} onOpen={openItem} />
            ))}
          </ul>
        )}
        {import.meta.env.DEV ? (
          <div className="border-t border-border p-2">
            <Button size="sm" variant="ghost" onClick={() => sendTest.mutate()}>
              Send test notification
            </Button>
          </div>
        ) : null}
      </PopoverContent>
    </Popover>
  );
}
