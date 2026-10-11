import { StreamLanguage, type StreamParser } from "@codemirror/language";
import type { Extension } from "@codemirror/state";

export interface LangInfo {
  id: string;
  label: string;
  load: () => Promise<Extension>;
}

const legacy = (label: string, id: string, parser: () => Promise<StreamParser<unknown>>): LangInfo => ({
  id,
  label,
  load: async () => StreamLanguage.define(await parser()),
});

const L: Record<string, LangInfo> = {
  javascript: { id: "javascript", label: "JavaScript", load: async () => (await import("@codemirror/lang-javascript")).javascript({ jsx: true }) },
  typescript: { id: "typescript", label: "TypeScript", load: async () => (await import("@codemirror/lang-javascript")).javascript({ jsx: true, typescript: true }) },
  rust: { id: "rust", label: "Rust", load: async () => (await import("@codemirror/lang-rust")).rust() },
  python: { id: "python", label: "Python", load: async () => (await import("@codemirror/lang-python")).python() },
  json: { id: "json", label: "JSON", load: async () => (await import("@codemirror/lang-json")).json() },
  markdown: { id: "markdown", label: "Markdown", load: async () => (await import("@codemirror/lang-markdown")).markdown() },
  html: { id: "html", label: "HTML", load: async () => (await import("@codemirror/lang-html")).html() },
  css: { id: "css", label: "CSS", load: async () => (await import("@codemirror/lang-css")).css() },
  cpp: { id: "cpp", label: "C/C++", load: async () => (await import("@codemirror/lang-cpp")).cpp() },
  java: { id: "java", label: "Java", load: async () => (await import("@codemirror/lang-java")).java() },
  sql: { id: "sql", label: "SQL", load: async () => (await import("@codemirror/lang-sql")).sql() },
  xml: { id: "xml", label: "XML", load: async () => (await import("@codemirror/lang-xml")).xml() },
  yaml: { id: "yaml", label: "YAML", load: async () => (await import("@codemirror/lang-yaml")).yaml() },
  php: { id: "php", label: "PHP", load: async () => (await import("@codemirror/lang-php")).php() },
  shell: legacy("Shell", "shell", async () => (await import("@codemirror/legacy-modes/mode/shell")).shell),
  lua: legacy("Lua", "lua", async () => (await import("@codemirror/legacy-modes/mode/lua")).lua),
  toml: legacy("TOML", "toml", async () => (await import("@codemirror/legacy-modes/mode/toml")).toml),
  csharp: legacy("C#", "csharp", async () => (await import("@codemirror/legacy-modes/mode/clike")).csharp),
  swift: legacy("Swift", "swift", async () => (await import("@codemirror/legacy-modes/mode/swift")).swift),
  go: legacy("Go", "go", async () => (await import("@codemirror/legacy-modes/mode/go")).go),
  ruby: legacy("Ruby", "ruby", async () => (await import("@codemirror/legacy-modes/mode/ruby")).ruby),
  ini: legacy("INI", "ini", async () => (await import("@codemirror/legacy-modes/mode/properties")).properties),
};

const BY_EXT: Record<string, keyof typeof L> = {
  js: "javascript", mjs: "javascript", cjs: "javascript", jsx: "javascript",
  ts: "typescript", tsx: "typescript", mts: "typescript",
  rs: "rust", py: "python", json: "json", jsonc: "json",
  md: "markdown", markdown: "markdown", html: "html", htm: "html", css: "css",
  c: "cpp", h: "cpp", cc: "cpp", cpp: "cpp", hpp: "cpp", java: "java", sql: "sql",
  xml: "xml", svg: "xml", yml: "yaml", yaml: "yaml", php: "php",
  sh: "shell", bash: "shell", zsh: "shell", lua: "lua", toml: "toml", cs: "csharp",
  swift: "swift", go: "go", rb: "ruby", ini: "ini", cfg: "ini", conf: "ini",
};

const PLAIN: LangInfo = { id: "text", label: "Plain text", load: async () => [] };

/** The language of a file, by extension (plain text when unknown). */
export function languageFor(fileName: string): LangInfo {
  const dot = fileName.lastIndexOf(".");
  if (dot < 0) return PLAIN;
  const key = BY_EXT[fileName.slice(dot + 1).toLowerCase()];
  return (key && L[key]) || PLAIN;
}

/** Every language id the editor knows (for tests and the status bar). */
export function languageIds(): string[] {
  return Object.keys(L);
}
