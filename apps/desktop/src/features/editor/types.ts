// Mirrors of the Rust editor types (kept out of the generated bindings.ts).
export interface EditorSaveResult {
  changeset_id: string;
  hash: string;
}

export type EditorAction = "improve_names" | "add_docs" | "simplify" | "fix_bugs" | "explain" | "ask";

export interface EditorAiRequest {
  action: EditorAction;
  language: string;
  file_name: string;
  selection: string;
  context_before: string;
  context_after: string;
  instructions: string | null;
}

export type EditorAiEvent = { type: "content"; delta: string } | { type: "done" } | { type: "error"; message: string };
