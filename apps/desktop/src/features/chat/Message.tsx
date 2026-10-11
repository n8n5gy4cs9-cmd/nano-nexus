import { Check, Copy, Pencil, RotateCw, WandSparkles } from "lucide-react";
import { useState } from "react";
import { useNavigate } from "react-router-dom";

import type { ErrorCode, Message as StoredMessage } from "@/bindings";
import { AssistantIcon, Button, IconButton, Textarea } from "@/components/ui";
import { api } from "@/lib/api";
import { extractAssistantToml } from "@/lib/assistant-toml";
import { cn } from "@/lib/cn";
import { useAssistantChoice } from "@/stores/assistant";
import type { ToolChip } from "@/stores/chat";

import { ErrorCard } from "./ErrorCard";
import { Markdown } from "./Markdown";
import { ThoughtsBlock } from "./ThoughtsBlock";
import { ToolChips } from "./ToolChips";

const openLink = (url: string) => void api.app.openExternal(url).catch(() => undefined);

export interface AssistantView {
  id: string;
  content: string;
  reasoning: string | null;
  streaming: boolean;
  /** When thinking started (live turns only), for the timer. */
  thinkingStartedAt?: number | undefined;
  stopped: boolean;
  error: { code: ErrorCode; message: string } | null;
  stats: { tps: number | null; ttftMs: number | null; tokensOut: number | null } | null;
  statusText?: string | null | undefined;
  tools?: ToolChip[] | undefined;
}

/** Who wrote an answer: name, icon, and color of the assistant. */
export interface AssistantBadge {
  name: string;
  icon: string;
  color: string;
}

export function formatStats(s: NonNullable<AssistantView["stats"]>): string {
  const parts: string[] = [];
  if (s.tps) parts.push(`${s.tps.toFixed(0)} tok/s`);
  if (s.ttftMs !== null) parts.push(`${(s.ttftMs / 1000).toFixed(1)} s`);
  return parts.join(" · ");
}

export function UserMessage({
  m,
  canEdit,
  onEdit,
  startEditing = false,
}: {
  m: Pick<StoredMessage, "id" | "content" | "attachments">;
  canEdit: boolean;
  onEdit: (text: string) => void;
  /** Open the editor (the edit-last shortcut). */
  startEditing?: boolean;
}) {
  const [editing, setEditing] = useState(startEditing);
  const [text, setText] = useState(m.content);
  return (
    <div className="group flex flex-col items-end gap-1" data-role="user">
      {m.attachments.length > 0 ? (
        <div className="flex flex-wrap justify-end gap-1">
          {m.attachments.map((a) => (
            <span key={a.id} className="rounded-full border border-border bg-surface-2 px-2 py-0.5 text-caption text-muted">
              📎 {a.label}
              {a.tokens ? ` · ${a.tokens} tok` : ""}
            </span>
          ))}
        </div>
      ) : null}
      {editing ? (
        <div className="flex w-full max-w-[85%] flex-col gap-2">
          <Textarea aria-label="Edit message" autoFocus value={text} onChange={(e) => setText(e.target.value)} />
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={() => { setEditing(false); setText(m.content); }}>
              Cancel
            </Button>
            <Button
              size="sm"
              variant="primary"
              disabled={!text.trim()}
              onClick={() => {
                setEditing(false);
                onEdit(text);
              }}
            >
              Send
            </Button>
          </div>
        </div>
      ) : (
        <div className="max-w-[85%] rounded-modal bg-surface-2 px-4 py-2.5 text-chat leading-[26px] break-words whitespace-pre-wrap">{m.content}</div>
      )}
      {canEdit && !editing ? (
        <IconButton aria-label="Edit message" size="sm" className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100" onClick={() => setEditing(true)}>
          <Pencil />
        </IconButton>
      ) : null}
    </div>
  );
}

export function AssistantMessage({
  v,
  assistant,
  onRegenerate,
  onRestart,
}: {
  v: AssistantView;
  assistant: AssistantBadge;
  onRegenerate?: (() => void) | undefined;
  onRestart?: (() => void) | undefined;
}) {
  const [copied, setCopied] = useState(false);
  const navigate = useNavigate();
  const builderToml = !v.streaming && v.content ? extractAssistantToml(v.content) : null;
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(v.content);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard unavailable */
    }
  };
  return (
    <div className="group" data-role="assistant" data-message-id={v.id}>
      <div className="mb-1 flex items-center gap-2 text-small font-medium text-muted">
        <AssistantIcon icon={assistant.icon} color={assistant.color} size={20} />
        {assistant.name}
      </div>
      <ToolChips chips={v.tools ?? []} />
      {v.reasoning ? <ThoughtsBlock text={v.reasoning} streaming={v.streaming && v.content === ""} startedAt={v.thinkingStartedAt} /> : null}
      {v.statusText ? <p className="my-1 text-small text-muted">{v.statusText}</p> : null}
      {v.content ? (
        <Markdown source={v.content} onOpenLink={openLink} className={cn(v.streaming && "streaming-caret")} />
      ) : v.streaming && !v.reasoning ? (
        <p className="animate-pulse text-muted">…</p>
      ) : null}
      {builderToml ? (
        <Button
          size="sm"
          className="mt-2"
          onClick={() => {
            useAssistantChoice.getState().setBuilderDraft(builderToml);
            void navigate("/assistants");
          }}
        >
          <WandSparkles /> Open in builder
        </Button>
      ) : null}
      {v.stopped && !v.streaming ? <p className="text-small text-faint">Stopped.</p> : null}
      {v.error ? <ErrorCard code={v.error.code} message={v.error.message} onRetry={onRegenerate} onRestart={onRestart} /> : null}
      {!v.streaming ? (
        <div className="mt-1 flex items-center gap-1 text-caption text-faint opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
          <IconButton aria-label={copied ? "Copied" : "Copy answer"} size="sm" onClick={() => void copy()} disabled={!v.content}>
            {copied ? <Check /> : <Copy />}
          </IconButton>
          {onRegenerate ? (
            <IconButton aria-label="Regenerate answer" size="sm" onClick={onRegenerate}>
              <RotateCw />
            </IconButton>
          ) : null}
          {v.stats ? <span className="tnum ml-1">{formatStats(v.stats)}</span> : null}
        </div>
      ) : null}
    </div>
  );
}
