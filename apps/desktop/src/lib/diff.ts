/** Parsing of unified diffs (as produced by `nexus_fs::preview`) for the diff viewer. */

export type DiffKind = "context" | "add" | "del" | "hunk";

export interface DiffLine {
  kind: DiffKind;
  text: string;
  /** 1-based line numbers; `null` where the side has no such line. */
  oldNo: number | null;
  newNo: number | null;
}

export interface SplitRow {
  left: DiffLine | null;
  right: DiffLine | null;
  /** A `@@` separator spanning both sides. */
  hunk?: DiffLine;
}

const HUNK = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/;

/** Parse a unified diff into lines with line numbers. File headers (`---`, `+++`) are skipped. */
export function parseUnifiedDiff(diff: string): DiffLine[] {
  const out: DiffLine[] = [];
  let oldNo = 0;
  let newNo = 0;
  let inHunk = false;
  for (const raw of diff.split("\n")) {
    const h = HUNK.exec(raw);
    if (h) {
      oldNo = Number(h[1]);
      newNo = Number(h[2]);
      inHunk = true;
      out.push({ kind: "hunk", text: raw, oldNo: null, newNo: null });
      continue;
    }
    if (!inHunk) continue; // `---`/`+++` headers and anything before the first hunk
    if (raw.startsWith("\\")) continue; // "\ No newline at end of file"
    if (raw.startsWith("+")) out.push({ kind: "add", text: raw.slice(1), oldNo: null, newNo: newNo++ });
    else if (raw.startsWith("-")) out.push({ kind: "del", text: raw.slice(1), oldNo: oldNo++, newNo: null });
    else if (raw.startsWith(" ")) out.push({ kind: "context", text: raw.slice(1), oldNo: oldNo++, newNo: newNo++ });
    // A trailing empty string from the final newline is ignored.
  }
  return out;
}

/** Pair removed and added lines side by side: each run of deletions lines up with the run of additions after it. */
export function toSplitRows(lines: DiffLine[]): SplitRow[] {
  const rows: SplitRow[] = [];
  let dels: DiffLine[] = [];
  let adds: DiffLine[] = [];
  const flush = () => {
    const n = Math.max(dels.length, adds.length);
    for (let k = 0; k < n; k++) rows.push({ left: dels[k] ?? null, right: adds[k] ?? null });
    dels = [];
    adds = [];
  };
  for (const line of lines) {
    if (line.kind === "del") {
      // A deletion after additions starts a new run.
      if (adds.length > 0) flush();
      dels.push(line);
    } else if (line.kind === "add") {
      adds.push(line);
    } else {
      flush();
      rows.push(line.kind === "hunk" ? { left: null, right: null, hunk: line } : { left: line, right: line });
    }
  }
  flush();
  return rows;
}
