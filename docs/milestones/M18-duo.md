# M18: Duo assistant (light + strong model)

**Goal:** one assistant that answers easy questions fast with a small model and hard ones with a
better model. The user chooses which installed model is which.

## v0.1 design (shipped, T18.01)
- Assistant `assistants/duo.toml` (now a `router` workflow with tools and bounded specialist consultations).
- `nexus-agent::duo::classify` picks `Tier::Light` or `Tier::Strong` per message, deterministically:
  strong when **Think harder** is on, attachments ≥ 1500 tokens, text ≥ 600 chars, a fenced code
  block, or a hard keyword (debug, refactor, algorithm, analyze, …). Otherwise light.
- `duo::pick_model` maps tier → model id from settings `duo.light_model_id` / `duo.strong_model_id`.
  Empty falls back to the other tier's model, then to the active model, so Duo always works.
- `service::runtime::duo_model_for` + `ensure_model` swap the single `llama-server` to the chosen
  model before the turn runs (status line "Duo: switching model…"). The message records the model id.
- UI: "Duo assistant" panel on the Models page with two selects.

## Limits of v0.1
- Only **one** model is loaded at a time, so the first answer after a switch pays the load time.
  Same-turn parallel models arrive in T18.04.
- The heuristic is intentionally simple; no extra model call.

## Exit check (T18.02)
Pick a 1–4B light model and a 7–12B strong model; confirm chit-chat uses light, code/long questions
use strong, and the answer metadata shows the right model.

## Isolated specialists (T18.06, owner request 2026-10-22)

Port the isolated-task/result pattern from
`REFE/pi-main/packages/coding-agent/examples/extensions/subagent/` into the shared inference lane.
Add Planner and Reviewer as standalone text assistants and an `ask_assistant(assistant, task)`
tool for Agent and Duo. The parent explicitly supplies task text (at most 6 000 characters).
Children get their own prompt and that task only: no conversation history, file handles, web
access, write tools, or recursive delegation. Only enabled Planner/Reviewer definitions are
eligible. The active scheduler executes consultations sequentially; this does not implement
T18.04's concurrent-model runtime. Bound child generation to 768 tokens and parent-visible results
to the existing tool budget; propagate parent cancellation. Existing hop and repeat guards apply.

Verify dispatch through the real tool registry, invalid/recursive targets, task/output limits,
cancellation, isolated request construction, enabled-state checks, and prompt lint for both new
assistants. Keep all file changes in the parent proposal/journal flow. No new dependencies.
