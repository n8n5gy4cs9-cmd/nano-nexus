import { baseName } from "@/lib/runtime-view";

/** `gemma-4-E4B-it-Q5_K_M.gguf` -> `gemma-4-E4B-it-Q5_K_M`. */
export function modelLabel(modelPath: string | undefined): string | null {
  if (!modelPath) return null;
  return baseName(modelPath).replace(/\.gguf$/i, "");
}

/** The parts of the status line, in order; empty when everything is off or unknown. */
export function statusParts(opts: {
  modelPath: string | undefined;
  tps: number | null;
  showModel: boolean;
  showSpeed: boolean;
}): string[] {
  const parts: string[] = [];
  const name = opts.showModel ? modelLabel(opts.modelPath) : null;
  if (name) parts.push(name);
  if (opts.showSpeed && opts.tps && opts.tps > 0) parts.push(`${opts.tps.toFixed(1)} tok/s`);
  return parts;
}

/** Tiny, quiet line under the composer: which model is running and how fast the last answer was. */
export function ChatStatusLine(props: Parameters<typeof statusParts>[0]) {
  const parts = statusParts(props);
  if (parts.length === 0) return null;
  return (
    <p aria-label="Model and speed" className="truncate px-5 pb-2 text-center text-caption text-faint" title={parts.join(" · ")}>
      {parts.join(" · ")}
    </p>
  );
}
