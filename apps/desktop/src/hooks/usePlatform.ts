import { useQuery } from "@tanstack/react-query";

import { api } from "@/lib/api";
import { detectPlatform, toPlatform, type Platform } from "@/lib/platform";

/** The OS the app runs on: from the backend when available, else guessed from the user agent. */
export function usePlatform(): Platform {
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.app.info, staleTime: Infinity, retry: false });
  return info.data ? toPlatform(info.data.os) : detectPlatform();
}
