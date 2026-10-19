import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import type { ObserveView } from "@/bindings";
import { api } from "@/lib/api";

export const OBSERVE_KEY = ["observe"] as const;

/**
 * The live Activity picture. While the page is mounted the backend polls every 2 s and pushes
 * `observe:snapshot`; the first value comes from `observe_snapshot`.
 */
export function useObserve() {
  const qc = useQueryClient();
  const query = useQuery({ queryKey: OBSERVE_KEY, queryFn: api.observe.snapshot, retry: false });
  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | undefined;
    void api.observe.watch(true).catch(() => {
      /* plain browser: no backend */
    });
    api.observe
      .onSnapshot((v) => qc.setQueryData<ObserveView>(OBSERVE_KEY, v))
      .then((unsubscribe) => {
        if (cancelled) unsubscribe();
        else off = unsubscribe;
      })
      .catch(() => {
        /* no event bus */
      });
    return () => {
      cancelled = true;
      off?.();
      void api.observe.watch(false).catch(() => {
        /* ignore */
      });
    };
  }, [qc]);
  return query;
}
