# M08: Analyzer and Reports (Folder Scout, DocLens + pipelines)

**Goal:** the deterministic analysis engine and its pipelines, with a gorgeous report UI. This
brings the pipeline assistants to life: Folder Scout, DocLens, Tidy, Secret Sentinel,
Log Detective, Data Wrangler, and Commit Crafter.

**Spec:** [ANALYZER.md](../architecture/ANALYZER.md) §1–4, ASSISTANTS §3.3, `prompts/internal/*`.
**Prereqs:** M06 (tools, router, pipelines hook), M05 (change sets for cleanup/tidy).
**Exit demo:** "Scan my Downloads" → a report with treemap and AI summary in < 15 s for 5k
files → Plan cleanup (duplicates) → preview → apply → undo. Then drop a 30-page PDF into DocLens →
a map-reduce summary with page refs → ask a question → an answer cites `[p. 12]`.

## Scope
1. `nexus-analyze`: scanner, categories table, junk rules, naming checks (files), duplicates
   (3-stage), treemap builder, facts builder, Markdown/JSON/CSV renderers.
2. Extractors: plain/markdown/code, PDF (`pdf-extract`, spike S5), DOCX (zip + quick-xml), HTML (strip tags).
3. Chunking + BM25. File reports per type (ANALYZER §2).
4. Secrets scanner (rules below), log parser + template clustering, CSV/JSON profiler.
5. Pipelines in `nexus-agent::pipelines`: `folder_report`, `doc_summarize`, `doc_qa`, `tidy_plan`,
   `secrets_report`, `log_triage`, `data_profile`, `commit_message`. Wire the `scan_folder` and
   `doc_search` tools for the router.
6. Store migration 0004 (reports). Commands `analyze_*`, `reports_*`, `report_narrative`, `cleanup_plan`.
7. UI: Analyzer page, report view (KPI cards, treemap on canvas, category bars, virtualized
   tables, tabs), file report views, export menu, and "Open full report" links from chat.

## Design notes
- **Duplicates:** group by size (≥ `min_dup_size`) → BLAKE3 of the first 64 KiB → full BLAKE3
  (rayon, `blake3::Hasher::update_mmap_rayon` for big files). Report wasted bytes = `(n-1) × size`.
- **Treemap:** compute in Rust (squarified layout is cheap, but doing it in the UI with
  `d3-hierarchy` lets the user zoom without IPC). Rust sends the pruned tree (≤ 2 000 nodes). The UI
  lays it out with `d3-hierarchy.treemap().tile(treemapSquarify)` and draws on `<canvas>`
  (hover via a quadtree or simple hit testing).
- **Secrets rules** (subset modeled on gitleaks' default config): AWS access key `AKIA[0-9A-Z]{16}`,
  GitHub tokens `gh[pousr]_[A-Za-z0-9]{36,}`, GitLab `glpat-[A-Za-z0-9_-]{20}`, Slack `xox[baprs]-…`,
  Stripe `sk_live_[0-9a-zA-Z]{24,}`, Google API `AIza[0-9A-Za-z_-]{35}`, OpenAI-style `sk-[A-Za-z0-9_-]{20,}`,
  Anthropic `sk-ant-[A-Za-z0-9_-]{20,}`, private key blocks `-----BEGIN (RSA|EC|OPENSSH|DSA|PGP) PRIVATE KEY-----`,
  JWT `eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+`, generic `(?i)(password|passwd|secret|api_key|token)\s*[:=]\s*['"][^'"]{8,}['"]`
  with Shannon entropy ≥ 3.5, `.env` files present, and PII (emails, phone numbers, IBAN; low severity).
  **Values are always redacted** (`AKIA****WXYZ`) before reaching the LLM or the UI. The LLM only
  sees types, files, and counts.
- **Log clustering:** mask `\d+`, hex ≥ 8, UUIDs, ISO timestamps, IPs, and quoted strings → template
  → count. Detect levels (`ERROR|WARN|INFO|DEBUG|TRACE|FATAL|panic|Exception|Traceback`). Use a
  timeline histogram (per minute/hour).
- **commit_message input:** a pasted diff, or, if a granted folder is a git repo, the output of
  `git diff --staged` (read-only command run with `std::process::Command`, only `git` with fixed
  args, cwd inside the grant).
- **AI narrative streaming** goes into the report card via `report_narrative` + Channel. It's
  stored in `reports.ai_narrative`.

## Steps (each step = its own tasks in tasks.md)
1. Scanner + categories + junk + naming + duplicates + treemap + facts + renderers (+ tests on generated trees, plus a 100k-file benchmark).
2. Extractors + spike S5 + chunk + BM25 (+ tests with fixture docs in `tests/fixtures/docs`).
3. File reports (code/text/pdf/docx/csv/json/log/image/zip).
4. Secrets + logs + data profilers (+ tests with fixtures, including **fake** secrets).
5. Pipelines + tool wiring + internal prompt tests (golden structure, ignored).
6. Store, commands, jobs integration (progress, cancel).
7. UI: Analyzer home, report view, file report views, exports, cleanup plan → Changes.

## Exit criteria
- [ ] 100k-file synthetic tree scanned in < 10 s (no hashing) on an M1. Cancel works within 300 ms.
- [ ] Narrative uses only numbers present in the facts (a validator regex-extracts numbers from the narrative and checks each appears in the facts; tested).
- [ ] DocLens Q&A cites pages that actually contain the answer on 5/5 hand-checked questions.
- [ ] No raw secret value ever appears in the UI, the DB, or prompts (test greps the DB and the logged prompts in debug).

## Pitfalls
- Scanning `~` hits TCC prompts for every protected folder. Scan only granted roots, and skip
  `~/Library` unless explicitly granted.
- `pdf-extract` can panic on malformed PDFs. Run it under `std::panic::catch_unwind` in `spawn_blocking`.
- Large CSVs: stream with the `csv` crate and sample (reservoir 10k rows) for profiling.
