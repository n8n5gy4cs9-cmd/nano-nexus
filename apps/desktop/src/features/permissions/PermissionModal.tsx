import { ChevronDown, ShieldQuestion } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";

import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";

import { ACCESS_VERB, LEVEL_LABEL, type Decision, type GrantLevel, type PermissionRequest } from "./types";

const LEVELS: GrantLevel[] = ["read", "read_write", "full"];

/** Lowest level that covers the access the assistant asked for. */
function minLevel(r: PermissionRequest): GrantLevel {
  if (r.needed === "read") return "read";
  return r.needed === "write" ? "read_write" : "full";
}

/** Queue of open permission prompts: shows the oldest, answers it, then the next. */
export function usePermissionQueue() {
  const [queue, setQueue] = useState<PermissionRequest[]>([]);
  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | undefined;
    const add = (r: PermissionRequest) => setQueue((q) => (q.some((x) => x.id === r.id) ? q : [...q, r]));
    api.permissions
      .onRequest(add)
      .then((u) => {
        if (cancelled) u();
        else off = u;
      })
      .catch(() => {
        /* no event bus (plain browser) */
      });
    // Re-show prompts that were open before a reload.
    api.permissions
      .pending()
      .then((list) => list.forEach(add))
      .catch(() => {});
    return () => {
      cancelled = true;
      off?.();
    };
  }, []);
  const answer = useCallback(async (id: string, decision: Decision) => {
    try {
      await api.permissions.respond(id, decision);
    } catch (e) {
      toast.error(toIpcException(e).message);
    } finally {
      setQueue((q) => q.filter((r) => r.id !== id));
    }
  }, []);
  return { current: queue[0] ?? null, waiting: Math.max(0, queue.length - 1), answer };
}

/** "Folder Scout wants to read ~/Downloads": Deny, Allow once, or Allow this folder (UI-DESIGN §4.8). */
export function PermissionModal() {
  const { current, waiting, answer } = usePermissionQueue();
  if (!current) return null;
  const floor = minLevel(current);
  const allowed = LEVELS.filter((l) => LEVELS.indexOf(l) >= LEVELS.indexOf(floor));
  return (
    <Dialog open onOpenChange={(open) => !open && void answer(current.id, { decision: "deny" })}>
      <DialogContent aria-describedby="perm-desc">
        <div className="flex items-start gap-3">
          <ShieldQuestion className="mt-0.5 size-6 shrink-0 text-accent" />
          <div className="min-w-0">
            <DialogTitle>
              {current.requested_by ? `${current.requested_by} (via ${current.assistant})` : current.assistant} wants to{" "}
              {ACCESS_VERB[current.needed]}
            </DialogTitle>
            <DialogDescription id="perm-desc">
              <span className="block truncate rounded-chip bg-surface-2 px-2 py-1 font-mono text-small text-text" title={current.path}>
                {current.path}
              </span>
              {current.reason ? <span className="mt-2 block">{current.reason}</span> : null}
            </DialogDescription>
          </div>
        </div>
        <DialogFooter className="items-center">
          {waiting > 0 ? <span className="mr-auto text-small text-faint">{waiting} more waiting</span> : null}
          <Button onClick={() => void answer(current.id, { decision: "deny" })}>Deny</Button>
          <Button onClick={() => void answer(current.id, { decision: "allow_once" })}>Allow once</Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="primary">
                Allow folder <ChevronDown />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {allowed.map((level) => (
                <DropdownMenuItem key={level} onSelect={() => void answer(current.id, { decision: "allow_folder", level })}>
                  {LEVEL_LABEL[level]}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
