import { useEffect } from "react";

import type { RuntimeState } from "@/bindings";
import { getTransport } from "@/lib/transport";
import { logActivity } from "@/stores/activity";

/** Feed global backend events (runtime state, model and change-set updates) into the console. */
export function useActivityBridge(): void {
  useEffect(() => {
    const offs: Array<() => void> = [];
    let alive = true;
    const t = getTransport();
    const add = (p: Promise<() => void>) =>
      void p.then((off) => (alive ? offs.push(off) : off())).catch(() => undefined);
    add(t.on<RuntimeState>("runtime:status", (s) => logActivity(s.state === "failed" ? "error" : "runtime", `runtime: ${s.state}`)));
    add(t.on<string>("model:changed", (id) => logActivity("runtime", `active model: ${id}`)));
    add(t.on<{ id: string; status: string }>("changeset:update", (u) => logActivity("runtime", `change set ${u.id.slice(0, 8)} ${u.status}`)));
    return () => {
      alive = false;
      offs.forEach((off) => off());
    };
  }, []);
}
