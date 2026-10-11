import { useEffect, type ReactNode } from "react";

import { applyTheme, resolveTheme, systemPrefersDark } from "@/lib/theme";
import { useUi } from "@/stores/ui";

/** Keeps <html data-theme / data-accent> in sync with the UI store and the OS setting. */
export function ThemeProvider({ children }: { children: ReactNode }) {
  const theme = useUi((s) => s.theme);
  const accent = useUi((s) => s.accent);

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
