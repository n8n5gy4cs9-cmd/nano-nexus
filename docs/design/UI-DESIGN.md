# NanoNexus UI and Visual Design

> Personality: **calm, precise, quietly luminous.** A dark-first workbench with a native macOS
> feel and one signature element: the **Nexus gradient** (violet → cyan), used sparingly for
> "AI is working" moments, the active model, and primary actions.

## 1. Principles
1. **Chat is the hero.** Everything else is one click away but never competes with the conversation column.
2. **Native first on macOS.** Overlay title bar with traffic lights, a vibrancy sidebar, SF-like
   rhythm, and system accent awareness.
3. **Show the work.** Tool chips ("📂 Read 3 files"), facts cards, and diffs make the AI's actions
   visible and trustworthy.
4. **Calm motion.** 150–220 ms ease-out, no bouncing. Streaming uses a soft caret and never shifts layout.
5. **Keyboard-first.** Every action is in the ⌘K palette, and every list is navigable with arrows.

## 2. Design tokens (CSS variables, `apps/desktop/src/styles/tokens.css`)

Color in OKLCH (Tailwind v4 native). Dark is the default. Light mirrors it.

```css
:root[data-theme="dark"] {
  --bg:            oklch(0.155 0.012 275);   /* window background */
  --bg-sidebar:    oklch(0.175 0.014 275 / 0.72); /* over vibrancy */
  --surface:       oklch(0.205 0.014 275);   /* cards, composer */
  --surface-2:     oklch(0.240 0.016 275);   /* hover, inputs */
  --surface-3:     oklch(0.285 0.018 275);   /* pressed, selected */
  --border:        oklch(0.330 0.018 275 / 0.55);
  --border-strong: oklch(0.420 0.020 275 / 0.70);
  --text:          oklch(0.955 0.006 275);
  --text-muted:    oklch(0.735 0.014 275);
  --text-faint:    oklch(0.560 0.014 275);
  --accent:        oklch(0.700 0.180 288);   /* violet */
  --accent-2:      oklch(0.820 0.120 205);   /* cyan */
  --accent-fg:     oklch(0.985 0.005 288);
  --success:       oklch(0.760 0.150 155);
  --warning:       oklch(0.830 0.140 80);
  --danger:        oklch(0.680 0.200 25);
  --info:          var(--accent-2);
  --ring:          oklch(0.700 0.180 288 / 0.55);
  --nexus-gradient: linear-gradient(135deg, var(--accent) 0%, var(--accent-2) 100%);
  --shadow-1: 0 1px 2px oklch(0 0 0 / 0.35);
  --shadow-2: 0 8px 24px oklch(0 0 0 / 0.35), 0 1px 0 oklch(1 0 0 / 0.04) inset;
  --code-bg:  oklch(0.180 0.012 275);
}
:root[data-theme="light"] {
  --bg: oklch(0.985 0.003 275); --bg-sidebar: oklch(0.965 0.006 275 / 0.75);
  --surface: oklch(1 0 0); --surface-2: oklch(0.970 0.005 275); --surface-3: oklch(0.940 0.008 275);
  --border: oklch(0.880 0.010 275); --border-strong: oklch(0.800 0.014 275);
  --text: oklch(0.220 0.020 275); --text-muted: oklch(0.450 0.020 275); --text-faint: oklch(0.600 0.016 275);
  --accent: oklch(0.560 0.210 288); --accent-2: oklch(0.620 0.130 215); --accent-fg: oklch(0.990 0 0);
  --success: oklch(0.600 0.150 155); --warning: oklch(0.700 0.150 70); --danger: oklch(0.580 0.210 25);
  --ring: oklch(0.560 0.210 288 / 0.45); --code-bg: oklch(0.975 0.004 275);
  --shadow-1: 0 1px 2px oklch(0 0 0 / 0.06); --shadow-2: 0 8px 24px oklch(0 0 0 / 0.08);
}
```

**Accent choices** (Settings → Appearance): Violet (default), Cyan, Emerald, Amber, Rose, Graphite.
Each swaps `--accent`/`--accent-2` and the gradient.

**Assistant hues** (avatar tiles = 2-stop gradient at 135°):
| Token | Stops (dark) | Used by |
|---|---|---|
| `violet` | oklch(.70 .18 288) → oklch(.62 .20 310) | Nexus, Prompt Architect |
| `teal` | oklch(.78 .12 185) → oklch(.68 .13 210) | Folder Scout, Tidy |
| `amber` | oklch(.84 .15 80) → oklch(.74 .17 55) | Rename Forge, Regex Smith |
| `blue` | oklch(.72 .14 250) → oklch(.64 .17 265) | DocLens, TL;DR |
| `green` | oklch(.78 .15 150) → oklch(.68 .15 170) | Code Mentor, Doc Writer, Commit Crafter |
| `red` | oklch(.70 .19 25) → oklch(.62 .20 10) | Bug Hunter, Secret Sentinel |
| `indigo` | oklch(.66 .16 275) → oklch(.58 .18 290) | Godot Sidekick, Log Detective |
| `pink` | oklch(.76 .14 350) → oklch(.68 .17 330) | Scribe, Polyglot |
| `slate` | oklch(.70 .03 260) → oklch(.58 .04 260) | Shell Sage, Data Wrangler |

### Typography
- UI: **Inter Variable** (`@fontsource-variable/inter`), `font-feature-settings: "cv11","ss01","tnum" (tables)`.
- Code: **JetBrains Mono Variable** (`@fontsource-variable/jetbrains-mono`), ligatures off by default.
- Scale (px / line-height): 11/16 caption · 12/18 small · **14/22 body (UI)** · **15/26 chat body** ·
  16/24 lead · 20/28 h3 · 24/32 h2 · 30/38 h1 (onboarding only). Weights: 400 / 500 / 600.
- Chat measure: max 72ch (`max-width: 760px`) centered.

### Spacing, radius, elevation
- 4 px grid: 4, 8, 12, 16, 20, 24, 32, 40, 56.
- Radius: 6 (chips, inputs-sm) · 8 (buttons) · 12 (cards) · 16 (composer, modals) · 999 (pills).
- Elevation: level 0 flat (lists) · level 1 cards (`--shadow-1` + border) · level 2 popovers and modals (`--shadow-2`).

### Motion
- Durations: 120 (hover) · 180 (appear) · 240 (panel slide). Easing `cubic-bezier(.2,.8,.2,1)`.
- Message appear: opacity 0→1, translateY 6→0 px. Streaming caret: 2 px bar with a gradient that pulses at 1.2 s.
- "Thinking" shimmer: a gradient sweep across the "Thinking…" label. Collapsing Thoughts animates height.
- All motion is disabled under `prefers-reduced-motion: reduce`.

### Iconography
`lucide-react` (outline, 1.75 stroke, 16/18/20 px). File-type icons are lucide icons tinted by category color.

## 3. App shell

```
┌────────────────────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●   (overlay titlebar, drag region 44px)          ⌘K Search…          🔔3   ◐ Ready ▾ │
├───────────────┬────────────────────────────────────────────────────────────┬───────────────┤
│ ▣ NanoNexus   │                                                            │  Inspector    │
│               │                  (current view)                            │  (toggle ⌘I)  │
│ + New chat ⌘N │                                                            │               │
│               │                                                            │               │
│ ◉ Chat        │                                                            │               │
│ ◇ Assistants  │                                                            │               │
│ ⬇ Models      │                                                            │               │
│ ▤ Analyzer    │                                                            │               │
│ ✎ Rename Forge│                                                            │               │
│ </> Editor    │                                                            │               │
│ ⟲ Changes     │                                                            │               │
│ 🔐 Permissions│                                                            │               │
│ ──────────    │                                                            │               │
│ PINNED        │                                                            │               │
│  · Game rename│                                                            │               │
│ TODAY         │                                                            │               │
│  · PDF: tax…  │                                                            │               │
│  · Downloads… │                                                            │               │
│ YESTERDAY     │                                                            │               │
│  …            │                                                            │               │
│ ──────────    │                                                            │               │
│ ⚙ Settings    │                                                            │               │
└───────────────┴────────────────────────────────────────────────────────────┴───────────────┘
 sidebar 248px (resizable 200–320, collapsible ⌘\)            inspector 300px (hidden by default)
```

- Sidebar: vibrancy (`window-vibrancy` → `NSVisualEffectMaterial::Sidebar`) on macOS, with a solid
  `--bg-sidebar` fallback elsewhere. Nav items are 32 px tall, the active item gets `--surface-3`
  plus a 2 px gradient bar on the left.
- Title-bar right cluster: **runtime pill** (●green Ready · ●amber Loading with spinner ·
  ●red Error). Clicking it opens a popover with the model name, ctx, tok/s, Restart, and Logs.
- 🔔 notification bell with an unread badge opens the notification popover.

## 4. Screens

### 4.1 Chat (home)
```
┌──────────────────────────── chat column (max 760) ────────────────────────────┐
│ [🗂 Folder Scout ▾]  Downloads cleanup                       ⋯  (export, pin) │  ← header 52px, sticky, blur
│ ───────────────────────────────────────────────────────────────────────────── │
│                                                                               │
│                                         ┌───────────────────────────────────┐ │
│                                         │ Scan my Downloads and tell me     │ │  ← user bubble: surface-2,
│                                         │ what to clean up                  │ │    right-aligned, radius 16
│                                         └───────────────────────────────────┘ │
│  ⬡ Folder Scout                                                              │  ← assistant: no bubble,
│  ┌ 🔐 Allowed read access to ~/Downloads ┐ ┌ 📂 Scanned 4,812 files · 1.9s ┐ │    avatar + name row,
│  ▸ Thoughts (12s)                                                            │    tool chips (pills)
│  **Your Downloads folder holds 38.4 GB**, mostly old videos and installers…  │
│  ## Findings                                                                  │
│  - …                                                                          │
│  [Open full report ↗]  [Plan cleanup]                                        │  ← action buttons from pipeline
│  ⧉ Copy  ↻ Regenerate   · 54 tok/s · 0.4 s                                   │  ← hover toolbar, faint stats
│                                                                               │
│ ┌───────────────────────────────────────────────────────────────────────────┐ │
│ │ 📎 ~/Downloads (folder) ✕   📄 notes.md · 412 tok ✕                       │ │  ← attachment chips
│ │ Ask Folder Scout…                                                         │ │  ← composer: surface, radius 16,
│ │                                                                           │ │    grows to 40% height
│ │ [＋] [/] [@]      ✦ Think harder ◯      ctx ▮▮▮▯▯ 3.1k/8k     [ ➤ Send ] │ │  ← send = gradient button;
│ └───────────────────────────────────────────────────────────────────────────┘ │    becomes ■ Stop when streaming
└───────────────────────────────────────────────────────────────────────────────┘
```
Details:
- **Empty state:** a large assistant avatar, name, tagline, and 4 starter cards (2×2), plus
  "Switch assistant" and recent assistants.
- **Assistant switcher (⌘/)**: a searchable popover listing assistants with icon, name, and tagline.
- **Markdown:** headings are 600 weight with smaller sizes than docs (h1 → 20 px in chat). Tables
  get zebra rows on `--surface`. Code blocks use the `--code-bg` header bar with the language,
  Copy, and "Open in Editor". Shiki themes are `github-dark-default` and `github-light-default`.
- **Thoughts block:** collapsed by default. Muted, italic-free, left border with a gradient,
  monospace timer.
- **Tool chips:** pill, `--surface-2`, icon + summary. Clicking expands args and a result preview.
- **Errors:** an inline card with a red left border, friendly copy, and an action button
  (Retry / Grant access / Open Models).
- **Drag-drop overlay:** the full chat column dims and shows "Drop to attach" with a dashed gradient border.

### 4.2 Assistants gallery
```
 Assistants                                   [Search…]        [+ New assistant] [Import]
 All · General · Documents · Files · Code · Data · Writing · System · Custom
 ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────┐
 │ ⬡ (gradient) │ │ ⬡            │ │ ⬡            │ │ ⬡            │   cards 240×168, hover lifts 2px
 │ DocLens      │ │ Folder Scout │ │ Rename Forge │ │ Code Mentor  │   name 600 / tagline muted 2 lines
 │ PDFs & docs, │ │ Map any      │ │ Fix bad names│ │ Explain &    │
 │ cited Q&A    │ │ folder…      │ │ in old code  │ │ improve code │
 │ 🔒 read      │ │ 🔒 read      │ │ 🔒 read+write│ │ —            │   permission chips
 │ [Chat] [⋯]   │ │ [Chat] [⋯]   │ │ [Open] [⋯]   │ │ [Chat] [⋯]   │   ⋯ = duplicate, export, edit (user)
 └──────────────┘ └──────────────┘ └──────────────┘ └──────────────┘
```
**Builder** (full-screen sheet): left form (identity: name, icon picker, color · Behavior: prompt
editor (CodeMirror, Markdown mode) with a live token counter (green ≤ 350, amber ≤ 450, red > 450)
· Model params sliders · Tools checklist · Workflow · Starters). Right: **Test chat** panel using the
draft. Footer: Validate · Save · Export.

### 4.3 Models (Download view)
```
 Models                                             Runtime: ● Ready  MiniCPM5 1B V2 · ctx 8k · 61 tok/s  [Restart] [Logs]
 Installed (2) · Discover · Search Hugging Face
 ┌──────────────────────────────────────────────────────────────────────────────────────────┐
 │ ⬡ MiniCPM5 1B · Opus-Fable5 V2      Q8_0 · 1.1 GB · 1.08B      ✦ Active   Built-in   │
 │   chat · code · thinking · tools          ▮ Runs great (needs 1.6 of 11.2 GB)          │
 └──────────────────────────────────────────────────────────────────────────────────────────┘
 ┌──────────────────────────────────────────────────────────────────────────────────────────┐
 │ ⬡ MiniCPM5 2B                        Q4_K_M · 1.56 GB · 2.5B                [Use] [🗑]  │
 │   ◔ Downloading 63% · 18.2 MB/s · 32 s left                    [Pause] [Cancel]        │  ← progress ring + bar (gradient)
 └──────────────────────────────────────────────────────────────────────────────────────────┘
```
Discover groups: **Tiny (≤1.5B)** · **Small (2–4B)** · **Coding** · **Thinking**. Each card has
a "Why this model" line. Search shows HF results; expanding a row lists its GGUF files by quant
with size and RAM badges.

### 4.4 Analyzer and Reports
```
 Analyzer                                       [＋ Scan folder]  [Analyze file]
 Recent reports: Downloads (today) · nano-nexus (Mon) · Old Game (Sun)
 ┌ Downloads — 38.4 GB · 4,812 files · scanned 1.9 s ago ────────────── [Export ▾] [Plan cleanup] ┐
 │ ✦ AI summary (streams)                                                                          │
 │ ┌ Total ┐┌ Duplicates ┐┌ Older than 1y ┐┌ Junk ┐   ← KPI cards (big tabular number + delta)     │
 │ ┌────────────── Treemap (d3-hierarchy squarify, canvas) ──────────────┐ ┌ By category (bars) ┐ │
 │ │  Movies 18.3 GB          │ old-installers 8.8 │ …                    │ │ Video ███████ 55%  │ │
 │ └──────────────────────────────────────────────────────────────────────┘ └────────────────────┘ │
 │ Tabs: Largest files · Largest folders · Duplicates · Junk · Naming · Age · Warnings               │
 │ (virtualized table: name · size · modified · category · actions [Reveal] [Open] [Add to plan])  │
 └──────────────────────────────────────────────────────────────────────────────────────────────────┘
```
Treemap colors follow the category palette (12 hues, OKLCH, L = 0.72, C = 0.12). Hover shows the
path and size. Click zooms in. The breadcrumb zooms out.

### 4.5 Rename Forge
```
 Rename Forge · Old Game (~/dev/old_game)  GDScript · 87 files · 1,240 symbols · 312 issues   [Re-index]
 Steps:  ① Index ✓ ─ ② Review issues ─ ③ AI proposals ─ ④ Plan & preview ─ ⑤ Apply ─ ⑥ Verify
 Filters: [Kind ▾] [Issue ▾] [File ▾] [Confidence ≥ 0.8] [Flags ▾]        [✦ Propose names (312)]
 ┌──┬────┬──────────────────┬──────────────────────┬──────┬──────┬────────────────────────────┐
 │☑ │ ƒ  │ doStuff2         │ apply_damage    ✎    │ 7·2·1│ ▮▮▮▯ │ applies damage to target… │
 │☐ │ 𝑥  │ plyr             │ player               │ 23   │ ▮▮▮▮ │ expand abbreviation       │
 │☑ │ ⚡ │ open_door (sig.) │ door_opened          │ 3·2  │ ▮▮▮▯ │ signals use past tense    │
 │⚠ │ ƒ  │ hit              │ take_hit         🔗  │ 9 ⚠  │ ▮▮▯▯ │ ambiguous: 2 classes      │
 └──┴────┴──────────────────┴──────────────────────┴──────┴──────┴────────────────────────────┘
 refs column = code·scene·string. Clicking a row opens a right drawer with every occurrence (code preview).
 Footer: 214 accepted · 12 need manual · [Plan & preview →]
```
Plan & preview reuses the **Changes** diff viewer. A git-dirty banner appears if needed.

### 4.6 Editor
File tree (granted roots) | tabs + CodeMirror | AI side panel (actions: Explain · Improve names ·
Add docs · Find bugs · Ask…). AI edits appear in `@codemirror/merge` unified view with per-chunk
Accept/Reject. Status bar: language · line:col · encoding · "Read-only (no write grant)" if applicable.

### 4.7 Changes (history)
A list of change sets (status pill: planned / applied / undone / failed) | a detail pane with
file list and stats (+12 −7, 3 moves, 2 to Trash) and a diff viewer (split/unified toggle).
Buttons: Apply (gradient) / Discard / Undo. Undo conflicts show a 3-option dialog.

### 4.8 Permissions
- **Folder access:** a table of grants (path · level dropdown · scope · expires · last used ·
  revoke) plus [+ Add folder].
- **macOS privacy:** status cards for Desktop, Documents, Downloads, and Full Disk Access, each
  with a "Check" and "Open System Settings" button and a one-line explanation.
- **Protected locations:** the deny list (built-in entries locked, user entries editable).
- **Activity:** the audit log (virtualized, filter by assistant/action, export CSV).

### 4.9 Notifications popover
360 px wide. Items: level icon, title, body (2 lines), relative time, and an action link.
"Mark all read", "Clear", and a ⚙ shortcut to notification settings.

### 4.10 Settings
Sections: General (launch at login, quit on close, language) · Appearance (theme, accent, font
size, show perf stats) · Model runtime (ctx, GPU layers, flash attn, threads, idle unload, server
logs) · Privacy (network downloads, deny list, clear history) · Notifications (per kind) ·
Shortcuts · Advanced (data folder, journal retention, Godot path, reset app, open logs).

### 4.11 Onboarding (first run, 4 steps, skippable)
1. **Welcome**: logo animation (gradient ring forming a hexagon "nexus"), then "Private AI on your Mac".
2. **Model check**: built-in model found → ✓ "MiniCPM5 1B ready" (live runtime status), or "Download a model" CTA.
3. **Give it a folder** (optional): pick a folder → Read grant, with an explanation of levels.
4. **Pick your first assistant**: 6 featured cards → starts a chat.

### 4.12 Command palette (⌘K, `cmdk`)
Groups: Navigate · New chat with… (assistants) · Conversations (FTS search) · Actions (Scan
folder, Analyze file, Open Rename Forge, Restart runtime, Toggle theme) · Models (activate).

## 5. Keyboard shortcuts
| Keys | Action |
|---|---|
| ⌘N | New chat (current assistant) |
| ⌘K | Command palette |
| ⌘/ | Switch assistant |
| ⌘⇧T | Toggle "Think harder" |
| ⌘. or Esc (while streaming) | Stop generation |
| ⌘↑ (empty composer) | Edit last user message |
| ⌘⇧C | Copy last answer |
| ⌘1…⌘8 | Chat, Assistants, Models, Analyzer, Rename Forge, Editor, Changes, Permissions |
| ⌘\ / ⌘I | Toggle sidebar / inspector |
| ⌘, | Settings |
| ⌘O / ⌘S / ⌘W | Editor: open, save, close tab |
| ⌥Space (global, optional) | Quick Ask window |

## 6. Components (shadcn/ui based, in `src/components/ui`)
Button (variants: primary = gradient, secondary, ghost, danger; sizes sm/md) · IconButton · Input ·
Textarea (auto-grow) · Select · Switch · Slider · Tabs · Tooltip · Popover · Dialog · Sheet ·
DropdownMenu · Badge/Pill · Progress (bar + ring) · Skeleton · Toast (sonner) · Command (cmdk) ·
ScrollArea · Table (TanStack Table + Virtual) · Kbd · EmptyState · StatCard · AssistantAvatar
(gradient tile + lucide icon) · FileIcon · DiffView · Markdown (react-markdown + remark-gfm +
Shiki code component) · Treemap.

## 7. Accessibility checklist
- Text contrast ≥ 4.5:1 (body) and ≥ 3:1 (large text/icons) in both themes. Check muted text on surfaces.
- Visible focus ring: 2 px `--ring` + 2 px offset on every focusable element.
- The chat log uses `role="log"` with `aria-live="polite"` and a final message announcement
  (not every token).
- All icon buttons have `aria-label`. Tooltips aren't the only way to get information.
- Every interaction works with the keyboard alone (tested in M13).

## 8. Asset list
- App icon: rounded-square macOS icon, deep-space background `oklch(0.16 0.02 280)` with a
  luminous hexagonal "nexus" ring in the violet→cyan gradient and a small core dot. Export
  1024 px → `pnpm tauri icon` generates all sizes.
- Tray icon (template image, monochrome hexagon, 18 pt).
- Onboarding hero: the same hexagon ring animated with SVG stroke-dashoffset.
