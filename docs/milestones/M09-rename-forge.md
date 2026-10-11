# M09: Rename Forge

**Goal:** safely rename badly named functions, variables, signals, constants, classes, and
(opt-in) strings in a legacy project, starting with your old Godot game. Every reference is
updated (code, scenes, project file, string calls), you review everything, and one click undoes it.

**Spec (normative):** [ANALYZER.md §5](../architecture/ANALYZER.md#5-rename-forge), `prompts/internal/rename_propose.md`,
`prompts/internal/strings_improve.md`, `schemas/llm/rename-proposals.schema.json`.
**Prereqs:** M08, spike S4. **Exit demo:** on a **copy** of the game: index → 300+ issues →
propose → accept ~200 → plan → diff → apply → Verify with Godot (no script errors) → run the game →
undo → the files are byte-identical to the originals.

## Scope
1. `nexus-analyze::code`: tree-sitter setup, language detection, tags queries per language,
   symbol index with scopes and refs, serialization to `cache/<session>/index.bin` (bincode or
   postcard), incremental re-index of changed files.
2. Godot adapter: `.tscn/.tres` parser (INI-like sections; a hand-written tolerant line parser),
   `project.godot` parser, string-call detection, ENGINE_VIRTUAL list, built-in method list
   (generated from Godot's `extension_api.json` by `scripts/gen-godot-builtins.mjs`, committed as a text file).
3. Naming rules engine (ANALYZER §5.5) + per-language conventions + case conversion (`heck`).
4. Proposal pipeline: batching, prompt rendering, `chat_json` with pattern, validator (keywords,
   collisions, convention, deny lists), fallback `needs_manual`.
5. Planner → ChangeSet (byte ranges), overlap checks, git-dirty detection (`git status --porcelain`).
6. Verify with Godot (if found): headless check + import. Output is streamed.
7. Strings mode + Godot translation CSV export (+ optional `tr()` rewrite change set).
8. Store migration 0005, commands `rename_*`, jobs, notifications ("312 proposals ready").
9. UI: stepper, issues grid (TanStack Table + Virtual), occurrence drawer with code previews,
   inline edit with live validation, bulk actions, Plan & preview (reuses Changes), Verify panel,
   Strings tab, Godot 3→4 hints panel.

## Design notes
- **Tags queries** (`queries/gdscript/tags.scm`): capture `function_definition name`,
  `variable_statement name`, `const_statement name`, `signal_statement name`, `class_name_statement name`,
  `enum_definition name` + enumerators, `parameters/identifier`, plus references (`identifier`,
  `attribute` → `attribute_call`/`attribute_subscript`). Exact node names must be checked against
  the grammar's `node-types.json` during spike S4. Write a test that dumps the S-expression of a
  sample file and assert against it.
- **Scope resolution:** each GDScript file is one class. Inner `class` blocks are nested scopes.
  A reference `foo` resolves local → parameter → member (including inherited via `extends`
  chain *within the project*) → global (class_name, autoload, engine). `obj.foo` with an unknown
  `obj` type resolves to all project classes defining `foo` → AMBIGUOUS if more than one.
- **Byte ranges:** tree-sitter gives byte offsets in UTF-8, the same as our `TextEdit`. Read files
  as bytes and decode as UTF-8 (lossy only for display). If a file isn't UTF-8, skip it with a warning.
- **Collision check:** after applying all accepted renames in memory (a name map per scope), no
  scope may have two symbols with the same final name, and no new name may shadow an engine
  global (`print`, `Vector2`, `Input`, …; list in `queries/gdscript/globals.txt`).
- **Proposal context budget:** each `<sym>` block ≤ 120 tokens. Batch ≤ 20 syms ≈ 2 400 tokens + prompt.
- **Re-propose:** the user can select rows and click "Try again" (temperature +0.2, the previous
  proposal is listed as rejected in the prompt).

## Steps
1. Spike S4 and grammar pin. `queries/gdscript/*.scm` + tests on fixture projects (a tiny Godot 3
   and Godot 4 project in `tests/fixtures/godot3_game`, `godot4_game`, written by hand).
2. Index + scopes + refs + serialization (+ tests: every ref found and no false positives on fixtures).
3. Godot adapter (scenes, project, string calls, virtuals, builtins) + tests.
4. Naming rules + tests (a table of names → expected issues).
5. Proposal pipeline + validator + tests with a scripted mock LLM.
6. Planner + change set + git-dirty + tests (apply plan on the fixture → re-index → old names gone at planned locations).
7. Verify with Godot.
8. Strings mode + CSV export.
9. Store, commands, UI.
10. **Real run on a copy of your game.** Fix every issue found and record notes in `docs/SPIKES.md` §rename-real-run.
11. Other languages (Rust, TS/JS, Python, C#, Lua) as follow-up tasks (P1).

## Exit criteria
- [ ] Fixture projects: rename everything flagged → the Godot fixture still loads with zero errors (if Godot is installed) and tests pass.
- [ ] Engine virtuals are never proposed (test).
- [ ] Scene `method="…"` connections and exported property keys are updated (tests).
- [ ] Undo restores byte-identical files (test).
- [ ] Real game: the exit demo passes. You can play the game.

## Pitfalls
- Godot 3 vs 4 syntax differs (`export var` vs `@export var`, `onready`). The grammar version must
  parse both, or you need two parse paths. Spike S4 decides.
- `.tscn` files can embed GDScript (`[sub_resource type="GDScript"]` with `script/source = "…"`).
  Index the embedded source too (offsets inside the string, with escapes accounted for), or flag
  it as "embedded script: manual".
- Don't rename inside comments by default (offer an "Also update comments" option, off by default).
