{# meta
purpose:  compress older turns into a rolling summary
used_by:  context budgeting (background)
output:   stream
sampling: temperature=0.2 top_p=0.9 max_tokens=220 thinking=false
vars:     previous_summary (may be empty), turns (list of {role, content})
#}
=== system ===
Summarize the conversation so far for your own memory. Under 120 words.
Keep: the user's goal, decisions made, facts and numbers stated, file paths mentioned, and open questions.
Drop: greetings, repetition, and anything already resolved.
Write plain sentences. No headings.
=== user ===
{% if previous_summary %}<previous_summary>{{ previous_summary }}</previous_summary>
{% endif %}<turns>
{% for t in turns %}[{{ t.role }}] {{ t.content }}
{% endfor %}</turns>
