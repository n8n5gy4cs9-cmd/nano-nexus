# Open Questions and Blockers

Agents: when you're blocked, or a task conflicts with an ADR or spec, **don't guess on anything
safety-relevant**. Add an entry here, mark the task in `tasks.md` with `⛔ blocked: see QUESTIONS Q-n`,
and move on to the next unblocked task.

Format:
```
## Q-<n>: <short title> (YYYY-MM-DD, task T..)
Context: …
Options: A) … B) …
Recommendation: …
Answer (owner): …
```

## Q-1: Defaults chosen during planning (answer only if you disagree)
See the open questions table in [`PRD.md` §9](PRD.md#9-open-questions-defaults-chosen-change-any-time):
Tauri + React UI, V2 Q8_0 default model, Windows Vulkan runtime, LAN Share remote policy
"chat + read", HTTP + pairing transport.
Answer (owner): _pending_

## Q-2: Windows Mica needs a transparent window, but the spec says `transparent: false` (2026-10-11, task T03.11)
Context: UI-DESIGN §3.1 wants a Mica backdrop on Windows 11 with a solid `--bg-sidebar` fallback on
Windows 10, while BUNDLING §6 sets `"transparent": false` in `tauri.windows.conf.json`. Mica is only
visible through a transparent webview, and a transparent window on Windows 10 (no Mica) would render
as a plain black/white fill unless we detect the OS build and switch the CSS.
Options: A) Keep `transparent: false` now (native decorations, solid colors, no Mica); decide after the
S7 Windows run. B) Set `transparent: true` + `windowEffects: ["mica"]` and add an OS-build check
(Win11 = build >= 22000) that sets `data-mica` on `<html>` so the CSS paints solid colors otherwise.
Recommendation: A for now, then B once S7 (T03.16) shows how the WebView2 transparency behaves on a real Windows 11 and 10.
Answer (owner): _pending_

## Q-3: Saving an exported conversation to a user-chosen file (2026-10-11, task T04.09)
Context: M04 step 6 says "Export conversation to Markdown (save dialog)". AGENTS §3 forbids writing files
outside `nexus-fs::Journal::apply` (plus app data/caches), and the webview has no `fs` plugin.
Options: A) `conversation_export_md` returns the text and the UI copies it to the clipboard / offers it
in a dialog now; the file save goes through a Journal "create file" op once M05 exists.
B) Allow a narrow exception: a command that writes one file to a path the user just chose in a native save dialog.
Recommendation: A now, then B-via-Journal in M05 (the save dialog path becomes a granted single-file create).
Answer (owner): _pending_

## Q-4: Hugging Face token storage (2026-10-11, task T07.08)
Context: MODEL-MANAGER §3 stores the HF token in the OS keychain via `keyring`. That crate pulls in
platform secret-service/keychain bindings that this build has not vetted (and it is untestable in the Linux container).
Options: A) Read the token from the `HF_TOKEN` environment variable only (never persisted by the app) and show a clear
"set HF_TOKEN" message for gated repos. B) Add `keyring` and a Settings field.
Recommendation: A now (gated repos are rare for GGUF); B as a follow-up once the Mac build can test it.
Answer (owner): _pending_

## Q-5: Split (multi-file) GGUF downloads (2026-10-11, task T07.08)
Context: MODEL-MANAGER §3 says split models download all parts. The downloader handles one file per job and none of the
catalog models are split.
Options: A) Show split models in search but refuse to download them with a clear message (current). B) Queue each shard
as its own download and register the model when the last one verifies.
Recommendation: A until a split model is actually needed.
Answer (owner): _pending_
