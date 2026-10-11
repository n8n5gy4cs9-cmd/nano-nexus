---
name: add-assistant
description: Create or improve a NanoNexus assistant definition (assistants/*.toml) that follows the small-model prompt rules and passes prompt_lint. Use when asked to add, tune, or review an assistant or its system prompt.
---

# Add or tune an assistant

1. Read `docs/architecture/ASSISTANTS.md` §1–§2 (the spec and rules R1–R16) and `assistants/README.md`.
2. Start from `assistants/_TEMPLATE.toml`. The file name is `<id>.toml`, and the id is kebab-case.
3. Pick `workflow.kind`:
   - `chat` for persona-only help.
   - `router` when it needs to read granted files (list `tools.allowed`).
   - `pipeline` when an existing Rust pipeline fits (folder_report, doc_qa, doc_summarize,
     rename_propose, strings_improve, tidy_plan, secrets_report, log_triage, data_profile, commit_message).
4. Write `[prompt] system = '''…'''` **as the last table**:
   - First line: "Your job: …". Numbered steps. An explicit output format. A word limit.
   - Data comes in tags. Add a grounding rule for documents. Keep "Never" lines for real safety limits only.
   - ≤ 250 words (hard limit 450 tokens). Never include `'''` inside the prompt.
5. Add 2–4 starters that show what it's best at. Pick a lucide icon and one of the 9 colors.
6. Validate: `cargo test -p nexus-agent prompt_lint` (from M06). Before M06, check the TOML against
   `schemas/assistant.schema.json`.
7. Built-in assistants also need a row in `assistants/README.md` and `docs/PRD.md` §6.
8. Commit: `assistants: add <name>` (or the task ID if it's part of a task) and push to `main`.
