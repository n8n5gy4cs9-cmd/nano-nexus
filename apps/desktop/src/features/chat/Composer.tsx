import { ArrowUp, Square } from "lucide-react";
import { useState } from "react";

import { Button, Switch, Textarea } from "@/components/ui";
import { cn } from "@/lib/cn";
import { formatTokens } from "@/lib/tokens";

export interface ComposerProps {
  placeholder: string;
  streaming: boolean;
  think: boolean;
  onThinkChange: (on: boolean) => void;
  onSend: (text: string) => void;
  onStop: () => void;
  /** Tokens already used by the conversation (last request + answer). */
  usedTokens: number;
  ctxSize: number;
  disabledReason?: string | undefined;
  initialText?: string;
}

/** Message box: Enter sends, Shift+Enter adds a line; becomes Stop while an answer streams. */
export function Composer({ placeholder, streaming, think, onThinkChange, onSend, onStop, usedTokens, ctxSize, disabledReason, initialText = "" }: ComposerProps) {
  const [text, setText] = useState(initialText);
  const draftTokens = Math.ceil(text.length / 3.2);
  const used = usedTokens + draftTokens;
  const fraction = ctxSize > 0 ? Math.min(1, used / ctxSize) : 0;
  const canSend = text.trim().length > 0 && !streaming && !disabledReason;

  const submit = () => {
    if (!canSend) return;
    onSend(text);
    setText("");
  };

  return (
    <div className="mx-auto w-full max-w-[760px] px-4 pb-4">
      <div className="rounded-modal border border-border bg-surface p-3 shadow-1 focus-within:border-border-strong">
        <Textarea
          aria-label="Message"
          placeholder={disabledReason ?? placeholder}
          value={text}
          maxHeight={280}
          className="border-0 bg-transparent px-1 focus-visible:ring-0"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              submit();
            }
          }}
        />
        <div className="mt-2 flex items-center gap-3">
          <label className="flex items-center gap-2 text-small text-muted">
            <Switch checked={think} onCheckedChange={onThinkChange} aria-label="Think harder" />
            Think harder
          </label>
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
