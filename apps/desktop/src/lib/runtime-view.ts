import type { RuntimeState } from "@/bindings";

export type PillTone = "neutral" | "success" | "warning" | "danger";

export interface RuntimeView {
  tone: PillTone;
  label: string;
  /** Show a spinner (starting or loading). */
  busy: boolean;
  /** One line under the label in the popover. */
  detail: string;
}

/** Last path segment, either separator. */
export function baseName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** How the runtime pill and its popover describe a supervisor state. */
export function describeRuntime(state: RuntimeState | undefined): RuntimeView {
  if (!state) return { tone: "neutral", label: "Model", busy: true, detail: "Checking the model…" };
  switch (state.state) {
    case "stopped":
      return { tone: "neutral", label: "Stopped", busy: false, detail: "The model is not running." };
    case "starting":
      return {
        tone: "warning",
        label: "Starting",
        busy: true,
        detail: state.attempt > 1 ? `Starting the model (attempt ${state.attempt})…` : "Starting the model…",
      };
    case "loading":
      return { tone: "warning", label: "Loading", busy: true, detail: "Loading the model into memory…" };
    case "ready":
      return { tone: "success", label: "Ready", busy: false, detail: baseName(state.model_path) };
    case "failed":
      return { tone: "danger", label: "Error", busy: false, detail: state.reason };
  }
}
