---
name: verify
description: Run NanoNexus's full verification suite (scripts/check.sh plus task-specific checks) and summarize failures with likely causes. Use before committing, after big changes, or when asked to check the project.
---

# Verify

1. `scripts/doctor.sh --quiet` (toolchain sanity; add `--windows` for cross-build tasks).
2. `scripts/check.sh`: Rust fmt/clippy/tests, frontend lint/typecheck/tests, JSON and shell syntax.
3. If the change touched inference and you're on the Mac with the runtime + model:
   `cargo test -p nexus-llm -- --ignored`.
4. If the change touched assistants or prompts: `cargo test -p nexus-agent prompt_lint` (from M06).
5. If the change touched bundling: `node scripts/fetch-model.mjs --verify`, plus `node scripts/verify-payload.mjs <app>` (from M12).

Report as a table: check | result | first error line | likely cause (see `docs/agent-playbook/GOTCHAS.md`).
Don't "fix" failing tests by weakening assertions. Fix the code, or ask in `docs/QUESTIONS.md`.
