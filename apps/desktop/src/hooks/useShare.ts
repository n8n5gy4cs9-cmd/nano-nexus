import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import type { ShareStatus } from "@/features/share/types";
import { api } from "@/lib/api";

export const SHARE_KEY = ["share-status"] as const;
export const SHARE_DEVICES_KEY = ["share-devices"] as const;

/** Live LAN Share status: one fetch, then pushed `share:status` updates. */
export function useShareStatus(): ShareStatus | undefined {
  const qc = useQueryClient();
  const query = useQuery({ queryKey: SHARE_KEY, queryFn: api.share.status, retry: false, staleTime: Infinity });
  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | undefined;
    api.share
      .onStatus((s) => {
        qc.setQueryData(SHARE_KEY, s);
        void qc.invalidateQueries({ queryKey: SHARE_DEVICES_KEY });
      })
      .then((unsubscribe) => {
        if (cancelled) unsubscribe();
        else off = unsubscribe;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      off?.();
    };
  }, [qc]);
  return query.data;
}
