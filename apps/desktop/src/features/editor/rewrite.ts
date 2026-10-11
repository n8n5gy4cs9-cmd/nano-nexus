/** Helpers for AI rewrites: pull the code out of the reply and sanity-check it. */

/** The first fenced code block of a reply. A fence the model never closed (cut off) still counts. */
export function extractFence(reply: string): string | null {
  const m = /```[^\n]*\n([\s\S]*?)(?:\n```|$)/.exec(reply);
  if (!m || m[1] === undefined) return null;
  return m[1];
}

/** `true` when the rewrite is more than 2x or less than 0.3x the size of the original. */
export function isSuspicious(original: string, proposal: string): boolean {
  const o = original.trim().length;
  if (o === 0) return false;
  const ratio = proposal.trim().length / o;
  return ratio > 2 || ratio < 0.3;
}

/** Keep the original's trailing newline so the diff does not show a phantom last-line change. */
export function matchTrailingNewline(original: string, proposal: string): string {
  const wants = original.endsWith("\n");
  if (wants && !proposal.endsWith("\n")) return `${proposal}\n`;
  if (!wants && proposal.endsWith("\n")) return proposal.replace(/\n+$/, "");
  return proposal;
}

/** Up to `before` lines above and `after` lines below `[from, to)` of `doc`. */
export function contextAround(doc: string, from: number, to: number, before = 30, after = 15) {
  const head = doc.slice(0, from).split("\n");
  const tail = doc.slice(to).split("\n");
  return {
    before: head.slice(Math.max(0, head.length - 1 - before), head.length - 1).join("\n"),
    after: tail.slice(1, 1 + after).join("\n"),
  };
}
