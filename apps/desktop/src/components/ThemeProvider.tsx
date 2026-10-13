import { useEffect, type ReactNode } from "react";

import { applyTheme, resolveTheme, systemPrefersDark } from "@/lib/theme";
import { applyZoom } from "@/lib/zoom";
import { useUi } from "@/stores/ui";

/** Keeps <html data-theme / data-accent> in sync with the UI store and the OS setting. */
export function ThemeProvider({ children }: { children: ReactNode }) {
  const theme = useUi((s) => s.theme);
  const accent = useUi((s) => s.accent);
  const zoom = useUi((s) => s.zoom);

  useEffect(() => applyZoom(document.documentElement, zoom), [zoom]);

  useEffect(() => {
    const root = document.documentElement;
    const apply = () => applyTheme(root, resolveTheme(theme, systemPrefersDark()), accent);
    apply();
    if (theme !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme, accent]);

  return <>{children}</>;
}
