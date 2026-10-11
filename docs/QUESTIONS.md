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
