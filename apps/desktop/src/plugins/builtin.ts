import { baseName } from "@/lib/runtime-view";

import type { Plugin } from "./types";

/** `gemma-4-E4B-it-Q5_K_M.gguf` -> `gemma-4-E4B-it-Q5_K_M`. */
export function modelLabel(modelPath: string | undefined): string | null {
  if (!modelPath) return null;
  return baseName(modelPath).replace(/\.gguf$/i, "");
}

export const modelNamePlugin: Plugin = {
  id: "model-name",
  name: "Model name",
  description: "Shows which model is running, under the chat box.",
  footer: (ctx) => modelLabel(ctx.modelPath),
};

export const speedPlugin: Plugin = {
  id: "speed",
  name: "Speed",
  description: "Shows the tokens per second of the last answer, under the chat box.",
  footer: (ctx) => (ctx.lastTps && ctx.lastTps > 0 ? `${ctx.lastTps.toFixed(1)} tok/s` : null),
};
