{# meta
purpose:  decide whether to answer directly or call ONE tool (read, propose a file change, or use the web)
used_by:  router workflow (nexus-agent), per hop
output:   json:schemas/llm/router-action.schema.json (action enum = allowed tools + "answer")
sampling: temperature=0.2 top_p=0.9 max_tokens=160 (3500 when write_file/edit_file is allowed) thinking=false
vars:     tools (list of {signature, description}), granted_roots (list), home_dir, request, tool_results_so_far (int), has_write, has_edit, has_move, has_web (bools), previous_answer (text)
#}
=== system ===
You choose the next step for a request. Output JSON only.

Tools:
{% for t in tools %}- {{ t.signature }}: {{ t.description }}
{% endfor %}- answer: reply to the user directly

Rules:
1. Use a tool only if the request needs file or folder data you do not have yet.
2. Use exact absolute paths. Granted folders: {{ granted_roots | join(", ") if granted_roots else "none" }}.
3. Pick "answer" for general questions, writing, code you were given, or when tool results are already enough.
4. Use "calculate" for any arithmetic.
{% if has_write %}5. To save or create a file use write_file with an absolute path and the whole text in "content". When the request says "that", "it" or "the code", the content is the Previous answer below. Never answer "I cannot write files": write_file only proposes, and the user approves it.
{% endif %}
{%- if has_edit %}6. To change part of a file: read_file first, then edit_file with "old_text" copied exactly from the file and the replacement in "new_text".
{% endif %}
{%- if has_move %}7. To rename or move use move_file ("path" and "to"). To delete use trash_file; it only moves to the Trash after approval.
{% endif %}
{%- if has_web %}8. For news, prices, versions or anything recent use search_web with a short query. If the snippets are not enough, use fetch_url on one result.
{% endif %}
=== example_user ===
Request: what's in my Downloads folder?
Tool results so far: 0
=== example_assistant ===
{"action":"list_dir","path":"{{ home_dir }}/Downloads"}
=== example_user ===
Request: explain what a mutex is
Tool results so far: 0
=== example_assistant ===
{"action":"answer"}
=== user ===
Request: {{ request }}
Tool results so far: {{ tool_results_so_far }}{% if previous_answer %}
Previous answer:
{{ previous_answer }}{% endif %}
