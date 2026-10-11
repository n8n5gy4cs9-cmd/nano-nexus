import { IS_WEB } from "@/lib/target";
import { ArrowUp, Paperclip, Square, X } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";

import type { AttachmentInput } from "@/bindings";

import { Button, IconButton, Switch, Textarea } from "@/components/ui";
import { LibrariesChip } from "@/features/library/LibrariesChip";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";
import { cn } from "@/lib/cn";
import { formatTokens } from "@/lib/tokens";

export interface ComposerProps {
  placeholder: string;
  streaming: boolean;
  think: boolean;
  onThinkChange: (on: boolean) => void;
  onSend: (text: string, attachments: AttachmentInput[]) => void;
  onStop: () => void;
  /** Tokens already used by the conversation (last request + answer). */
  usedTokens: number;
  ctxSize: number;
  disabledReason?: string | undefined;
  initialText?: string;
  /** Reports whether the draft is empty (for the edit-last shortcut). */
  onEmptyChange?: (empty: boolean) => void;
}

const MAX_ATTACHMENTS = 5;

/** Token estimate for an attachment chip (the real cost is measured when the turn runs). */
export function estimateAttachmentTokens(a: AttachmentInput): number {
  return Math.ceil((a.text ?? "").length / 3.2);
}

export function isTruncated(a: AttachmentInput): boolean {
  return (a.text ?? "").includes("[… truncated");
}

/** Message box: Enter sends, Shift+Enter adds a line; becomes Stop while an answer streams. */
export function Composer({ placeholder, streaming, think, onThinkChange, onSend, onStop, usedTokens, ctxSize, disabledReason, initialText = "", onEmptyChange }: ComposerProps) {
  const [text, setText] = useState(initialText);
  const [files, setFiles] = useState<AttachmentInput[]>([]);
  const [dragging, setDragging] = useState(false);

  const add = (incoming: AttachmentInput[]) =>
    setFiles((cur) => {
      const merged = [...cur, ...incoming.filter((n) => !cur.some((c) => c.path !== null && c.path === n.path))];
      if (merged.length > MAX_ATTACHMENTS) toast.message(`Up to ${MAX_ATTACHMENTS} files per message.`);
      return merged.slice(0, MAX_ATTACHMENTS);
    });
  const readPaths = (paths: string[]) => {
    api.attachments
      .read(paths)
      .then(add)
      .catch((e: unknown) => toast.error(toIpcException(e).message));
  };
  const pick = () => {
    api.attachments
      .pick()
      .then(add)
      .catch((e: unknown) => toast.error(toIpcException(e).message));
  };

  // OS drag-and-drop onto the window (Tauri delivers paths, not HTML5 files).
  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | undefined;
    api.attachments
      .onDrag({ enter: () => setDragging(true), leave: () => setDragging(false), drop: (p) => { setDragging(false); readPaths(p); } })
      .then((u) => (cancelled ? u() : (off = u)))
      .catch(() => undefined);
    return () => {
      cancelled = true;
      off?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- handlers only use stable setters
  }, []);
  useEffect(() => {
    onEmptyChange?.(text.length === 0 && files.length === 0);
  }, [text, files.length, onEmptyChange]);
  const draftTokens = Math.ceil(text.length / 3.2) + files.reduce((n, f) => n + estimateAttachmentTokens(f), 0);
  const used = usedTokens + draftTokens;
  const fraction = ctxSize > 0 ? Math.min(1, used / ctxSize) : 0;
  const canSend = (text.trim().length > 0 || files.length > 0) && !streaming && !disabledReason;

  const submit = () => {
    if (!canSend) return;
    onSend(text, files);
    setText("");
    setFiles([]);
  };

  return (
    <div className="mx-auto w-full max-w-[760px] px-4 pb-4">
      <div
        className={cn(
          "relative rounded-modal border border-border bg-surface p-3 shadow-1 focus-within:border-border-strong",
          dragging && "border-dashed border-accent",
        )}
      >
        {dragging ? (
          <div className="pointer-events-none absolute inset-0 z-10 grid place-items-center rounded-modal bg-surface/90 text-body font-medium text-accent">
            Drop to attach
          </div>
        ) : null}
        {files.length > 0 ? (
          <ul className="mb-2 flex flex-wrap gap-1.5" aria-label="Attachments">
            {files.map((f) => (
              <li key={f.path ?? f.label} className="flex items-center gap-1.5 rounded-full border border-border bg-surface-2 py-0.5 pr-1 pl-2.5 text-small">
                <span>📎 {f.label}</span>
                <span className="tnum text-caption text-faint">~{estimateAttachmentTokens(f).toLocaleString()} tok</span>
                {isTruncated(f) ? <span className="text-caption text-warning">truncated</span> : null}
                <button type="button" aria-label={`Remove ${f.label}`} className="rounded-full p-0.5 text-muted hover:bg-surface-3 hover:text-text" onClick={() => setFiles((cur) => cur.filter((x) => x !== f))}>
                  <X className="size-3" />
                </button>
              </li>
            ))}
          </ul>
        ) : null}
        <Textarea
          aria-label="Message"
          data-composer=""
          autoFocus
          placeholder={disabledReason ?? placeholder}
          value={text}
          maxHeight={280}
          className="border-0 bg-transparent px-1 focus-visible:border-transparent focus-visible:ring-0"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              submit();
            }
          }}
        />
        <div className="mt-2 flex items-center gap-3">
          {IS_WEB ? null : (
            <IconButton aria-label="Attach files" size="sm" onClick={pick}>
              <Paperclip />
            </IconButton>
          )}
          <label className="flex items-center gap-2 text-small text-muted">
            <Switch checked={think} onCheckedChange={onThinkChange} aria-label="Think harder" />
            Think harder
          </label>
          <LibrariesChip />
          <div className="ml-auto flex items-center gap-3">
            <div
              role="meter"
              aria-label="Context used"
              aria-valuemin={0}
              aria-valuemax={ctxSize}
              aria-valuenow={Math.min(used, ctxSize)}
              title={`${used.toLocaleString()} of ${ctxSize.toLocaleString()} tokens`}
              className="flex items-center gap-2 text-caption text-faint"
            >
              <span className="h-1.5 w-16 overflow-hidden rounded-full bg-surface-3">
                <span className={cn("block h-full rounded-full", fraction > 0.9 ? "bg-danger" : fraction > 0.75 ? "bg-warning" : "bg-accent")} style={{ width: `${Math.round(fraction * 100)}%` }} />
              </span>
              <span className="tnum">
                {formatTokens(used)}/{formatTokens(ctxSize)}
              </span>
            </div>
            {streaming ? (
              <Button variant="secondary" onClick={onStop} aria-label="Stop generating">
                <Square className="fill-current" /> Stop
              </Button>
            ) : (
              <Button variant="primary" onClick={submit} disabled={!canSend} aria-label="Send message">
                <ArrowUp /> Send
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
