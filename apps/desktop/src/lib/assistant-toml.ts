/**
 * Prompt Architect answers with one fenced ```toml block holding an assistant definition. This finds
 * it (the block must contain `schema_version = 1`) so the chat can offer "Open in builder".
 */
export function extractAssistantToml(markdown: string): string | null {
  const re = /```toml[^\n]*\n([\s\S]*?)```/g;
  for (const m of markdown.matchAll(re)) {
    const body = m[1] ?? "";
    if (/^\s*schema_version\s*=\s*1\s*$/m.test(body)) return body.trim() + "\n";
  }
  return null;
}
