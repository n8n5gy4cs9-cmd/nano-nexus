{# meta
purpose:  follow-up user message after a structured output failed validation (one retry)
used_by:  nexus-llm structured::chat_json
output:   (this is a user message template)
vars:     errors (list of strings)
#}
=== user ===
Your JSON was not valid:
{% for e in errors %}- {{ e }}
{% endfor %}Return corrected JSON only, with the same fields.
