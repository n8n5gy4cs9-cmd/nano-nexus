# M06: Assistants, Prompts, Tools, Router

**Goal:** the 17 built-in assistants are live in a gallery, users can create, duplicate,
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
1. Spec + validation + Registry + prompt_lint (all 17 built-ins must pass; fix the prompts if not).
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

## REFE repair follow-up (owner request, 2026-10-22)

T06.14 repairs the existing router rather than replacing its permission or journal contracts.
Reference: `REFE/pi-main/packages/agent/src/agent-loop.ts` appends actual tool results to the
context before the next inference call. Port that observation-feedback behavior to our
constrained-JSON router; the final answer and intermediate decisions share the bounded result
budget. Preserve exact `edit_file.old_text` bytes, including indentation and trailing newlines.

Acceptance: a deterministic router test must inspect the next decision's messages for the
contents returned by `read_file`. A read → write proposal test must use the real tool registry,
verify no file exists before Apply, then apply via `Journal::apply` and assert file contents.
Keep loop limits, repeated-call protection, deny rules, and permission checks intact.

This is a behavioral port into Rust; it does not introduce Pi's process execution or direct
filesystem writes. Existing REFE checkouts are local reference material, not bundled dependencies.
Concurrent Duo inference remains T18.04; Duo tier selection alone is not sub-agent delegation.

T06.15 keeps observations useful under context pressure. Select tool results newest first within
the existing 2 400-token shared budget; allocate final-answer attachment space to tool results
before user attachments. Preserve token-accounting indices for the original attachment list.
Regression tests must demonstrate that a large early read cannot hide a later pending-write
status and a large user attachment cannot hide it from the final inference request. This applies
to every assistant using the shared router/turn engine, including Agent and Duo.

T06.16 adds an ignored live-model test through the application chat service, not just a scripted
router. With `NEXUS_MODEL` pointing to an existing GGUF, start the real runtime on loopback, grant
a temporary work folder, enable automatic journal apply only in the isolated test context, and
verify Agent create/read/edit, Reviewer consultation, and Duo create. Inspect persisted tool
traces and exact disk contents; always shut down the runtime, including on failed assertions.
Do not copy models into fixtures or build outputs. Bound each turn with a cancellation timeout.

The live Gemma repro returned `write_file` with `path` and no `content`, twice even after a
correction request. T06.16 therefore also repairs the routing contract: a union of flat action
objects requires each action's arguments during decoding, and Rust validates required arguments
before a correction retry. Add a conditional write example (skip empty example pairs when a
feature is absent), retain invalid-decision errors in final context, and tag plain tool statuses
consistently. Keep the original prompt budgets, grants, loop guards, and journal behavior.

T06.17 repairs the intent gate for non-file tools. Arithmetic requests containing numbers and
operators/percentage/math words must route when `calculate` is available; current time/date
requests must route when `now` is available. Do not enable disabled tools. Keep general small
talk gated off. Add a failing intent regression and extend the ignored live chat-service test
to require successful calculator/clock tool traces and a grounded numeric answer.

T06.17 also handles a repeated live-model failure: MiniCPM may stop after reading a file or
repeat that read without performing the requested edit. For explicit file-change imperatives
with a path/filename, permit one bounded completion correction. Constrain that correction to
relevant actions (read first for an unobserved edit; edit/write for an observed edit; move or
trash only for those explicit requests). Do not execute a repeated call twice. If no change
was proposed/applied, return a deterministic incomplete report rather than a generated success
claim. Explanatory questions and general writing do not trigger this correction.
Current clock questions call the available `now` tool directly before the final answer.
