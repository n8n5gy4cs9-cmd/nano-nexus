/**
 * Split Markdown into top-level blocks on blank lines that are outside code fences, so a streaming
 * message re-renders only its last block (earlier blocks are memoized by their text).
 */
export function splitMarkdownBlocks(src: string): string[] {
  const blocks: string[] = [];
  let current: string[] = [];
  let fence: string | null = null;
  for (const line of src.split("\n")) {
    const m = /^\s*(`{3,}|~{3,})/.exec(line);
    if (m) {
      const marker = m[1] ?? "";
      if (fence === null) fence = marker[0] ?? "`";
      else if (marker[0] === fence) fence = null;
    }
    if (fence === null && line.trim() === "") {
      if (current.length > 0) {
        blocks.push(current.join("\n"));
        current = [];
      }
      continue;
    }
    current.push(line);
  }
  if (current.length > 0) blocks.push(current.join("\n"));
  return blocks;
}

const ALIASES: Record<string, string> = {
  js: "javascript",
  ts: "typescript",
  py: "python",
  sh: "bash",
  shell: "bash",
  zsh: "bash",
  yml: "yaml",
  rs: "rust",
  gd: "gdscript",
  "c++": "cpp",
  "c#": "csharp",
  md: "markdown",
  txt: "text",
  plaintext: "text",
};

/** Normalize a fence info string (`ts`, `language-js`) to a Shiki language id. */
export function normalizeLanguage(raw: string | undefined): string {
  const l = (raw ?? "").trim().toLowerCase().replace(/^language-/, "");
  if (l === "") return "text";
  return ALIASES[l] ?? l;
}
