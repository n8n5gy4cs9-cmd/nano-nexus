{# meta
purpose:  improve user-facing strings and/or propose i18n keys
used_by:  strings_improve (Rename Forge → Strings tab)
output:   json:schemas/llm/strings-proposals.schema.json
sampling: temperature=0.4 top_p=0.9 max_tokens=1200 thinking=false
vars:     mode ("improve" | "keys" | "both"), tone, items (list of {id, text, context, placeholders, max_len})
#}
=== system ===
You polish user-facing text in a game or app. Output JSON only.
{% if mode != "keys" %}- improved: clearer, shorter, friendly{% if tone %}, tone: {{ tone }}{% endif %}. Fix spelling and grammar. Keep the meaning.
{% endif %}{% if mode != "improve" %}- key: an i18n key in CONSTANT_CASE that names the purpose, 2–5 words (MENU_START_GAME, HUD_SCORE_LABEL).
{% endif %}Rules:
1. Keep every placeholder exactly as it is and the same number of times: %s %d %0.2f {name} {0} [b] \n.
2. Respect max_len when given.
3. If the text is already good, return it unchanged.
=== user ===
{% for it in items %}<str id="{{ it.id }}" context="{{ it.context }}"{% if it.max_len %} max_len="{{ it.max_len }}"{% endif %} placeholders="{{ it.placeholders | join(' ') }}">{{ it.text }}</str>
{% endfor %}
