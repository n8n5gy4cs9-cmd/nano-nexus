# CLAUDE.md

@AGENTS.md

## Claude Code specifics
- Skills for this repo live in `.claude/skills/`:
  - `next-task`: pick, implement, verify, tick, commit + push the next task from `tasks.md`.
  - `verify`: run the full verification suite and summarize failures.
  - `add-assistant`: create or modify an assistant TOML that passes prompt_lint.
- Commit attribution: follow your harness's attribution instructions. **Never put a model
  identifier in commit messages, code, or docs.**
- Prefer `scripts/progress.sh` over reading the whole `tasks.md` when you only need the next task.
- Large planning docs: read only the milestone section you need (they're split per milestone on purpose).
