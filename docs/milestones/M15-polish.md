# M15: Polish

**Goal:** turn a working app into a delightful one, with fast navigation, a smooth first run, an
accessibility pass, perf budgets met, and assistant quality measured.

**Prereqs:** M04–M14. **Exit demo:** a fresh install (macOS and Windows) → onboarding → first chat
in < 60 s. Every screen is reachable by keyboard. The eval table is green.

## Scope
1. **Command palette** (⌘K/Ctrl+K, `cmdk`): navigate, new chat with assistant X, search
   conversations (FTS), actions (scan folder, analyze file, open Rename Forge, restart runtime,
   toggle theme), and model activation.
2. **Onboarding** (UI-DESIGN §4.11) with the animated hexagon. Re-runnable from Settings.
3. **Quick Ask window** (optional, global shortcut ⌥Space / Alt+Space via `tauri-plugin-global-shortcut`):
   a small floating composer that answers with Nexus and has an "Open in main window" link.
4. **Shortcuts:** all of UI-DESIGN §5, plus a Settings → Shortcuts viewer.
5. **Accessibility pass** (UI-DESIGN §7): contrast audit with a script over the token pairs, focus
   order, screen-reader labels, and reduced motion.
6. **Performance budgets:** cold start < 1.5 s, idle RAM < 250 MB (without the model), chat at
   60 fps, scan 100k files < 10 s. Profile and fix.
7. **Assistant evals:** `scripts/eval-assistants.sh` runs the golden set (ASSISTANTS §8.2) against
   the active model and prints a pass table. Add results for each bundled model to `docs/SPIKES.md`.
8. **Precompiled Metal shaders** (optional): build `default.metallib` as the Godot project did
   (needs `xcodebuild -downloadComponent MetalToolchain` on Xcode 26+), and compare first-load
   time vs. the embedded library. Keep it only if it saves > 500 ms.
9. **Empty, error, and loading states** everywhere (skeletons, friendly copy, retry buttons).
10. **Visual QA:** screenshot every screen in light/dark on macOS and Windows and fix spacing and alignment issues.

## Exit criteria
- [ ] Budgets met (numbers recorded in `docs/SPIKES.md` §perf).
- [ ] Keyboard-only walkthrough of all screens passes.
- [ ] Eval pass rate ≥ 80% on the default model for every assistant (or documented exceptions with a recommended bigger model).
