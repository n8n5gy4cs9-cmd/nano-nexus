import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import type { RuntimeState } from "@/bindings";
import { api } from "@/lib/api";

export const RUNTIME_KEY = ["runtime"] as const;

/** Live runtime state: one initial fetch, then pushed updates from `runtime:status`. */
export function useRuntime(): RuntimeState | undefined {
  const qc = useQueryClient();
  const query = useQuery({ queryKey: RUNTIME_KEY, queryFn: api.runtime.status, retry: false, staleTime: Infinity });
  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | undefined;
    api.runtime
      .onStatus((s) => qc.setQueryData(RUNTIME_KEY, s))
      .then((unsubscribe) => {
        if (cancelled) unsubscribe();
        else off = unsubscribe;
      })
      .catch(() => {
        /* no event bus (plain browser): the initial fetch is all we get */
      });
    return () => {
      cancelled = true;
      off?.();
    };
  }, [qc]);
  return query.data;
}
