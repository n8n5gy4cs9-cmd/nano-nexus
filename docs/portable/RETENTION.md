# Retained NanoNexus features and incompatible reference behaviour (UP.048)

Source audit: 2026-11-02. Source reading only; nothing built, run or tested. This extends
`NANONEXUS-INVENTORY.md` (what exists) with the rule each portable-conversion task must follow and a list of
reference behaviours NanoNexus deliberately does not adopt.

## Retention rules per feature

| Feature | Must stay true while adding studio engines | Checked by |
|---|---|---|
| Assistants and Duo | Tools stay declared per assistant; Duo uses one shared model registry; no second engine runs unless a task says so; consultations stay bounded and non-recursive | ASSISTANTS contract, `prompt_lint` |
| File proposals and journal | Engines (image, speech, TTS) never write user files directly. Their outputs go to the USB `outputs/` area through the engine service. Deleting or exporting a saved output into a user folder goes through `Journal::apply` (UP.042) | ADR-005, ADR-016 |
| Analyzer and reports | Reports stay in USB data; no change to extraction or the document-QA pipeline | ANALYZER |
| Knowledge libraries | Copied library paths are rebound relative to DIST (UP.023b); retrieval and citations unchanged | M17 |
| Authenticated LAN Share | New studio commands are desktop-only and stay off the LAN allow-list until each is reviewed; pairing auth is never relaxed; never autostarts in portable mode | LAN-SHARE, ADR-018 |
| Activity | External log readers stay opt-in and re-confirmed each launch; engine jobs may emit activity events but must not read other tools' logs | OBSERVE |
| Offline help | Docs and viewer stay bundled; iframe sandbox and CSP unchanged | M17 |
| Permission engine | Canonicalise first, deny list wins, whole-DIST protection stays | PERMISSIONS §8 |

## Reference behaviour NanoNexus does not adopt

| Reference behaviour | Why it is incompatible | What replaces it |
|---|---|---|
| Browser `localStorage` for conversations, settings, theme | Non-persistent WKWebView on Mac in portable mode; second history store | SQLite and USB settings (UP.022b) |
| Linking models by external absolute path | Breaks self-contained USB and mount changes | Import into the shared store |
| Localhost HTTP dispatcher with no auth | NanoNexus only exposes an authenticated, paired LAN API | Tauri commands over the Transport facade |
| First-launch downloads of Node, npm, pip, engines | No target-side installs or implicit downloads | Prebuilt private workers (UP.050 onward) |
| Reset/cleanup/update scripts that delete directories | Could remove models and data | Staged updates (UP.054), reclaim with preview (UP.035) |
| Direct writes of generated files and deletes from the UI | Bypasses journal and permissions for user-visible changes | Engine outputs in USB `outputs/`; user-folder export and deletion via Journal |
| Web search toggled per chat with a disk cache | Extra write destination and consent model | ADR-016 opt-in, no cache (see WEB-SEARCH.md) |
| Python pickle `ckpt` loading | Executes untrusted code | Rejected; `safetensors`/GGUF only |
| Backend downloads from unpinned URLs | Supply-chain risk | Pinned digests (UP.053) |

## Not verified

No feature above was exercised. This document does not claim any of them works in a portable DIST.
