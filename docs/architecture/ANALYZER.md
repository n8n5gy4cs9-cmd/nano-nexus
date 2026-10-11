# Analyzer, Reports, and Rename Forge (`crates/nexus-analyze`)

> Rust computes the facts. The LLM writes the story and proposes names. Every list is computed,
> never "remembered" by the model.

## 1. Folder scan (`scan::scan_folder`)

```rust
pub struct ScanOptions {
    pub root: PathBuf,                 // already permission-checked (Read)
    pub include_hidden: bool,          // default false
    pub respect_gitignore: bool,       // default true for code projects, false otherwise
    pub follow_symlinks: bool,         // always false in v1
    pub max_entries: usize,            // 1_000_000 hard cap → report "truncated"
    pub duplicates: DupMode,           // Off | Fast (size+64KB head) | Full (BLAKE3)
    pub min_dup_size: u64,             // 4 KiB default
}
pub fn scan_folder(opts: &ScanOptions, progress: &dyn Fn(ScanProgress), cancel: &CancellationToken) -> Result<FolderReport>;
```

Implementation: `ignore::WalkBuilder` (parallel walker, gitignore support) → collect `FileEntry`
into a `Vec` (≈ 120 bytes/entry; 1M entries ≈ 120 MB, acceptable) → aggregate. Progress is
throttled to 10 Hz: `{files_seen, bytes_seen, current_dir}`. Permission errors become
`warnings: [{path, error}]` and never abort the scan.

### FolderReport (serialized as JSON to `reports/<id>.json`)
```rust
pub struct FolderReport {
  pub id: ReportId, pub root: PathBuf, pub scanned_at: OffsetDateTime, pub duration_ms: u64,
  pub totals: Totals { files, dirs, bytes, hidden_files, symlinks },
  pub by_category: Vec<CategoryStat { category, files, bytes, pct_bytes }>,
  pub by_extension: Vec<ExtStat { ext, files, bytes }>,             // top 30
  pub largest_files: Vec<FileRef { rel_path, bytes, modified }>,    // top 50
  pub largest_dirs: Vec<DirRef { rel_path, bytes, files }>,         // top 30 (recursive sizes)
  pub age: AgeBuckets { last_7d, last_30d, last_365d, older, unknown },   // by bytes and count
  pub duplicates: Vec<DupGroup { bytes_each, hash, paths: Vec<String> }>, // sorted by wasted bytes
  pub wasted_by_duplicates: u64,
  pub empty_dirs: Vec<String>, pub empty_files: Vec<String>,
  pub junk: Vec<JunkHit { rule, rel_path, bytes }>,                 // see junk rules
  pub naming_issues: Vec<NamingHit { kind, rel_path, detail }>,
  pub max_depth: u32, pub deepest_path: String,
  pub treemap: TreeNode,                                            // pruned: children < 0.5% merged into "other"
  pub warnings: Vec<ScanWarning>, pub truncated: bool,
}
```

### Categories (deterministic, by extension, then magic bytes via `infer`)
`Image, Video, Audio, Document, Pdf, Spreadsheet, Presentation, Code, Data, Archive, Installer,
Font, Model3D, GameAsset, Executable, System, Other`. The mapping table lives in
`crates/nexus-analyze/src/categories.rs` (≥ 250 extensions). Examples: `.py .rs .ts .cs .go .java` → Code; `.dmg .pkg .exe .msi` → Installer.

### Junk rules (each with an id and a "safe to trash?" flag)
`.DS_Store`, `Thumbs.db`, `desktop.ini`, `~$*.docx` (Office lock files), `*.tmp`, `*.crdownload`, `*.part`
(stale > 7 days), `node_modules/` (size-reported, **not** auto-trash), `target/` (Rust), `.venv/`, `dist/`, `build/`, `__pycache__/`, `.gradle/`, `DerivedData/`, `*.log` > 50 MB.
Folders that can be regenerated are flagged "regenerable" and never "junk-to-delete" by default.

### Naming issues (files)
Spaces or mixed separators, non-ASCII, names longer than 100 characters, `copy`/`(1)`/`final_final`/`v2_new`
patterns, inconsistent case within a folder, extension mismatch (magic bytes ≠ ext), and trailing
spaces or dots (Windows-hostile).

### Facts for the LLM (`facts::folder_facts(&report) -> String`, ≤ 1 200 tokens)
```
<facts>
root: ~/Downloads   scanned: 2026-10-11 14:02   files: 4,812   folders: 311   size: 38.4 GB
top categories by size: Video 21.0 GB (55%), Installer 9.2 GB (24%), Archive 4.1 GB (11%), …
largest files: 1) Movies/talk.mov 6.2 GB (2024-03-02) 2) …(top 10)
largest folders: 1) Movies 18.3 GB 2) old-installers 8.8 GB …(top 8)
age: older than 1 year 27.1 GB (71%) · last 30 days 2.3 GB
duplicates: 37 groups, 2.4 GB wasted; biggest: setup.dmg ×3 (1.1 GB each)
junk: .DS_Store ×212 (1.3 MB) · *.crdownload ×4 (900 MB, stale)
naming: 64 files with "copy"/"(1)" patterns · 12 non-ASCII names
warnings: 3 folders unreadable (permission)
</facts>
```
All numbers are pre-formatted (human-readable units). The model never formats bytes.

## 2. File reports (`report::file_report(path)`)

| Kind | Facts computed (Rust) | LLM step |
|---|---|---|
| Code | language, LOC/blank/comment, symbols by kind, longest functions (top 5 with line counts), TODO/FIXME list, imports, cyclomatic-ish branch count per function (tree-sitter node count of if/for/while/match) | `file_report_code.md`: purpose, structure, smells, 3 suggestions |
| Text / Markdown | words, headings outline, reading time, language guess (`whatlang`) | TL;DR summary |
| PDF | pages, title/author/creation date (metadata), text per page (`pdf-extract`), images-only pages flagged "scanned (no text)" | `doc_summarize` pipeline |
| DOCX | text via zip + `word/document.xml` parse (quick-xml), headings | `doc_summarize` |
| CSV / TSV | rows, columns, inferred types, null %, unique counts, min/max/mean, top values, sample rows | `data_profile.md` |
| JSON | depth, top-level shape, key frequencies, array lengths | `data_profile.md` |
| Logs | line count, time range, level counts, top 20 message templates (number/hex/uuid/path masking), first and last error | `log_triage.md` |
| Image | dimensions, format, color type, EXIF date/camera (`kamadak-exif`), GPS presence ⚠ (privacy flag) | none (facts only) |
| Archive (zip) | entry count, uncompressed size, top entries | none |

Text extraction lives in `extract::{pdf, docx, html, plain}`. Spike S5 compares `pdf-extract`
against alternatives on 3 real PDFs. Fallback: if `pdftotext` (poppler) is on PATH, use it.

Chunking (`chunk::chunk_doc`): split on headings or paragraphs into ~700-token chunks with
80-token overlap, keeping `page_start/page_end`. BM25 (`bm25` module, k1 = 1.2, b = 0.75,
lowercase + unicode word tokenizer + English stopwords) ranks chunks for `doc_qa`.

## 3. Report UI and exports
- The UI renders the JSON (see UI-DESIGN §Analyzer). The AI narrative streams into a card at the top.
- Exports: **Markdown** (`render::markdown`, nice for notes), **JSON** (the raw report), and
  **CSV** (largest files, duplicates, and junk as separate CSVs zipped, or one chosen table).
- "Plan cleanup" from duplicates or junk → builds a `ChangeSet` of `Trash` ops (keeps the oldest
  copy of each duplicate group by default) → Changes preview → Apply.

## 4. Watches (F6.7, M11)
`watches(id, root, schedule: daily|weekly|on_change, last_report_id, enabled)`. A
`tokio::time::interval` checks once a minute while the app runs. `on_change` uses the `notify`
crate, debounced 10 minutes. After a scan, `diff_reports(prev, new)` gives deltas
(size, new big files, new duplicates). If they exceed thresholds, an OS notification fires:
*"Downloads grew by 3.2 GB since Monday: 2 new installers (2.4 GB)."*

---

## 5. Rename Forge

Rename Forge is **language- and framework-agnostic**: point it at any code project and it renames
symbols, files, and folders while updating every reference it can prove. It also answers
"where is this used?" (§5.9). No language or engine gets special treatment in the core; per-language
knowledge lives in data files (tags queries, reserved-name lists).

### 5.1 Pipeline overview
```
Pick project (grant: ReadWrite needed to apply; Read is enough to analyze)
 → Index (tree-sitter, per-language tags queries)  → SymbolIndex (+ file/folder tree)
 → Detect issues (naming rules for symbols, files, folders) → candidates
 → Propose (LLM, batches of ≤ 20)          → proposals (validated)
 → Review grid (user accepts/edits)        → accepted set
 → Plan (exact byte ranges + file moves)   → ChangeSet (+ conflicts)
 → Preview diff → Apply (journal) → Verify (optional: the project's own checker, §5.7)
 → Undo available in Changes
```

### 5.2 Index model
```rust
pub struct Symbol {
  pub id: SymbolId, pub name: String, pub kind: SymbolKind, pub lang: Lang,
  pub def: Location /* file, byte range, line */, pub scope: ScopeKey /* file::class::func */,
  pub signature: String,          // e.g. "def calc(a, b) -> int"
  pub doc: Option<String>,        // preceding comment
  pub refs: Vec<Ref>,             // all occurrences, including def
  pub flags: SymbolFlags,         // RESERVED, EXPORTED, PUBLIC_API, DYNAMIC_REFS, OVERRIDE, TEST, ENTRY_POINT
}
pub enum SymbolKind { Function, Method, Variable, Constant, Parameter, Field, Class, Interface, Enum, EnumMember, Module, File, Folder }
pub struct Ref { pub loc: Location, pub kind: RefKind }   // Code | Import | StringLiteral | ConfigFile | Doc | Comment
```

Resolution is **name + scope based** (not a full type checker). Local variables and parameters
resolve within the function scope. Members resolve within the class and through
`self.`/`this.`/bare access. Cross-file references resolve through each language's import graph
(§5.3). Ambiguous cases (the same member name in two classes accessed through an untyped
variable, e.g. `enemy.hit()`) are marked `AMBIGUOUS` and must be confirmed by the user per
occurrence in the review grid.

### 5.3 Language adapters
| Language | Grammar crate | Import / module resolution | Notes |
|---|---|---|---|
| Python | `tree-sitter-python` 0.25 | `import a.b`, `from a import b`, packages via `__init__.py`, relative imports | Dunder methods never renamed; `__all__` entries are refs |
| JavaScript / TypeScript (+ JSX/TSX) | `tree-sitter-javascript` 0.25 / `tree-sitter-typescript` 0.23 | ES `import`/`export`, `require`, `tsconfig` `paths`, index files, extension-less specifiers | Exported names are flagged `PUBLIC_API`; React components stay PascalCase |
| Rust | `tree-sitter-rust` 0.24 | `mod x;` ↔ `x.rs` / `x/mod.rs`, `use crate::…`, `pub use` | Trait impl methods (`OVERRIDE`) are skipped; `pub` items flagged `PUBLIC_API` (warn) |
| C# | `tree-sitter-c-sharp` 0.23 | namespaces and `using`; one type per file convention | Interface implementations and `override` are skipped |
| Go | `tree-sitter-go` 0.25 | package = folder; `import "module/path"` | Exported (capitalized) names flagged `PUBLIC_API`; renaming a folder rewrites import paths |
| Java | `tree-sitter-java` 0.23 | `package` and `import` match the folder tree; class = file name | Renaming a public class renames its file |
| Lua, C/C++, others | `tree-sitter-lua` 0.5, `tree-sitter-c`/`-cpp` | `require` / `#include` | P1, one `tags.scm` each |

Each adapter is a `.scm` tags query file in `crates/nexus-analyze/queries/<lang>/tags.scm`
(the tree-sitter "tags" convention: `@definition.function`, `@reference.call`, …) plus a small
Rust resolver for imports. Adding a language = a query file, a reserved-name list, a convention
table, and fixtures. Spike S4 pins the grammar versions against `tree-sitter` 0.27.

### 5.4 References outside the code
A rename that only edits identifiers leaves a project broken. These are indexed as `Ref`s:

| Where | Pattern | Ref kind / action |
|---|---|---|
| Imports and module declarations | `from util import calc_total`, `import {x} from "./x"`, `mod x;`, `using Foo.Bar;` | Import: rewrite the specifier (symbol renames) or the path (file/folder renames) |
| Config and manifests | `package.json` (`main`, `bin`, `exports`, scripts), `tsconfig` `paths`, `Cargo.toml` (`[[bin]]`, `[lib]`, mod paths), `pyproject.toml` (entry points, packages), `*.csproj`, `go.mod`, CI workflows, Dockerfiles, YAML/JSON/TOML in general | ConfigFile: rewrite exact path or name tokens (tokenized match, never a substring) |
| String-based lookups | `getattr(obj, "name")`, `hasattr`, `obj["name"]`, `Reflect.get`, `Type.GetMethod("Name")`, `nameof`, decorators/annotations with names, `importlib.import_module("pkg.mod")`, `eval`-free dynamic dispatch tables | StringLiteral: auto-included when the string exactly equals the old name **and** sits in a recognized call position; other equal strings are listed as "possible refs" (unchecked by default) |
| Docs | Markdown links and code spans naming the symbol or path (`README.md`, `docs/**`) | Doc: opt-in "Also update docs" |
| Comments | the old name inside comments or docstrings | Comment: opt-in "Also update comments", off by default |

Dynamic references that cannot be proven (computed names, `eval`, reflection over runtime data) set
the `DYNAMIC_REFS` flag on the symbol: the grid shows 🔗 and the user decides.

**Never rename (reserved lists, data files):** language keywords and builtins
(`queries/<lang>/keywords.txt`, `globals.txt`), entry points (`main`, `__main__`), dunder and
special methods (`__init__`, `toString`, `Dispose`), framework hooks the project declares, test
functions (`test_*`, `#[test]`, `it(...)`), methods that override or implement a base/trait/interface
method (found in the index), and anything on the project's **ignore list** (stored per project;
editable in the UI; `RESERVED` flag).

### 5.5 Detecting bad names (`naming::check`)
Conventions per language and kind (from the official style guides):
| Language | Functions / variables | Types | Constants | Files |
|---|---|---|---|---|
| Python (PEP 8) | `snake_case` | `PascalCase` | `CONSTANT_CASE` | `snake_case.py` |
| JS / TS | `camelCase` | `PascalCase` | `CONSTANT_CASE` or `camelCase` | project style: `kebab-case`, `camelCase`, or `PascalCase` (components); detected from the majority |
| Rust | `snake_case` | `CamelCase` | `SCREAMING_SNAKE` | `snake_case.rs` |
| C# | `PascalCase` methods, `camelCase` locals, `_camelCase` private fields | `PascalCase` | `PascalCase` | `PascalCase.cs` = type name |
| Go | `MixedCaps` (exported) / `mixedCaps` | `MixedCaps` | `MixedCaps` | `snake_case.go` |
| Java | `camelCase` | `PascalCase` | `CONSTANT_CASE` | `PascalCase.java` = public class |

Rules, each with an id and severity:
- `case.mismatch` (the kind's convention is violated) · `len.too_short` (< 3 chars, except `i j k x y z dt id ui` and loop indices)
- `meaningless` (dictionary: `foo bar baz tmp temp stuff thing data2 test1 asdf obj val func1 doit doStuff`)
- `hungarian` (`strName`, `iCount`, `m_`, `g_`) · `abbrev` (`plyr`, `btn`, `mgr`, `calc`, `cnt`, `idx`; configurable allow-list)
- `numbered` (`enemy2`, `label_3`) · `bool.prefix` (bool vars without `is_/has_/can_/should_`, warn only when the type is known)
- `inconsistent.synonym` (`get_hp` / `fetch_health` / `read_life` in the same project → suggest one vocabulary)
- `file.case` (a file or folder name breaks the project's or language's convention) · `file.mismatch` (a file name does not match the main type it holds, where the language expects it) · `file.generic` (`utils`, `helpers`, `misc`, `new_file2`: warn only)
- `typo` (optional, `symspell`-style dictionary on split words)

### 5.6 Proposals (LLM, `rename_propose.md`)
Each batch has ≤ 20 symbols of the same kind and language. Each symbol is sent as:
```
<sym id="s17" kind="function" lang="python" convention="snake_case" file="player.py" issues="meaningless,abbrev">
def doStuff2(p):               # signature
  # 3 lines of body or doc
  usages: player.py:88 `doStuff2(enemy)` · hud.py:12 `player.doStuff2(target)`
</sym>
```
Files and folders are proposed the same way (`kind="file"`, with the first lines of the file, the
types it defines, and who imports it).

Schema (pattern chosen per batch from the convention):
```json
{"type":"object","additionalProperties":false,"required":["proposals"],
 "properties":{"proposals":{"type":"array","maxItems":20,"items":{
   "type":"object","additionalProperties":false,"required":["id","new_name","confidence","reason"],
   "properties":{
     "id":{"type":"string","enum":["s17","s18","…"]},
     "new_name":{"type":"string","pattern":"^[a-z][a-z0-9]*(_[a-z0-9]+)*$","maxLength":48},
     "confidence":{"type":"number","minimum":0,"maximum":1},
     "reason":{"type":"string","maxLength":120}}}}}}
```
Validation (Rust): the id exists and isn't duplicated · the new name ≠ the old name · not a
language keyword or builtin · matches the convention · **no collision** in the same scope, with
globals, or (files) with an existing sibling · not `RESERVED` · consistent with the
accepted-synonym vocabulary. Invalid → one reask. Still invalid → the item gets status
`needs_manual` (fallback).

### 5.7 Review grid → plan → apply → verify
Grid columns: ☑ · kind icon · old → **new** (editable, revalidated live) · refs (code / import /
config / string) · confidence bar · reason · issues · flags (⚠ ambiguous, 🔗 dynamic refs, 🌐 public API).
Bulk actions: accept all ≥ 0.8 · reject flagged · "include string refs" per row.

`plan::build_changeset(accepted)` → for each symbol, an `Edit` per file with exact byte ranges
of identifier nodes and accepted import, config, string, and doc refs. File and folder renames
become `Move` ops **plus** `Edit`s for every import path and config reference that points at them.
Overlap check. Base hashes from index time. Two details matter:
- **Case-only renames** (`Utils.py` → `utils.py`) are executed as two moves through a temporary
  name, because macOS and Windows file systems are case-insensitive.
- Folder moves are applied before the edits inside them, so edits target post-move paths.

If the project is a git repo with uncommitted changes, show the banner *"You have uncommitted
changes. Commit first so you can diff/revert with git too."* with the buttons [Continue anyway]
[Cancel]. Where the project is a git repo, file renames use plain moves (git detects renames).

After apply: re-index changed files and confirm that no old name remains in the planned
locations (a self-check). **Verify** (optional, never automatic): detect the project's own
checker from its manifest and offer to run it with a fixed argument list (never through a shell),
streaming the output: `cargo check` (Cargo.toml) · `tsc --noEmit` (tsconfig) · `python -m
compileall -q .` (Python) · `dotnet build --no-restore` (*.csproj) · `go build ./...` (go.mod) ·
`mvn -q -o compile` or `gradle --offline compileJava` (Java). A non-zero exit marks the verify as
failed and lists the first errors. The binary must be on PATH.

### 5.8 Strings mode (`strings_improve`)
Extract user-facing string literals (heuristics: UI call positions such as `text`, `label`,
`title`, `message`, `t()`/`gettext()`/`i18n.t()` arguments; log and `print` calls are excluded). The
LLM proposes improved text and/or an i18n key (`MENU_START_GAME`). Placeholders (`%s %d %0.2f
{name} {0}`) are extracted deterministically and **must match exactly** in proposals (validated).
Export the key/text table as CSV or JSON and, if wanted, rewrite code to `t("KEY")` (function name
configurable) via a change set.

### 5.9 Find: "where is this used, and what is this project?"
Rename Forge doubles as a code explorer, using the same index:
- **Find symbol:** search by name (exact, prefix, fuzzy) across symbols, files, and folders; results show kind, location, signature, doc, and a usage count by kind (code / import / config / string).
- **Usages:** every reference with a 1-line code preview, grouped by file; jump to the editor (M10).
- **Project overview:** languages and file counts, entry points, largest files and functions, the naming-issue summary, and import hot spots (most-imported files).
- The router assistants get a read-only tool `find_symbol(query)` that returns this data (budgeted), so "where is `calc_total` used?" in chat works after a project is indexed.
