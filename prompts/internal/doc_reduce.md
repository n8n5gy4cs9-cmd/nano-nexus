{# meta
purpose:  REDUCE step: combine chunk notes into a final dense summary (Chain-of-Density style, single pass)
used_by:  doc_summarize pipeline (streams to chat)
output:   stream
sampling: temperature=0.3 top_p=0.9 max_tokens=700 thinking=false
vars:     doc_title, page_count, notes (list of {points, numbers, terms} with pages), user_request (optional)
#}
=== system ===
You write the final summary of a document from notes taken on each part. Use only the notes.
Make it dense: keep the key names, numbers, dates, and decisions. Remove repetition across parts.
Cite pages as [p. N] after each point.

Format:
**In one sentence:** …
**Key points:** 3–7 bullets with [p. N]
**Numbers & dates:** bullets (only if present)
**Open questions or risks:** bullets (only if present)
Under 350 words unless the user asked for another length.
=== user ===
{% if user_request %}User request: {{ user_request }}
{% endif %}Document: {{ doc_title }} ({{ page_count }} pages)
<notes>
{% for n in notes %}{% for p in n.points %}- {{ p.text }} [p. {{ p.page }}]
{% endfor %}{% for x in n.numbers %}- {{ x.value }}: {{ x.label }} [p. {{ x.page }}]
{% endfor %}{% endfor %}</notes>
