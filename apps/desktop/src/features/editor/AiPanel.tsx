import { Loader2, Send, Square } from "lucide-react";
import { useState } from "react";

import { Button, Input } from "@/components/ui";
import { cn } from "@/lib/cn";

import type { EditorAction } from "./types";

const ACTIONS: { id: EditorAction; label: string }[] = [
  { id: "explain", label: "Explain" },
  { id: "improve_names", label: "Improve names" },
  { id: "add_docs", label: "Add docs" },
  { id: "fix_bugs", label: "Find bugs" },
  { id: "simplify", label: "Simplify" },
];

export interface AiState {
  running: boolean;
  /** Streamed text (a text answer, or the raw reply of a rewrite). */
  text: string;
  error: string | null;
  /** A rewrite is being reviewed in the merge view. */
  reviewing: boolean;
  /** The rewrite is much longer or shorter than the original. */
  suspicious: boolean;
  /** The selection the answer is about is empty. */
  needsSelection: boolean;
}

/** Right-hand panel: AI actions on the selection, the answer, and the review controls. */
export function AiPanel({
  state,
  hasSelection,
  readOnly,
  onRun,
  onStop,
  onFinish,
  onRejectAll,
}: {
  state: AiState;
  hasSelection: boolean;
  readOnly: boolean;
  onRun: (action: EditorAction, instructions: string | null) => void;
  onStop: () => void;
  onFinish: () => void;
  onRejectAll: () => void;
}) {
  const [question, setQuestion] = useState("");
  const rewriteBlocked = readOnly;
  return (
    <aside aria-label="AI actions" className="flex w-72 shrink-0 flex-col border-l border-border bg-surface">
      <div className="border-b border-border p-3">
        <div className="text-body font-semibold">AI actions</div>
        <p className="text-caption text-muted">
          {hasSelection ? "Works on the selected code." : "Select some code first."}
        </p>
        <div className="mt-2 flex flex-wrap gap-1.5">
          {ACTIONS.map((a) => {
            const rewrite = a.id !== "explain";
            return (
              <Button
                key={a.id}
                size="sm"
                variant="secondary"
                disabled={!hasSelection || state.running || state.reviewing || (rewrite && rewriteBlocked)}
                onClick={() => onRun(a.id, null)}
              >
                {a.label}
              </Button>
            );
          })}
        </div>
        <form
          className="mt-2 flex gap-1.5"
          onSubmit={(e) => {
            e.preventDefault();
            if (question.trim()) onRun("ask", question.trim());
          }}
        >
          <Input value={question} onChange={(e) => setQuestion(e.target.value)} placeholder="Ask about this code…" aria-label="Ask about the selection" />
          <Button type="submit" size="sm" disabled={!hasSelection || state.running || state.reviewing || !question.trim()} aria-label="Ask">
            <Send />
          </Button>
        </form>
        {rewriteBlocked ? <p className="mt-2 text-caption text-warning">This file is read-only, so only Explain and Ask work.</p> : null}
      </div>

      <div className="min-h-0 flex-1 overflow-auto p-3">
        {state.running ? (
          <div className="mb-2 flex items-center gap-2 text-small text-muted">
            <Loader2 className="size-3.5 animate-spin" />
            Working…
            <Button size="sm" variant="ghost" onClick={onStop}>
              <Square />
              Stop
            </Button>
          </div>
        ) : null}
        {state.error ? (
          <p role="alert" className="text-small text-danger">
            {state.error}
          </p>
        ) : null}
        {state.reviewing ? (
          <div className="rounded-card border border-border p-3 text-small">
            <p>Review the changes in the editor. Accept or reject each one, then finish.</p>
            {state.suspicious ? (
              <p role="alert" className="mt-2 text-warning">
                Suspicious rewrite: it is much longer or shorter than your code. Check it carefully.
              </p>
            ) : null}
            <div className="mt-2 flex gap-2">
              <Button size="sm" onClick={onFinish}>
                Finish review
              </Button>
              <Button size="sm" variant="secondary" onClick={onRejectAll}>
                Reject all
              </Button>
            </div>
          </div>
        ) : state.text ? (
          <pre className={cn("whitespace-pre-wrap break-words text-small leading-5 text-text")}>{state.text}</pre>
        ) : !state.running && !state.error ? (
          <p className="text-small text-muted">Pick an action. Changes always appear as a diff first; nothing is saved until you press Save.</p>
        ) : null}
      </div>
    </aside>
  );
}
