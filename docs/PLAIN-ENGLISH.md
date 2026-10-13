# NanoNexus in plain English

Short, practical answers to the questions that come up most. For the full specs see `docs/PLAN.md` and
`docs/architecture/`. For asking your books questions see `docs/GUIDE-ASK-YOUR-BOOKS.md`.

## 1. The disk is filling up: what can I delete?
Run `scripts/clean-trash.sh`. It deletes only things that can be rebuilt. It never touches
`dist-bundles/` (your finished apps), your original `.gguf` files, or source code.

| Command | What goes | What stays |
|---|---|---|
| `scripts/clean-trash.sh` | Staged bundle payloads (`vendor/bundle`), generated Tauri overlays, the app copy in `target/release/bundle` | Finished apps in `dist-bundles/`, all models |
| `... --rust` | Debug builds (`target/debug`), `incremental` caches, plus the line above | `target/release` (release builds stay fast), all models |
| `... --models` | Models in `vendor/models` except the default one `run.sh` downloaded | The default model |
| `... --all` | `--models` plus the whole `target/` folder (next build is slow) | The default model |
| `... --default-model` | Also the default model (`run.sh` downloads it again) | |

Add `--dry-run` to preview sizes, `--yes` to skip the question.
Your original `.gguf` (e.g. in Downloads) is never touched. Copies the bundler made in `vendor/models`
are removed by `--models` and `--all`. Models installed inside the app live in its data folder, which the
script does not touch. To keep every model, use only the plain run or `--rust`.

## 2. Which model should I pick?
- **Discover tags:** `librarian` marks models that cope well with "search the books, then answer". Bigger
  is more accurate. `optimal` marks the pick for its size class.
- **Rough guide:** Ternary Bonsai 2 27B (strong machines), Bonsai 8B, Qwen3.5 4B (best balance),
  Gemma 4 E4B (good answers, slow on older laptops), MiniCPM5 2B Q4 (older laptops, check citations).
- **Models under 2B** often miss answers. Use them to find where to read, then read the page yourself.
- **Unverified models** (the Ternary Bonsai family) show a "Community: unverified" badge. They may need
  PrismML's llama.cpp build; see Q-8 in `docs/QUESTIONS.md`.
- **Speed:** nobody has measured it for you. Run `scripts/bench-model.sh <model.gguf>` on each model
  (time to first token, tokens per second, memory). It is written for macOS.

## 3. Asking your books (Libraries)
1. Add PDFs or docs on the **Library** page (one library per book or doc set). Scanned PDFs are skipped
   (no OCR).
2. Search is keyword-based and offline. The model only sees the few passages the search returns, so
   most wrong answers are search misses. Ask using the book's own class and function names.
3. **Librarian assistant:** always strict. Answers only from the passages, cites `[n]` with book and page,
   quotes code exactly, says "not in the library" instead of guessing.
4. **Coding help instead of quotes:** use **Code Mentor** with the **Libraries** chip on, and set
   Settings → Privacy → **Library answer style** to **Guide**. The books are the main reference, but the
   model may explain and write adapted code, marking what is from the books `[n]` and what is "my own
   suggestion". **Strict** brings back answers only from the books.
5. Any `[n]`-less claim is untrusted. Click the citation and read the passage.

## 4. The App Builder status line
Orange = building, green = finished, red = something was not built. The log below it says what is done and
what is missing. A red result can still contain finished targets (for example the macOS app can be fine
while Windows failed).

## 5. Windows build says clang-cl / llvm-rc are missing, but they are installed
Homebrew installs LLVM without putting it on your `PATH`. The checker (`scripts/doctor.sh --windows`) only
sees tools on the `PATH` of the terminal that started the App Builder. In that terminal run
`export PATH="$(brew --prefix llvm)/bin:$PATH"`, then start the build again. The other warnings
(`NEXUS_SIGN_IDENTITY`, `XWIN_CACHE_DIR`) are optional.

## 6. Where things live
- `vendor/models/` models fetched or imported for bundling and `run.sh` (git-ignored)
- `vendor/bundle/`, `apps/desktop/src-tauri/gen/` staged build inputs (safe to delete)
- `dist-bundles/` your finished macOS apps (never auto-deleted)
- `target/` Rust build output (the big one; see section 1)
- `catalog/models.json` the Discover list; `assistants/*.toml` the assistants
