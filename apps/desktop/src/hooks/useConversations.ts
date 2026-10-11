import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";

import { api } from "@/lib/api";

export const CONVERSATIONS_KEY = ["conversations"] as const;

/** Debounce a fast-changing value (the search box). */
export function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}

/** The conversation list (optionally filtered by full-text search), kept fresh by `conversation:updated`. */
export function useConversations(search: string) {
  const qc = useQueryClient();
  const query = search.trim();
  const list = useQuery({
    queryKey: [...CONVERSATIONS_KEY, "list", query],
    queryFn: () => api.conversations.list(query === "" ? null : query, 100),
    retry: false,
  });
  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | undefined;
    api.conversations
      .onUpdated(() => void qc.invalidateQueries({ queryKey: CONVERSATIONS_KEY }))
      .then((u) => (cancelled ? u() : (off = u)))
      .catch(() => {
        /* no event bus (plain browser) */
      });
    return () => {
      cancelled = true;
      off?.();
    };
  }, [qc]);
  return list;
}
