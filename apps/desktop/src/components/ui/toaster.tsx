import { Toaster as Sonner } from "sonner";

import { useUi } from "@/stores/ui";

/** Toasts, themed through the tokens. */
export function Toaster() {
  const theme = useUi((s) => s.theme);
  return (
    <Sonner
      theme={theme}
      position="bottom-right"
      toastOptions={{
        style: {
          background: "var(--surface)",
          color: "var(--text)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius-card)",
          boxShadow: "var(--shadow-2)",
        },
      }}
    />
  );
}
