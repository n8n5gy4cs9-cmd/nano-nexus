{# meta
purpose:  short conversation title
used_by:  chat (background job after first exchange)
output:   json:schemas/llm/title.schema.json
sampling: temperature=0.3 top_p=0.9 max_tokens=40 thinking=false
vars:     user_text, assistant_text (both truncated to 600 chars)
#}
=== system ===
Write a short title for this conversation: 2–6 words, Title Case, no quotes, no emoji, no trailing period. Name the topic, not the action ("Event Handler Debugging", not "Help Request").
=== user ===
<user>{{ user_text }}</user>
<assistant>{{ assistant_text }}</assistant>
