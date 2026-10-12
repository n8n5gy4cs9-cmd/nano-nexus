import { Check, ChevronDown, ChevronRight, X } from "lucide-react";
import { useState } from "react";

import { Badge, Button, IconButton, Input } from "@/components/ui";
import { cn } from "@/lib/cn";

import { type ItemStatus, type RenameItem, chosenName } from "./types";
import { UsagesList } from "./UsagesList";

const STATUS_TONE: Record<ItemStatus, "neutral" | "accent" | "success" | "warning" | "danger"> = {
  candidate: "neutral",
  proposed: "accent",
  accepted: "success",
  rejected: "neutral",
  needs_manual: "warning",
  applied: "success",
};

const STATUS_LABEL: Record<ItemStatus, string> = {
  candidate: "No proposal",
  proposed: "Proposed",
  accepted: "Accepted",
  rejected: "Rejected",
  needs_manual: "Needs you",
  applied: "Applied",
};

/** Rows shown before "Show more" (the list can hold thousands of symbols). */
const PAGE = 200;

function Row({
  item,
  sessionId,
  onEdit,
  onStatus,
  onStrings,
  onRetry,
}: {
  item: RenameItem;
  sessionId: string;
  onEdit: (item: RenameItem, name: string) => void;
  onStatus: (item: RenameItem, status: ItemStatus) => void;
  onStrings: (item: RenameItem, on: boolean) => void;
  onRetry: (item: RenameItem) => void;
}) {
  const [open, setOpen] = useState(false);
  const name = chosenName(item) ?? "";
  const [draft, setDraft] = useState(name);
  const [seenName, setSeenName] = useState(name);
  if (seenName !== name) {
    setSeenName(name);
    setDraft(name);
  }
  const commit = () => {
    if (draft.trim() && draft !== name) onEdit(item, draft.trim());
    else setDraft(name);
  };
  const locked = item.status === "applied";
  return (
    <li className={cn("rounded-card border border-border bg-surface", item.status === "rejected" && "opacity-60")} data-symbol-id={item.symbol_id}>
      <div className="flex flex-wrap items-center gap-2 px-3 py-2">
        <IconButton aria-label={open ? "Hide usages" : "Show usages"} size="sm" onClick={() => setOpen((o) => !o)}>
          {open ? <ChevronDown /> : <ChevronRight />}
        </IconButton>
        <div className="min-w-0 flex-1 basis-56">
          <div className="flex items-center gap-2">
            <code className="truncate text-body font-medium">{item.old_name}</code>
            <span className="text-faint">→</span>
            <Input
              aria-label={`New name for ${item.old_name}`}
              className="h-7 w-48 font-mono"
              value={draft}
              placeholder="no proposal"
              disabled={locked}
              onChange={(e) => setDraft(e.target.value)}
              onBlur={commit}
              onKeyDown={(e) => {
                if (e.key === "Enter") commit();
              }}
            />
          </div>
          <div className="truncate text-small text-muted">
            {item.kind} · {item.path}
            {item.line > 0 ? `:${item.line}` : ""} · {item.usages} usage{item.usages === 1 ? "" : "s"}
          </div>
        </div>
        <Badge tone={STATUS_TONE[item.status]}>{STATUS_LABEL[item.status]}</Badge>
        {item.confidence !== null ? <span className="tnum text-caption text-faint">{Math.round(item.confidence * 100)}%</span> : null}
        {locked ? null : (
          <div className="flex items-center gap-1">
            <IconButton aria-label={`Accept ${item.old_name}`} size="sm" disabled={!name || item.status === "accepted"} onClick={() => onStatus(item, "accepted")}>
              <Check />
            </IconButton>
            <IconButton aria-label={`Reject ${item.old_name}`} size="sm" disabled={item.status === "rejected"} onClick={() => onStatus(item, "rejected")}>
              <X />
            </IconButton>
            <Button size="sm" variant="ghost" onClick={() => onRetry(item)}>
              Try again
            </Button>
          </div>
        )}
      </div>
      {item.error ? <p className="px-3 pb-2 text-small text-danger">{item.error}</p> : null}
      {item.reason || item.issues.length > 0 || item.flags.length > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5 px-3 pb-2 text-small text-muted">
          {item.reason ? <span>{item.reason}</span> : null}
          {item.issues.map((i) => (
            <Badge key={i} tone="warning">
              {i}
            </Badge>
          ))}
          {item.flags.map((f) => (
            <Badge key={f}>{f.replace("_", " ")}</Badge>
          ))}
        </div>
      ) : null}
      {open ? (
        <div className="flex flex-col gap-2 border-t border-border px-3 py-2">
          <label className="flex items-center gap-2 text-small">
            <input type="checkbox" checked={item.include_strings} disabled={locked} onChange={(e) => onStrings(item, e.target.checked)} />
            Also rename text in quotes that matches this name
          </label>
          <UsagesList sessionId={sessionId} symbolId={item.symbol_id} />
        </div>
      ) : null}
    </li>
  );
}

/** The issues grid: one expandable row per symbol, file, or folder with naming problems. */
export function ItemsTable({
  items,
  sessionId,
  onEdit,
  onStatus,
  onStrings,
  onRetry,
}: {
  items: RenameItem[];
  sessionId: string;
  onEdit: (item: RenameItem, name: string) => void;
  onStatus: (item: RenameItem, status: ItemStatus) => void;
  onStrings: (item: RenameItem, on: boolean) => void;
  onRetry: (item: RenameItem) => void;
}) {
  const [shown, setShown] = useState(PAGE);
  if (items.length === 0) return <p className="py-6 text-center text-small text-muted">Nothing here.</p>;
  return (
    <>
      <ul className="flex flex-col gap-1.5">
        {items.slice(0, shown).map((i) => (
          <Row key={i.symbol_id} item={i} sessionId={sessionId} onEdit={onEdit} onStatus={onStatus} onStrings={onStrings} onRetry={onRetry} />
        ))}
      </ul>
      {items.length > shown ? (
        <Button className="mt-2 self-center" onClick={() => setShown((n) => n + PAGE)}>
          Show more ({items.length - shown} left)
        </Button>
      ) : null}
    </>
  );
}
