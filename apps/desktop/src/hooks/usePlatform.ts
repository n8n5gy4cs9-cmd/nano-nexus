import { useQuery } from "@tanstack/react-query";

import { api } from "@/lib/api";
import { IS_WEB } from "@/lib/target";
import { detectPlatform, toPlatform, type Platform } from "@/lib/platform";

/** The OS the app runs on: from the backend when available, else guessed from the user agent. */
export function usePlatform(): Platform {
  // A phone or another computer in the browser gets neutral styling (no macOS title bar padding or transparency).
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.app.info, staleTime: Infinity, retry: false });
  if (IS_WEB) return "linux";
  return info.data ? toPlatform(info.data.os) : detectPlatform();
}
