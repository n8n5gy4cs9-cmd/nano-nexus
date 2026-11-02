# Portable coding reference audit

2026-11-02. Owner added ClaudeCode-Portable-main and code-stick-main during the active 12-task conversion batch. Source audit only; no runtimes/providers/agents executed.

## ClaudeCode-Portable features to carry forward

| Reference feature and evidence | NanoNexus integration task |
|---|---|
| Provider profiles/model switching: lib/providers.mjs, config.mjs; nine named providers and custom endpoints | UP.080: explicit cloud opt-in, local llama.cpp default, capability flags and memory-only credentials |
| Official Claude CLI/Agent SDK bridge: lib/agent.mjs, runtime.mjs | UP.081: optional private runtime, license/redistribution gate, explicit engine identity |
| Streaming adapter modes: adapter.mjs, external-adapter.mjs, responses-adapter.mjs | UP.082: protocol/capability adapter layer; no silent tool-format fallback |
| Real terminal + GUI session resume: sessions.mjs, native-sessions.mjs, transcript.mjs, resume scripts | UP.083: truthful per-engine session identity, explicit incompatible-profile handling |
| Selected project or projectless chats; attachment folders | UP.084: project context and attachment ownership through existing grants |
| Tool timeline, bounded turns, approval requests/expiry, cancel, response timeout and surfaced provider errors | UP.085: end-to-end NanoNexus coding lifecycle and observable failure states |
| Pinned per-target runtime, verified Node bootstrap, staged update/rollback, link-free exFAT layout | UP.053/054: prepare prebuilt runtimes before shipping, no target npm install |
| Diagnostics/redaction and model/provider compatibility warnings | UP.071/085: report actual engine/model/tool compatibility and error cause |

The reference default can permit SDK tools to mutate files directly (including bypassPermissions). This conflicts with NanoNexus ADR-005 and cannot be copied into the trusted tool path. An optional external coding engine must propose changes through an isolated scratch workspace and return diffs/change sets for Journal application, or use an intercepted tool boundary that demonstrably prevents direct writes. A terminal with unrestricted shell writes cannot be advertised as the NanoNexus permission-enforced agent. Do not enable bypass mode as an integration shortcut.

Reference credentials persist as plaintext settings.json. Do not inherit that default on removable media. The reference uses SDK resume IDs and profile fingerprints; retain that distinction rather than mixing NanoNexus message IDs with external runtime IDs. Provider names in this source are reference configuration, not evidence of current API compatibility.

## Useful code-stick patterns

| Pattern and source | Adaptation |
|---|---|
| Prebuilt per-target engine catalog and manifest | Reuse design under UP.050–053; retain NanoNexus pinned llama.cpp rather than adding a duplicate Ollama model store |
| Engine upgrade separated from model store, staged extraction/copy | UP.054: preserve shared models/data and rollback apps/runtime only |
| Local loopback inference + owned process tree cleanup | Keep existing authenticated Supervisor; never reuse an unrelated localhost process based only on port |
| Explicit app data/cache/config relocation and prestaging | UP.022: route all worker env before spawn and bundle lazy assets ahead of time |
| Capacity/model guidance, target trimming/add-targets | UP.073: honest memory/context estimates and requested-platform packaging |
| Diagnostics, local redacted bug reports, status | UP.070/071: bounded USB-local reports with clear unobserved writes |
| Non-destructive add/remove model and orphan pruning | UP.030–033: digest reference counting; no unguarded prune/uninstall on whole DIST |

Do not import fixed-port assumptions, an unverified-download override, global npx/npm setup, an additional Ollama store, or universal zero-residue claims. The reference's RAM and latency claims are examples, not measured guarantees for our engine/model/hardware.

## Agentic coding acceptance contract

A request to create/edit code must produce real, visible proposed changes, not an assistant narrative claiming writes. Trace request → selected project/grants → tool-capable model → validated action → tool result/error → proposal → user approval → Journal apply → recorded result. Read/search/list must feed bounded real tool output back into the model. Invalid tools/arguments produce visible actionable errors; cancellation terminates inference/worker work and expires approvals. Tool failures cannot become successful final summaries. Persist turn/session IDs and recovery state without replaying already-applied writes. New provider/model profiles must show tool capability limitations before coding starts.

Current no-testing instruction prevents runtime proof. Source improvements must be reported as unverified, never as evidence that agentic coding now works. No cloud requests, tool execution, model benchmark, build or smoke run is authorized by this audit.

## License obligations

Both wrapper repositories have root MIT LICENSE files. Preserve actual notices for any copied code. Claude Code/Agent SDK, opencode, Ollama, Node and every transitive dependency/model retain separate terms: wrapper MIT does not grant redistribution rights for their payloads. Pin and collect terms/notices before bundling optional runtimes. No dependency has been added by this audit.
