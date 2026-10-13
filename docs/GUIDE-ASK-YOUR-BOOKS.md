# Ask your books without hallucinations

For: "I have 5 programming books or doc sets (JavaScript, HTML/CSS, PHP, my own game engine's manual), one of them has the answer, and I want the code from the book, not a guess."

## How it works (so you know where it can fail)
Search is keyword-based (BM25), offline, with no model involved. The Librarian first runs `kb_search`,
gets the top few passages (about 1,200 tokens), and may answer only from those. Anything the search
does not return, the model never sees. So **most wrong answers are search misses, not model lies.**

## Setup (once)
1. Library page → add each PDF. Prefer **one library per book** or per topic ("JavaScript", "PHP", "MyEngine-API", "MyEngine-Manual").
   Searches can then be scoped, and the citation names the book.
2. Check the indexing note on each document. **Scanned PDFs (images) are skipped; there is no OCR.** If
   a book shows 0 chunks, export a text version or use an OCR'd copy.
3. Use the Library page's search box to confirm the book's text is readable (code indentation can be
   mangled by PDFs).
4. Pick a **Librarian**-tagged model in Models → Discover. Bigger is more accurate:
   Qwen3.5 35B-A3B (32 GB machines), Qwen3.5 9B (best reader on a normal laptop), Qwen3.5 4B (best balance), Gemma 4 E4B (good
   answers, but slow on older laptops). On an older laptop use MiniCPM5 2B Q4 and check every citation.
   Models under 2B often miss; use them only to find *where* to read.
5. Start the **Librarian** assistant, not plain chat.

## Reading the source
Click a citation or a search result to open the passage. For HTML and Markdown documents, switch to
**Original document** to see the page rendered, and **Raw** to see its source text. PDFs show text only.
This guide and all other docs are in the app under **Help** (sidebar), offline.

## How to ask
- **Use the book's own words.** Class, method, and menu names beat descriptions:
  "addEventListener passive" or "Entity::SetPosition" finds more than "how do I react to scrolling" or "how do I move an object".
- **One question per message**, with the task and the thing you need:
  "Show the code for object pooling with `ObjectPool<T>`. Quote it exactly and give book + page."
- **Demand the quote:** "Quote the exact passage, then explain." A claim with no quote is suspect.
- **Allow no-answer:** end with "If it is not in the books, say so." (The Librarian does this already.)
- **Scope it** when you know the book: pick that library, or name it ("in the MyEngine manual").

## Finding which book has it
1. Ask "Which of my books covers X? List book and page for each." The sources list shows who matched.
2. Re-ask with the best book selected for the full answer.
3. If books disagree, ask "Do my books disagree on X?" The Librarian shows both with citations.

## Checking the answer (30 seconds)
- Every claim needs a `[n]`. No `[n]` means untrusted. Click it and read the passage.
- Code must appear in the passage character-for-character. If it only looks similar, ask again with
  "use only code that appears in the passage".
- "That is not in the library" is a good result. Rephrase with the book's terms (or its index entries)
  before concluding the book lacks it.
- Version drift: APIs change between versions (PHP 7 vs 8, ES5 vs ES2024, your engine v1 vs v2). Ask "which version is this for?" and check the book's edition.

## When it still goes wrong
| Symptom | Likely cause | Fix |
|---|---|---|
| Plausible code with no citation | Search returned nothing, model improvised | Say "answer only from passages"; rephrase using book terms |
| "Not in the library" but you know it is there | Query words differ from the book's | Try synonyms, class names, chapter title |
| Wrong book answers | Same topic in several books | Scope to one library |
| Garbled code | PDF text layer | Check in the Library search box; use an EPUB or Markdown copy |
| Slow or truncated | Big passages | Raise context in model settings, or ask a narrower question |

Use a 4B+ model for important lookups. Small models are fine for finding *where* to read; then read the
cited page yourself.

## When you want coding help, not a quote
Use **Code Mentor** (or any chat assistant) with the **Libraries** chip on, and set
Settings → Privacy → **Library answer style** to **Guide**. The books become the main reference, the
model may explain and write adapted code, and it must mark what is from the books `[n]` and what is
"my own suggestion". Switch back to **Strict**, or use the **Librarian**, when you need the book's
exact words. The Librarian is always strict.
