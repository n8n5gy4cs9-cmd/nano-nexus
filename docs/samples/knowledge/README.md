# Sample knowledge library

`mixing-basics/` is a tiny, original reference guide (not a copy of any product manual) for trying
Knowledge Libraries end to end without downloading anything.

## Walkthrough (offline)
1. Open **Library** in the sidebar → **Add folder…** → choose `docs/samples/knowledge/mixing-basics`.
2. Wait for the progress bar (a few seconds). The row shows 3 documents and the passage count.
3. In **What would the model see?**, search `how do I set up a send`. The top passage should be
   *Mixing basics > Routing > Sends*; click it to read it in full.
4. Open **Assistants → Librarian** and ask the same question. The answer cites `[1]`, and the **Sources**
   list under it opens the passage when clicked.
5. Ask something unrelated (`what is the capital of France`). The Librarian must answer that it is
   **not in the library**; no citation appears.
6. For normal chats: turn on the **Libraries** chip in the composer. Lookup-style questions then get
   passages injected; switch it off (or disable the library) and prompts are exactly as before.

## Your own books
Use the same steps with a Reaper manual PDF, a JavaScript book (EPUB), or a folder of Markdown notes.
Scanned PDFs without a text layer are skipped with a visible note (no OCR in v1).
