# Milestone Kickoff Prompts

Send one of these when starting a milestone (after the system prompt). They prime the agent with
the milestone's intent and its riskiest parts. Then let it run the task loop.

| Milestone | Kickoff prompt |
|---|---|
| **M00** | "Start M00 (docs/milestones/M00-bootstrap.md). Build the Cargo workspace with 8 library crates and implement nexus-core fully with tests. Don't add Tauri yet. Finish with scripts/check.sh green. Do T00.01 first." |
| **M01** 🍎 | "Start M01 on the Mac. The scripts setup-runtime.sh and fetch-model.mjs already exist: run them, fix only what breaks, then write smoke-runtime.sh and run spikes S1–S3, recording evidence in docs/SPIKES.md. If a spike changes a default, update ADR-008 and catalog/models.json." |
| **M02** | "Start M02. Implement nexus-llm exactly per docs/architecture/INFERENCE-CONTRACT.md. The riskiest parts are SSE parsing, process ownership/shutdown (no orphans), and the Windows Job Object. Build the mock server and the fake binary before the supervisor." |
| **M03** | "Start M03. Scaffold the Tauri 2.12 app with platform configs from day one. Make the shell beautiful per UI-DESIGN §2–3. Run spike S6 (tauri-specta) early, and S7 (the Windows cross-compiled skeleton) before the milestone ends. The frontend must call src/lib/api.ts only." |
| **M04** | "Start M04, the first daily-usable build. Introduce the service layer (Caller, Sink) and use it for every command. Streaming must be smooth: coalesce deltas in Rust, rAF flush in React, memoize finished markdown blocks. Keep the chat responsive at 390 px." |
| **M05** | "Start M05. This is safety-critical. Implement PERMISSIONS.md literally, with the full test list in §8 and a proptest for apply/undo. No shortcuts in canonicalization. Every write goes through Journal::apply." |
| **M06** | "Start M06. Load the 17 built-in assistants from assistants/*.toml (the [prompt] table holds the system prompt), validate them with schemas/assistant.schema.json, and add prompt_lint. Implement the router with the heuristic gate and the flat action schema. Measure router accuracy on the real model." |
| **M07** | "Start M07. Implement nexus-models per MODEL-MANAGER.md: GGUF parser with guards, HF client, resumable verified downloader (manual redirects, token stripping), RAM fit, bundled manifest (1..n models). Then the Models UI." |
| **M08** | "Start M08. Rust computes facts, and the LLM only narrates. Build the scanner and aggregates with benchmarks first, then extractors, then pipelines with internal prompts from prompts/internal/. Validate that narrative numbers exist in the facts. Secrets are always masked." |
| **M09** | "Start M09 with spike S4 on the owner's own projects (copies). Build the fixture projects first. Rename Forge is language-agnostic: symbols, files, and folders, with imports, config files, and string lookups updated. Plans use exact tree-sitter byte ranges, never regex." |
| **M10** | "Start M10. CodeMirror 6 with one EditorView per tab and tokens-based theming. Saving is a single-file change set. AI edits come back as a full rewritten selection that you diff locally, shown in the merge view." |
| **M11** | "Start M11. A JobRegistry for all long operations, notification preferences and routing, and folder watches with diff_reports and threshold notifications." |
| **M12** 🍎 | "Start M12. Implement stage-bundle, build-app.sh, verify-payload, the App Builder (zero-dependency Node server + a single HTML page), and model packs. The guide in guide.html is the acceptance spec for the user-facing flows. Update its status badges when they work." |
| **M13** 🍎🪟 | "Start M13. Windows builds are produced ON THE MAC with cargo-xwin + NSIS. Implement fetch-runtime-windows (digest-verified), the three formats (installer, portable, kit), the NSIS hook, and Windows process safety (Job Object, CREATE_NO_WINDOW). Test in the Win11 ARM VM and on x64 hardware." |
| **M14** | "Start M14. LAN Share: nexus-share (axum) with pairing auth first (tests before features), then WebSocket RPC over the existing service layer, then the web build and mobile layout. Default port 9889. llama-server stays loopback-only." |
| **M15** | "Start M15. Polish: command palette, onboarding, a11y and perf budgets, assistant evals. Take screenshots of every screen in both themes and fix what looks off." |
