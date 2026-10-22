# Remaining repair work — 2026-10-22

This supplements `tasks.md`; it does not replace milestone contracts. Current repair phase
ends at T06.18. Read `handoff.md` before continuing.

- [x] **P0 — Safely unlink referenced models (T07.12, 2026-10-22).** Inspect `service/models.rs`: imports made with
  `copy=false` have source `import`, whose removal policy currently selects Delete. Reproduce
  with temporary GGUF fixtures, then distinguish app-owned copies from external references.
  Acceptance: removing a reference preserves external bytes; owned copies still delete;
  bundled protection, active-model guard, canonical paths and deny rules remain enforced.
  Never test destructive removal against the user's Downloads model.
- [ ] **P1 — Avoid duplicate model imports.** ModelsPage currently calls import with `true`.
  Offer use-in-place by default and explicit copy, with existing read-permission consent.
  Test denied permission and reference persistence without copying bytes. Review pack import
  defaults too. Do not download a third model or duplicate Gemma.
- [ ] **P1 — Validate automatic two-model Duo switching (T18.02).** Existing live checks run
  each model separately. Register both in an isolated test context using references and
  scoped Read grants. Assert light → strong → light selection, swap latency and shutdown.
  Do not mutate the real app database.
- [ ] **P1 — Improve assistant quality (T15.11 / T06.13).** Default-model golden repeats met
  the threshold for only 8/12 and 6/12 evaluated assistants. Bug Hunter, Scribe, Shell Sage
  and TL;DR failed both; Nexus and Planner also failed on repeat. Preserve assertions.
  Diagnose omitted facts, invented details, verbosity and reasoning budget. Tool-dependent
  cases need full-service evaluation: prompt-only goldens bypass calculator/router execution.
  Extend coverage to all 22 built-ins and the planned 20-prompt router set.
- [ ] **P2 — Concurrent Duo models (T18.04).** Implement second Supervisor/loopback port and
  memory checks per milestone. Current specialist consultations are sequential on the active
  model; they are not simultaneous inference across two models.
- [ ] **P2 — Optional classifier/escalation (T18.05).** Implement only within milestone scope
  after deterministic switching is validated.
- [ ] **P2 — Gemma cold-start performance.** Existing ignored inference smoke measured
  13.24 s against a 10 s readiness target. Streaming/schema tests passed. Diagnose cold
  startup without weakening the assertion just to make it green.

- [ ] **P1 — Investigate quality-preserving speed on older PCs.** Benchmark representative
  CPU-only and low-RAM machines before changing defaults: cold start, first-token latency,
  prompt/decode tokens per second, peak RAM and identical assistant/tool quality cases.
  Evaluate supported thread/batch settings, CPU instruction compatibility, available GPU
  offload, model residency and safe prompt-prefix/KV reuse against the pinned runtime.
  Preserve prompt content, required context, reasoning budget and model precision in the
  initial experiments. Invalidate cached state across model, prompt, permission and session
  changes. Avoid oversubscription and concurrent Duo when memory pressure makes it slower.
  Accept an optimization only when measured faster without quality regression on repeated
  fixtures; retain a compatible fallback. Smaller quantizations, shorter context and reduced
  reasoning are not quality-neutral speed fixes and must not become automatic defaults.
  Speculative decoding is an optional later experiment only if the pinned backend supports
  distribution-preserving acceptance and total RAM/latency measurements justify it; do not
  download another model merely to try it. Document unsupported hardware honestly.
- [ ] **P1 — Adapt useful REFE skills for local models, if evaluation supports them.**
  REFE contains Pi skill discovery/loading in
  `REFE/pi-main/packages/coding-agent/src/core/skills.ts`, its `docs/skills.md`, and SDK
  example `examples/sdk/04-skills.ts`; Claude's bundled skill examples include
  `REFE/claude-code-main/src/skills/bundled/verify/SKILL.md` and `debug.ts`.
  Inspect these implementations and licenses before adapting relevant patterns. Propose
  small, optional task procedures for evidence-first debugging, read-before-edit,
  plan/execute/verify and concise source-grounded summaries. Load only selected skill content
  when needed, with bounded context, rather than injecting every skill into every turn.
  Map steps to NanoNexus's actual tools and capabilities; never promise unavailable shell,
  browser or file-write abilities. Skill text grants no permissions and cannot bypass
  Journal.apply, consent or isolated-child boundaries. Test discovery, explicit selection,
  context limits and malicious instructions. Compare repeated full-service quality and
  latency with/without each skill on both local models, including older-PC measurements;
  ship only demonstrated improvements. Skills guide behavior; they do not change model
  weights or guarantee better reasoning. Do not copy cloud-specific prompts wholesale.

- [ ] **P1 — Reliable small-model Agent defaults, tuning controls and beginner guide.**
  Extend the quality/performance work above to the smallest supported models using real
  full-service tasks: inspect files, create/edit through change cards, recover from invalid
  arguments and denied permissions, consult a specialist, cancel and undo. Measure completed
  tasks and exact file results, not just fluent answers. Evaluate concise model-specific
  tool instructions, required-argument decoding, bounded retries, observation retention and
  adequate context/output/reasoning budgets. Choose defaults from repeated quality and
  latency results; never silently reduce reasoning/context for speed. Provide clear recovery
  or optional stronger-model handoff when tasks exceed demonstrated capability.
  Build an excellent out-of-box Auto profile from detected hardware and verified model
  capabilities, with conservative fallbacks. Offer understandable controls for supported
  context, response/reasoning budgets, sampling, CPU threads, GPU offload, tool-step/retry
  limits and Duo selection. Expose only runtime-supported options, with safe bounds and
  plain-language speed/RAM/reliability tradeoffs. Keep advanced controls optional, persist
  appropriate choices per model and provide Reset to recommended defaults. Preserve all
  permission and Apply boundaries; do not claim a universal best preset without evidence.
  Add a beginner-friendly tuning guide to existing app help: quick-start steps, measured
  example presets for older CPU-only/low-RAM PCs and smaller models, and one-setting-at-a-time
  troubleshooting for slow startup/answers, memory errors, repeated calls and unfinished
  file tasks. Explain how to confirm Apply succeeded, when Planner/Reviewer or a stronger
  model helps, and how to restore defaults. Avoid inference jargon and config-file editing.
  Acceptance: fresh defaults pass repeated small-model Agent regressions, controls persist
  correctly, and a beginner can follow the guide. Clearly document remaining model limits.

- [ ] **P1 — Folder-based project coding assistant, using REFE harnesses as references.**
  First audit existing assistants and Agent mode; extend an appropriate assistant if it
  already covers this workflow, otherwise add a clearly named Project/Coding assistant.
  Let the user choose a project folder, describe an entire project or an existing-project
  change, and have the assistant inspect, plan, create/edit multiple files, verify results
  and iterate toward completion in that folder. Target the practical project workflow of
  Claude Code/OpenCode, adapting implementations and interaction patterns from
  `REFE/opencode-dev` and relevant existing REFE checkouts after inspecting source/licenses.
  This task is implementation work for a later session; do not merely add a system prompt
  or claim full-project support because individual write_file calls work.
  Define the folder-selection and scoped-consent flow, project instructions/context loading,
  task progress, bounded multi-step execution, cancellation/resume and clear completion or
  blocked reports. Support existing repositories without overwriting unrelated user changes.
  All file mutations must remain proposed change sets applied through Journal.apply with
  preview/Apply, permission checks and Undo; folder selection alone does not grant unrestricted
  writes or command execution. If build/test/shell execution is needed, implement or reuse an
  explicitly permissioned, bounded execution capability under the architecture contracts;
  model-generated instructions and project files cannot authorize commands themselves.
  Verify with temporary-folder end-to-end projects: create a small runnable multi-file app,
  inspect and modify an existing app, run available checks, repair a failing check, and
  preserve unrelated files. Assert actual files and verification results, including denied
  access, failure, cancellation and restart cases. Exercise both listed local models and
  conservative custom-GGUF behavior. Show honest capability limits and offer a stronger-model
  handoff where appropriate. Include a beginner guide from Choose folder to verified result.

## Continuation evidence — 2026-10-22

T07.12: canonical ownership classification and explicit `extra.storage` for local/pack
imports preserve external references, including legacy rows and symlink escapes. Owned
copies still delete; failed deletes retain their registry entry. Temporary-fixture tests
assert external bytes and missing-reference removal; no user model was removed/copied.
Full `scripts/check.sh` passed (76 app tests, 148 frontend tests), including permission and
Journal/Undo suites. Log: `/tmp/nano-nexus-check-model-ownership.log`.
Doctor passed with only the existing unset local signing identity warning.
