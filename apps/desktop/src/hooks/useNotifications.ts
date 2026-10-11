import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { api } from "@/lib/api";

export const NOTIFICATIONS_KEY = ["notifications"] as const;

/** Notification list and unread count, refreshed whenever `notification:new` arrives. */
export function useNotifications(limit = 50) {
  const qc = useQueryClient();
  const list = useQuery({ queryKey: [...NOTIFICATIONS_KEY, "list", limit], queryFn: () => api.notifications.list(limit), retry: false });
  const unread = useQuery({ queryKey: [...NOTIFICATIONS_KEY, "unread"], queryFn: api.notifications.unreadCount, retry: false });
  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | undefined;
    api.notifications
      .onNew(() => void qc.invalidateQueries({ queryKey: NOTIFICATIONS_KEY }))
      .then((unsubscribe) => {
        if (cancelled) unsubscribe();
        else off = unsubscribe;
      })
      .catch(() => {
        /* no event bus (plain browser) */
      });
    return () => {
      cancelled = true;
      off?.();
    };
  }, [qc]);
  return { items: list.data ?? [], unread: unread.data ?? 0 };
}
