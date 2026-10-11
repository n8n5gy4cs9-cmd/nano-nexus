import { AlertCircle } from "lucide-react";
import { useNavigate } from "react-router-dom";

import type { ErrorCode } from "@/bindings";
import { Button } from "@/components/ui";

interface Copy {
  text: string;
  action?: { label: string; kind: "retry" | "restart" | "models" | "permissions" };
}

/** Friendly copy and a next step for each error code (DATA-AND-IPC §4). */
export function errorCopy(code: ErrorCode, message: string): Copy {
  switch (code) {
    case "runtime_unavailable":
      return { text: "The model isn't running yet.", action: { label: "Restart the model", kind: "restart" } };
    case "model_missing":
      return { text: "No model installed.", action: { label: "Open Models", kind: "models" } };
    case "permission_denied":
      return { text: "NanoNexus doesn't have access to this folder.", action: { label: "Grant access…", kind: "permissions" } };
    case "context_overflow":
      return { text: "That's too much text for this model at once. Try a shorter message, or start a new chat.", action: { label: "Try again", kind: "retry" } };
    case "busy":
      return { text: "The model is busy with another task. Try again in a moment.", action: { label: "Try again", kind: "retry" } };
    case "llm_invalid_output":
      return { text: "The model gave an unusable answer.", action: { label: "Try again", kind: "retry" } };
    case "cancelled":
      return { text: "Stopped." };
    default:
      return { text: message || "Something went wrong.", action: { label: "Try again", kind: "retry" } };
  }
}

export function ErrorCard({
  code,
  message,
  onRetry,
  onRestart,
}: {
  code: ErrorCode;
  message: string;
  onRetry?: (() => void) | undefined;
  onRestart?: (() => void) | undefined;
}) {
  const navigate = useNavigate();
  const copy = errorCopy(code, message);
  const run = () => {
    switch (copy.action?.kind) {
      case "retry":
        onRetry?.();
        break;
      case "restart":
        onRestart?.();
        break;
      case "models":
        void navigate("/models");
        break;
      case "permissions":
        void navigate("/permissions");
        break;
    }
  };
  return (
    <div role="alert" className="my-2 flex items-start gap-3 rounded-card border-l-2 border-danger bg-surface p-3">
      <AlertCircle className="mt-0.5 size-4 shrink-0 text-danger" />
      <div className="min-w-0 flex-1">
        <p className="text-body">{copy.text}</p>
        {message && message !== copy.text ? (
          <details className="mt-1 text-small text-muted">
            <summary className="cursor-pointer">Details</summary>
            <pre className="mt-1 whitespace-pre-wrap">{message}</pre>
          </details>
        ) : null}
      </div>
      {copy.action ? (
        <Button size="sm" variant="secondary" onClick={run}>
          {copy.action.label}
        </Button>
      ) : null}
    </div>
  );
}
