# M06: Assistants, Prompts, Tools, Router

**Goal:** the 18 built-in assistants are live in a gallery, users can create, duplicate,
import, and export their own, and the `router` workflow lets assistants read granted files and
folders through tools.

**Spec:** [ASSISTANTS.md](../architecture/ASSISTANTS.md), `schemas/assistant.schema.json`,
`assistants/*.toml`, `prompts/internal/*.md`.
**Prereqs:** M05. **Exit demo:** in Nexus, ask "what's in ~/Downloads/notes.md?" → the permission
prompt appears → Allow → a tool chip shows "Read notes.md" → a grounded answer. Then build
"Regex Smith" in the builder, test it in the side panel, save it, and export it.

## Scope
- `nexus-agent`: `AssistantSpec` (serde + schema validation via `jsonschema` against the JSON
  form of the TOML), `Registry` (built-ins via `include_dir!("$CARGO_MANIFEST_DIR/../../assistants")`,
  user ones from the store), internal prompt loader (`include_dir!` of `prompts/internal`), full
  context builder, `Tool` trait + tools (`read_file`, `list_dir`, `search_files`, `grep_text`,
  `file_info`, `calculate`, `now`; `scan_folder` and `doc_search` are stubs until M08), router
  workflow with `needs_tools_heuristic`, loop guard, and hop limit.
- **prompt_lint** test (ASSISTANTS §8.1).
- Store: `assistants` repo (user TOML + built-in overrides for enabled/sort).
- Commands: `assistants_*`, `assistant_validate` (returns field errors + token count).
- UI: Assistants gallery, assistant switcher (⌘/), builder sheet with test chat, chat tool chips,
  and pipeline-assistant placeholders ("Available in M08/M09": the card is disabled with a tooltip).

## Design notes
- The TOML → `AssistantSpec` mapping must reject unknown keys (`#[serde(deny_unknown_fields)]`) so typos surface.
- **Token counting in the builder:** if the runtime is up, use `/tokenize`, otherwise chars/3.2.
  Show green ≤ 350 / amber ≤ 450 / red > 450.
- **Router validation** (Rust, after `chat_json`): the action must be allowed, path args are
  resolved (`~` expanded; bare names matched against granted root basenames), and the same
  `(tool,args)` pair can't be called twice in one turn.
- **Tool output budget:** each tool truncates to a budget (default 1 200 tokens) and appends
  `[truncated: 120 more entries]`.
- The **Prompt Architect** output contract: its final answer contains one fenced ```toml block.
  The chat UI detects `schema_version = 1` inside a toml block and shows **Open in builder**.
- `calculate`: use a tiny safe evaluator (`meval` 0.2 or a hand-written shunting-yard) with
  numbers and `+ - * / ^ % ( )`, `sqrt`, `round`, and nothing else.

## Steps
1. Spec + validation + Registry + prompt_lint (all 18 built-ins must pass; fix the prompts if not).
2. Prompt rendering with all variables + shared preamble; snapshot tests with `insta`.
3. Tools + permission integration (via the broker from M05) + ToolOutput budgeting + tests on temp dirs.
4. Router workflow + heuristic + tests with a scripted mock LLM (a queue of canned JSON replies).
5. Store + commands.
6. Gallery UI, switcher, builder + test chat (it uses a draft spec, sent with `chat_send` as `draft_spec`).
7. Tool chips and status lines in chat. Import and export via dialogs.

## Exit criteria
- [ ] prompt_lint passes for all built-ins. A broken user TOML shows field errors.
- [ ] Router: a 20-prompt manual eval on the real model (`docs/SPIKES.md` §router) with ≥ 80% correct tool choice; failures fall back to `answer` gracefully.
- [ ] Denied permission → the assistant explains it couldn't access the folder (no crash, no hallucinated content).

## Pitfalls
- 1B models over-call tools. The heuristic gate matters, so measure with and without it.
- `include_dir!` paths are relative to the crate. Add `cargo:rerun-if-changed=../../assistants` in a `build.rs` so edits trigger rebuilds.
