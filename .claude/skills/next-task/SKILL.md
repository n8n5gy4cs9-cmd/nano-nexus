---
name: next-task
description: Pick the next unchecked task from tasks.md, implement it exactly per its milestone spec and architecture contracts, verify with scripts/check.sh, tick it, and commit + push to main. Use when asked to continue the project, do the next task, or work through the plan.
---

# Next task

1. Run `scripts/progress.sh --next` to get the next task ID and title. If it's marked 🍎/🪟 and you
   aren't on that platform, do every non-platform part you can, note what remains in the task
   line, and move on to the next task.
2. Open `tasks.md` and find the task's milestone header. Read that milestone doc in
   `docs/milestones/` (Scope, Design notes, Steps, Pitfalls) and any architecture doc it links.
3. Write a short plan: files, types and functions, tests. Keep to the task's scope.
4. Implement. Follow `docs/agent-playbook/CONVENTIONS.md` and the rules in `AGENTS.md`.
5. Add tests. Run `scripts/check.sh` until green.
6. Self-review with `docs/agent-playbook/REVIEW_CHECKLIST.md`.
7. Tick the task in `tasks.md` (`- [x] **<ID>** … (YYYY-MM-DD)`), then run `scripts/progress.sh --write`.
8. Commit and push:
   ```bash
   git add -A && git commit -m "<ID>: <imperative summary>" && git push origin main
   ```
9. Report the task done, the check results, and anything left for the owner.

If blocked: write the question in `docs/QUESTIONS.md`, mark the task `⛔ blocked: see QUESTIONS Q-n`,
commit + push, then continue with the next unblocked task.
