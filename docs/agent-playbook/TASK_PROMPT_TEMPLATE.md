# Task Prompt Template

Use this as the **user message** for one task (after the system prompt from `SYSTEM_PROMPT.md`).
Fill in the `<…>` parts. The more of the spec you paste, the better a smaller model performs.

```text
TASK: <ID> <title from tasks.md>
MILESTONE: <Mxx name>  (spec: docs/milestones/<file>.md)

CONTEXT (pasted from the spec):
<paste the milestone "Scope" bullet(s) and "Design notes" relevant to this task>
<paste the relevant contract excerpt, e.g. INFERENCE-CONTRACT §4.2 or PERMISSIONS §3>

FILES YOU WILL LIKELY TOUCH:
- <path>
- <path>

ACCEPTANCE:
- <what must be true; copy from exit criteria or derive it>
- Tests: <which tests to add, e.g. "unit tests for SSE decoder: CRLF, split UTF-8, [DONE]">
- scripts/check.sh is green.

DO NOT:
- Touch files outside the list unless necessary (explain why if you do).
- Start the next task.

WHEN DONE: tick <ID> in tasks.md with today's date, run scripts/progress.sh --write,
commit "<ID>: <summary>" and push to main. Report the test output summary.
```

## Example (filled)

```text
TASK: T02.02 SSE decoder (CRLF, partial lines, UTF-8 splits, [DONE]) + exhaustive tests
MILESTONE: M02 Inference crate nexus-llm (spec: docs/milestones/M02-nexus-llm.md)

CONTEXT:
"SseDecoder: buffer bytes, split on \n (strip a trailing \r), collect data: lines until a blank line,
and emit the joined payload. [DONE] ends the stream. Decode UTF-8 only on complete lines."
INFERENCE-CONTRACT §4.2: chunks look like {"choices":[{"delta":{"content":"Hel"}}]} …

FILES: crates/nexus-llm/src/sse.rs, crates/nexus-llm/src/lib.rs, crates/nexus-llm/tests/sse.rs

ACCEPTANCE:
- SseDecoder::push(&mut self, bytes: &[u8]) -> Vec<SseEvent>; SseEvent::{Data(String), Done}
- Tests: CRLF line endings; an event split across 3 chunks; the 4-byte emoji split 1+3 bytes;
  multiple data: lines joined with \n; comments (": ping") ignored; [DONE] → Done; garbage after Done ignored.
- scripts/check.sh green.
```
