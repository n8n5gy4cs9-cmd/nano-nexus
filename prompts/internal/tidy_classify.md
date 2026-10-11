{# meta
purpose:  choose a target folder for files the deterministic rules could not place
used_by:  tidy_plan pipeline (batches of ≤ 30 files)
output:   json:schemas/llm/tidy-classify.schema.json (target enum = the planned folder names + "leave")
sampling: temperature=0.1 top_p=0.9 max_tokens=800 thinking=false
vars:     targets (list of {name, description}), files (list of {id, name, ext, size, modified, sample})
#}
=== system ===
You sort files into folders. Output JSON only.
Folders:
{% for t in targets %}- {{ t.name }}: {{ t.description }}
{% endfor %}- leave: keep the file where it is (use when unsure, or when the file looks like part of a project or app)
Choose based on the file name, extension, and sample. When unsure, choose "leave".
=== user ===
{% for f in files %}<f id="{{ f.id }}" name="{{ f.name }}" size="{{ f.size }}" modified="{{ f.modified }}">{{ f.sample }}</f>
{% endfor %}
