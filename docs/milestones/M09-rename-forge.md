# M09: Rename Forge

**Goal:** one assistant that can go through the code files **and folders** of any project and
rename them with ease: functions, variables, classes, constants, files, folders, and (opt-in)
strings. Every reference is updated (code, imports, config files, string lookups), you review
everything, and one click undoes it. It can also find where any name is used and give an overview
of the project. No language or framework gets special treatment.

**Spec (normative):** [ANALYZER.md §5](../architecture/ANALYZER.md#5-rename-forge), `prompts/internal/rename_propose.md`,
`prompts/internal/strings_improve.md`, `schemas/llm/rename-proposals.schema.json`.
**Prereqs:** M08, spike S4. **Exit demo:** on a **copy** of one of your own projects: index → 300+
issues (symbols, files, folders) → propose → accept ~200 → plan → diff → apply → Verify with the
project's own checker (build/type-check passes) → undo → the files are byte-identical to the originals.
Then ask "where is `calc_total` used?" and get every usage.

## Scope
1. `nexus-analyze::code`: tree-sitter setup, language detection, tags queries per language
   (Python, JS/TS/JSX/TSX, Rust, C#, Go, Java first), symbol index with scopes, refs, and the
   file/folder tree, serialization to `cache/<session>/index.bin` (bincode or postcard),
   incremental re-index of changed files.
2. Import/module resolution per language (so file and folder renames rewrite import paths and
   symbol renames follow `import`/`use`/`using`).
3. Non-code references: config/manifest scanner (`package.json`, `tsconfig`, `Cargo.toml`,
   `pyproject.toml`, `*.csproj`, `go.mod`, YAML/JSON/TOML), string-lookup detection, opt-in docs/comments.
4. Reserved-name lists per language (keywords, builtins, entry points, dunders, tests, overrides)
   plus a per-project ignore list.
5. Naming rules engine (ANALYZER §5.5) for symbols, files, and folders + per-language conventions + case conversion (`heck`).
6. Proposal pipeline: batching, prompt rendering, `chat_json` with pattern, validator (keywords,
   collisions, convention, deny lists), fallback `needs_manual`.
7. Planner → ChangeSet (byte ranges + moves, case-only renames via a temp name), overlap checks,
   git-dirty detection (`git status --porcelain`).
8. Find: symbol and usage search, project overview, `find_symbol` router tool.
9. Verify (optional): the project's own checker with a fixed argument list, output streamed.
10. Strings mode + CSV/JSON export (+ optional `t("KEY")` rewrite change set).
11. Store migration 0005, commands `rename_*`, jobs, notifications ("312 proposals ready").
12. UI: stepper, issues grid (TanStack Table + Virtual) with Symbols / Files & folders tabs,
    occurrence drawer with code previews, inline edit with live validation, bulk actions,
    Plan & preview (reuses Changes), Verify panel, Strings tab, Find tab.

## Design notes
- **Tags queries** (`queries/<lang>/tags.scm`): capture function, method, class/type, variable,
  constant, field, parameter, and enum definitions, plus references (identifiers, member access,
  calls) and imports. Exact node names must be checked against each grammar's `node-types.json`
  during spike S4. Write a test per language that dumps the S-expression of a sample file and
  asserts against it.
- **Scope resolution:** a reference `foo` resolves local → parameter → enclosing class/member →
  module → imports → global. `obj.foo` with an unknown `obj` type resolves to all project types
  defining `foo` → AMBIGUOUS if more than one.
- **Files and folders are symbols** (`SymbolKind::File`/`Folder`): their "references" are the
  imports and config entries that name them. Renaming a folder rewrites every path below it.
- **Byte ranges:** tree-sitter gives byte offsets in UTF-8, the same as our `TextEdit`. Read files
  as bytes and decode as UTF-8 (lossy only for display). If a file isn't UTF-8, skip it with a warning.
- **Collision check:** after applying all accepted renames in memory (a name map per scope and per
  folder), no scope or folder may have two entries with the same final name (compare
  case-insensitively for files), and no new name may shadow a language builtin or global.
- **Case-only file renames** go through a temporary name (two moves) because macOS and Windows
  file systems are case-insensitive.
- **Proposal context budget:** each `<sym>` block ≤ 120 tokens. Batch ≤ 20 syms ≈ 2 400 tokens + prompt.
- **Re-propose:** the user can select rows and click "Try again" (temperature +0.2, the previous
  proposal is listed as rejected in the prompt).
- **Verify never runs by itself.** It needs the checker on PATH, uses fixed argument lists (never
  a shell), and is cancelable.

## Steps
1. Spike S4 and grammar pins. `queries/<lang>/*.scm` + tests on small fixture projects (one per
   language, hand-written, with cross-file refs, imports, string lookups, and config files).
2. Index + scopes + refs + imports + serialization (+ tests: every ref found and no false positives on fixtures).
3. Non-code refs, reserved lists, and the ignore list + tests.
4. Naming rules + tests (a table of names → expected issues, for symbols and for files/folders).
5. Proposal pipeline + validator + tests with a scripted mock LLM.
6. Planner + change set + git-dirty + tests (apply plan on the fixture → re-index → old names gone at planned locations; folder rename updates imports).
7. Find + overview + `find_symbol` tool.
8. Verify.
9. Strings mode + export.
10. Store, commands, UI.
11. **Real run on a copy of your own project.** Fix every issue found and record notes in `docs/SPIKES.md` §rename-real-run.
12. More languages (Kotlin, Swift, PHP, Ruby, C/C++, Lua) as follow-up tasks (P1), one query file each.

## Exit criteria
- [ ] Fixture projects: rename everything flagged → each fixture still builds or type-checks (when its tool is installed) and its tests pass.
- [ ] Reserved names (entry points, dunders, overrides, tests) are never proposed (test).
- [ ] A folder rename rewrites every import path and config reference below it (test).
- [ ] A case-only file rename works on a case-insensitive volume (test with the temp-name path).
- [ ] Undo restores byte-identical files and folders (test).
- [ ] Find: `calc_total` returns every usage on the fixtures, grouped by kind (test).
- [ ] Real project copy: the exit demo passes and the project still builds.

## Pitfalls
- Grammar versions must match the `tree-sitter` ABI (spike S4 decides pins or patches).
- TypeScript path aliases and extension-less imports are the usual source of missed references: resolve via `tsconfig` `paths`.
- Python relative imports and `__init__.py` re-exports; Rust `mod`/`pub use` chains; Go packages are folders: renaming a folder changes the package path.
- Don't rename inside comments or docs by default (offer "Also update comments/docs", off by default).
- Public API names (exported, `pub`, capitalized in Go) may be used by other repos: flag them and default to unchecked.
