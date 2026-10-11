import { Library } from "lucide-react";
import { useEffect, useState } from "react";

import { api } from "@/lib/api";
import { cn } from "@/lib/cn";

const KEY = "knowledge.chat_context";

/**
 * Composer chip: "Libraries" is on when `knowledge.chat_context` is set. It only appears when at least
 * one enabled, indexed library exists, so with no libraries the composer looks exactly as before.
 * Plain effects (no query cache) keep it independent of providers.
 */
export function LibrariesChip() {
  const [available, setAvailable] = useState(false);
  const [on, setOn] = useState(false);

  useEffect(() => {
    let alive = true;
    try {
      void Promise.all([api.knowledge.list(), api.settings.get()])
        .then(([libs, settings]) => {
          if (!alive) return;
          setAvailable(libs.some((l) => l.enabled && l.chunk_count > 0));
          setOn((settings as unknown as Record<string, unknown>)[KEY] === true);
        })
        .catch(() => undefined);
    } catch {
      /* no transport (tests, web preview) */
    }
    return () => {
      alive = false;
    };
  }, []);

  if (!available) return null;
  const flip = () => {
    const next = !on;
    setOn(next);
    void api.settings.set(KEY, next).catch(() => setOn(!next));
  };
  return (
    <button
      type="button"
      aria-pressed={on}
      onClick={flip}
      title="Search your offline libraries and cite them in answers"
      className={cn(
        "flex items-center gap-1 rounded-chip border px-2 py-0.5 text-small",
        on ? "border-accent bg-accent/10 text-accent" : "border-border text-muted hover:bg-surface-2",
      )}
    >
      <Library className="size-3.5" /> Libraries
    </button>
  );
}
