// Hand-written mirrors of the Rust knowledge types (nexus-knowledge, service::knowledge).
export interface KbLibrary {
  id: string;
  name: string;
  source: "bundled" | "user";
  path: string;
  mode: "copy" | "reference";
  enabled: boolean;
  doc_count: number;
  chunk_count: number;
  bytes: number;
  indexed_at: number | null;
  created_at: number;
}

export interface KbDocument {
  id: number;
  library_id: string;
  path: string;
  title: string | null;
  hash: string;
  kind: string;
  pages: number;
  bytes: number;
  status: "indexed" | "skipped" | "failed";
  note: string | null;
}

export interface KbLibraryDetail {
  library: KbLibrary;
  documents: KbDocument[];
}

export interface KbProgress {
  library_id: string;
  files_done: number;
  files_total: number;
  current: string;
  chunks: number;
  skipped: number;
}

export interface KbIndexResult {
  library_id: string;
  indexed: number;
  unchanged: number;
  skipped: number;
  failed: number;
  removed: number;
  chunks: number;
  truncated: boolean;
}

export interface KbPassage {
  chunk_id: number;
  document_id: number;
  library_id: string;
  library_name: string;
  document: string;
  doc_path: string;
  heading_path: string;
  page: number | null;
  text: string;
  tokens: number;
  score: number;
}

/** Citation links in finished answers point here; the chat opens the passage instead of a browser. */
export const PASSAGE_URL_PREFIX = "https://nanonexus.local/passage/";

/** `https://nanonexus.local/passage/57` -> 57, anything else -> null. */
export function parsePassageUrl(url: string): number | null {
  if (!url.startsWith(PASSAGE_URL_PREFIX)) return null;
  const n = Number(url.slice(PASSAGE_URL_PREFIX.length));
  return Number.isInteger(n) && n > 0 ? n : null;
}
