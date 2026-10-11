# M10: Code Editor

**Goal:** a small, fast, good-looking editor for quick edits and AI-assisted changes, fully
inside the permission and journal model.

**Spec:** UI-DESIGN §4.6, PERMISSIONS §5 (Write op), DATA-AND-IPC (`editor_*`, `fs_*`).
**Prereqs:** M05, M06. **Exit demo:** open a project folder → open `player.py` (Python
highlighted) → select a function → "Improve names" → a diff appears → accept 2 of 3 hunks →
⌘S → saved through a change set → Undo in Changes reverts it.

## Scope
- File tree of granted roots (lazy `fs_list_dir`, `.gitignore`-aware toggle, file-type icons).
- Tabs (dirty dot, middle-click close, reopen closed ⌘⇧T), a session restore list in settings.
- CodeMirror 6 setup: theme from tokens, line numbers, folding, bracket matching, multiple
  selections, search panel, go-to-line, indentation detection, word wrap toggle, minimap off.
- Languages: `@codemirror/lang-javascript` (js/ts/jsx/tsx), `-rust`, `-python`, `-json`,
  `-markdown`, `-html`, `-css`, `-cpp`, `-java`, `-sql`, `-xml`, `-yaml`, `-php`, plus
  `@codemirror/legacy-modes` (shell, lua, toml, csharp via clike, swift, go, ruby, ini).
- Save: `editor_save(path, contents, base_hash)` → a single Write op → confirm popover ("Save
  changes to player.py?" with an "Always allow for this folder (session)" option) → apply → journal.
  Read-only mode when there's no write grant (with a banner and a "Grant write access" button).
- AI actions panel: Explain · Improve names · Add docs · Find bugs · Simplify · Ask (free text).
  Edits come back as full-file proposals from the model → **we compute the diff locally** →
  `@codemirror/merge` unifiedMergeView with per-chunk accept/reject.
- External change detection: poll mtime on focus. If the file changed and the buffer is clean →
  reload. If dirty → banner "File changed on disk" [Reload] [Keep mine].

## Design notes
- Small models can't reliably produce unified diffs. Ask for the **whole rewritten
  selection** in one fenced block (`prompts/internal/editor_rewrite.md`), extract the first code
  fence, and diff it against the original selection. If the result is > 2× or < 0.3× the original
  length, warn "suspicious rewrite".
- Large files: > 2 MB opens read-only with a warning. > 20 MB refuses (use Analyzer).

## Steps
1. Editor shell (tree + tabs + CodeMirror + theme). 2. Languages (+ snapshot test of tokens).
3. Save via change set + read-only mode. 4. AI actions + merge view. 5. External change detection. 6. Shortcuts and polish.

## Exit criteria
- [ ] Exit demo passes. Typing in a 10k-line file stays smooth (no dropped frames > 50 ms).
- [ ] No save path bypasses `editor_save` → change set.

## Pitfalls
- CodeMirror and React: create the `EditorView` once per tab and keep `EditorState`s in a Map. Don't re-create on every render.
