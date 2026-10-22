# Remaining repair work — 2026-10-22

This supplements `tasks.md`; it does not replace milestone contracts. Current repair phase
ends at T06.18. Read `handoff.md` before continuing.

- [ ] **P0 — Safely unlink referenced models.** Inspect `service/models.rs`: imports made with
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
