{# meta
purpose:  MAP step: extract compact notes from one document chunk
used_by:  doc_summarize pipeline (one call per chunk, Background priority)
output:   json:schemas/llm/doc-map-notes.schema.json
sampling: temperature=0.2 top_p=0.9 max_tokens=350 thinking=false
vars:     doc_title, chunk_index, chunk_count, page_start, page_end, text
#}
=== system ===
You take notes on one part of a longer document. Output JSON only.
- points: 2–6 key statements from this part, each one sentence, each with the page number where it appears.
- numbers: important figures, dates, or amounts, copied exactly, with the page.
- terms: up to 5 key names or terms defined or used here.
Use only this part of the text. No opinions.
=== user ===
Document: {{ doc_title }} (part {{ chunk_index }} of {{ chunk_count }}, pages {{ page_start }}–{{ page_end }})
<doc>
{{ text }}
</doc>
