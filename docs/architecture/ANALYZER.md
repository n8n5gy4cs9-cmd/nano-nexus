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
`crates/nexus-analyze/src/categories.rs` (≥ 250 extensions). Examples: `.gd .tscn .tres .godot` → GameAsset/Code (`.gd` = Code;
`.tscn/.tres/.import` = GameAsset); `.dmg .pkg .exe .msi` → Installer.

### Junk rules (each with an id and a "safe to trash?" flag)
`.DS_Store`, `Thumbs.db`, `desktop.ini`, `~$*.docx` (Office lock files), `*.tmp`, `*.crdownload`, `*.part`
(stale > 7 days), `node_modules/` (size-reported, **not** auto-trash), `target/` (Rust), `.godot/` (Godot 4
import cache: regenerable), `__pycache__/`, `.gradle/`, `DerivedData/`, `*.log` > 50 MB.
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

### 5.1 Pipeline overview
```
Pick project (grant: ReadWrite needed to apply; Read is enough to analyze)
 → Index (tree-sitter + adapters)          → SymbolIndex
 → Detect issues (naming rules)            → candidates
 → Propose (LLM, batches of ≤ 20)          → proposals (validated)
 → Review grid (user accepts/edits)        → accepted set
 → Plan (exact byte ranges in all refs)    → ChangeSet (+ conflicts)
 → Preview diff → Apply (journal) → Verify (optional: Godot headless check, see §5.7)
 → Undo available in Changes
```

### 5.2 Index model
```rust
pub struct Symbol {
  pub id: SymbolId, pub name: String, pub kind: SymbolKind, pub lang: Lang,
  pub def: Location /* file, byte range, line */, pub scope: ScopeKey /* file::class::func */,
  pub signature: String,          // e.g. "func calc(a, b) -> int"
  pub doc: Option<String>,        // preceding comment
  pub refs: Vec<Ref>,             // all occurrences, including def
  pub flags: SymbolFlags,         // ENGINE_VIRTUAL, EXPORTED, PUBLIC_API, DYNAMIC_REFS, AUTOLOAD, SIGNAL, OVERRIDE
}
pub enum SymbolKind { Function, Method, Variable, Constant, Parameter, Field, Class, Enum, EnumMember, Signal, Node, File }
pub struct Ref { pub loc: Location, pub kind: RefKind }   // Code | StringLiteral | SceneFile | ProjectFile | Comment
```

Resolution is **name + scope based** (not a full type checker). Local variables and parameters
resolve within the function scope. Members resolve within the class (file) and through
`self.`/bare access. Cross-file references to `class_name` types and autoloads resolve by global
name. Ambiguous cases (same member name in two classes accessed through an untyped variable,
e.g. `enemy.hit()`) are marked `AMBIGUOUS` and must be confirmed by the user per occurrence in
the review grid.

### 5.3 Language adapters
| Language | Grammar crate | Notes |
|---|---|---|
| GDScript | `tree-sitter-gdscript` 6.1 | First-class. Godot adapter below. Spike S4: ABI compatibility with `tree-sitter` 0.27 |
| Rust | `tree-sitter-rust` 0.24 | Skip trait impl methods (`OVERRIDE`) and `pub` items flagged `PUBLIC_API` (warn) |
| Python | `tree-sitter-python` 0.25 | Dunder methods never renamed |
| JavaScript / TypeScript | `tree-sitter-javascript` 0.25 / `tree-sitter-typescript` 0.23 | Exported names are flagged `PUBLIC_API` |
| C# | `tree-sitter-c-sharp` 0.23 | Unity messages (`Start`, `Update`, `Awake`, …) → `ENGINE_VIRTUAL` |
| Lua | `tree-sitter-lua` 0.5 | LÖVE callbacks (`love.load`, `love.update`, `love.draw`) → `ENGINE_VIRTUAL` |

Each adapter is a `.scm` tags query file in `crates/nexus-analyze/queries/<lang>/tags.scm`
(the tree-sitter "tags" convention: `@definition.function`, `@reference.call`, …) plus a small
Rust resolver.

### 5.4 Godot adapter (the important one for your game)
Godot code refers to names in places that aren't GDScript identifiers. All of them must be
indexed as `Ref`s, or a rename breaks the game.

| Where | Pattern | Ref kind / action |
|---|---|---|
| `.tscn` / `.tres` signal connections | `[connection signal="pressed" from="Button" to="." method="_on_button_pressed"]` | SceneFile: rewrite the `method="…"` value (and `signal="…"` for custom signals) |
| `.tscn` exported property values | inside `[node …]`: `move_speed = 300.0` where `move_speed` is an `@export var` of the attached script | SceneFile: rewrite the key (otherwise the saved value is silently lost) |
| AnimationPlayer method tracks | `"method": &"spawn_enemy"` in `tracks/N/keys` | SceneFile |
| `project.godot` autoloads | `[autoload] GameState="*res://autoload/game_state.gd"` | ProjectFile: autoload names are globals used in code |
| `project.godot` input actions | `[input] move_left={…}` used as `Input.is_action_pressed("move_left")` | StringLiteral: "action rename" mode (opt-in) |
| String-based calls | `call("x")`, `call_deferred("x")`, `callv`, `has_method("x")`, `Callable(obj, "x")`, `connect("sig", …)`, `emit_signal("sig")`, `rpc("x")`, `set("p")`, `get("p")`, `set_deferred("p")`, `tween.tween_property(o, "p", …)` | StringLiteral: auto-included when the string exactly equals the old name **and** sits in one of these call positions; other equal strings are listed as "possible refs" (unchecked by default) |
| `$NodePath` / `get_node("A/B")` | node names | Not identifiers. Out of scope unless "Node rename" mode is enabled (P2) |
| File renames (`res://` paths, `.uid` sidecars) | `ext_resource path="res://…"`, `preload("res://…")` | P2 "file rename mode": update every `res://` reference and move the `.uid` file along |

**Never rename (ENGINE_VIRTUAL deny list):** `_init _ready _process _physics_process _input
_unhandled_input _unhandled_key_input _shortcut_input _gui_input _enter_tree _exit_tree
_notification _draw _get _set _get_property_list _property_can_revert _property_get_revert
_validate_property _to_string _get_configuration_warnings _integrate_forces _static_init
_can_drop_data _drop_data _get_drag_data _make_custom_tooltip _has_point _structured_text_parser
_get_minimum_size _run` (EditorScript) and every method that overrides a built-in class method
(detected via a bundled list of Godot 4.x built-in method names per class, generated from
`extension_api.json`, in `queries/gdscript/builtin_methods.txt`).

**GDScript style guide targets** (official Godot docs, "GDScript style guide"):
| Kind | Convention | Example |
|---|---|---|
| File names | snake_case | `yaml_parser.gd` |
| `class_name`, node names, enums (type) | PascalCase | `class_name YAMLParser` |
| Functions, variables, parameters | snake_case | `func load_level()` |
| Private/internal members | leading underscore + snake_case | `var _counter` |
| Signals | snake_case, **past tense** | `signal door_opened` |
| Constants, enum members | CONSTANT_CASE | `const MAX_SPEED = 200` |

**Godot 3 → 4 modernization hints** (flagged, never auto-applied): `yield(` → `await`, `onready var` →
`@onready var`, `export var` → `@export var`, `.instance()` → `.instantiate()`, `KinematicBody2D` →
`CharacterBody2D`, `connect("s", self, "m")` → `s.connect(m)`, `rand_range` → `randf_range`,
`deg2rad` → `deg_to_rad`, `PoolStringArray` → `PackedStringArray`, `setget` → property syntax.
The report recommends running Godot's own converter first: `godot --headless --convert-3to4`
(make a git commit before and after).

### 5.5 Detecting bad names (`naming::check`)
Rules, each with an id and severity:
- `case.mismatch` (the kind's convention is violated) · `len.too_short` (< 3 chars, except `i j k x y z dt id ui` and loop indices)
- `meaningless` (dictionary: `foo bar baz tmp temp stuff thing data2 test1 asdf obj val func1 doit doStuff`)
- `hungarian` (`strName`, `iCount`, `m_`, `g_`) · `abbrev` (`plyr`, `btn`, `mgr`, `calc`, `cnt`, `idx`; configurable allow-list)
- `numbered` (`enemy2`, `label_3`) · `signal.tense` (a signal not in past tense, e.g. `signal open_door`)
- `bool.prefix` (bool vars without `is_/has_/can_/should_`, warn only when the type is known)
- `inconsistent.synonym` (`get_hp` / `fetch_health` / `read_life` in the same project → suggest one vocabulary)
- `typo` (optional, `symspell`-style dictionary on split words)

### 5.6 Proposals (LLM, `rename_propose.md`)
Each batch has ≤ 20 symbols of the same kind and language. Each symbol is sent as:
```
<sym id="s17" kind="function" lang="gdscript" convention="snake_case" file="player.gd" issues="meaningless,abbrev">
func doStuff2(p):              # signature
  # 3 lines of body or doc
  usages: player.gd:88 `doStuff2(enemy)` · hud.gd:12 `player.doStuff2(target)`
</sym>
```
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
language keyword or builtin · matches the convention · **no collision** in the same scope or
with globals (class_names, autoloads) · not in ENGINE_VIRTUAL · consistent with the
accepted-synonym vocabulary. Invalid → one reask. Still invalid → the item gets status
`needs_manual` (fallback).

### 5.7 Review grid → plan → apply
Grid columns: ☑ · kind icon · old → **new** (editable, revalidated live) · refs (code / scene /
string) · confidence bar · reason · issues · flags (⚠ ambiguous, 🔗 dynamic refs, 🌐 public API).
Bulk actions: accept all ≥ 0.8 · reject flagged · "include string refs" per row.

`plan::build_changeset(accepted)` → for each symbol, an `Edit` per file with exact byte ranges
of identifier nodes and accepted string or scene refs. Overlap check. Base hashes from index
time. If the project is a git repo with uncommitted changes, show the banner *"You have
uncommitted changes. Commit first so you can diff/revert with git too."* with the buttons
[Continue anyway] [Cancel].

After apply: re-index changed files and confirm that no old name remains in the planned
locations (a self-check). If `godot` is on PATH or configured in Settings, offer
**Verify with Godot**: for each changed script, run
`godot --headless --path <project> --check-only --script <res://file.gd>` (`--check-only` must be
paired with `--script`), then `godot --headless --path <project> --import` (Godot ≥ 4.3), which
loads the whole project and reports parse errors and broken resources. The output is streamed
into the panel and any `ERROR`/`SCRIPT ERROR` line marks the verify as failed.

### 5.8 Strings mode (`strings_improve`)
Extract user-facing string literals (heuristics: inside `text =`, `label.text =`, `tr()`,
`print()` excluded, `.tscn` `text = "…"` properties). The LLM proposes improved text and/or an
i18n key (`MENU_START_GAME`). Placeholders (`%s %d %0.2f {name} {0}`) are extracted
deterministically and **must match exactly** in proposals (validated). Export
`translations.csv` in Godot's format (`keys,en`) and, if wanted, rewrite code to
`tr("KEY")` via a change set.
