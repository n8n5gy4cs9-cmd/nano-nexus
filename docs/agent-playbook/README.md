# Agent Playbook

Everything another AI (or you) needs to continue NanoNexus reliably.

| File | Use it for |
|---|---|
| [`SYSTEM_PROMPT.md`](SYSTEM_PROMPT.md) | The system prompt to give any continuing coding AI (full and short versions) |
| [`TASK_PROMPT_TEMPLATE.md`](TASK_PROMPT_TEMPLATE.md) | Per-task message template, with a filled example |
| [`MILESTONE_PROMPTS.md`](MILESTONE_PROMPTS.md) | One kickoff prompt per milestone (M00–M15) |
| [`CONVENTIONS.md`](CONVENTIONS.md) | Rust, TypeScript, prompt, git, and docs conventions |
| [`REVIEW_CHECKLIST.md`](REVIEW_CHECKLIST.md) | Pre-commit self-review |
| [`GOTCHAS.md`](GOTCHAS.md) | Known pitfalls, each with symptom → cause → fix |

Also: root [`AGENTS.md`](../../AGENTS.md) (rules), [`CLAUDE.md`](../../CLAUDE.md) (Claude Code),
and [`.claude/skills/`](../../.claude/skills) (`next-task`, `verify`, `add-assistant`).

**Recommended loop for a smaller model:**
1. System prompt = `SYSTEM_PROMPT.md` (full version).
2. When a milestone starts, send its kickoff prompt from `MILESTONE_PROMPTS.md`.
3. For each task, send `TASK_PROMPT_TEMPLATE.md` filled with the task's spec excerpt.
4. After each task: `scripts/check.sh` is green → tick → commit → push to `main`.
