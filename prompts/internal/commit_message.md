{# meta
purpose:  Conventional Commit message from a diff (or per-file summaries for big diffs)
used_by:  commit_message pipeline (big diffs: Rust first summarizes stats per file and truncates hunks)
output:   json:schemas/llm/commit-message.schema.json (rendered to text by Rust)
sampling: temperature=0.2 top_p=0.9 max_tokens=400 thinking=false
vars:     stats (files changed, insertions, deletions), diff (possibly truncated)
#}
=== system ===
You write a Conventional Commits 1.0.0 message for the diff. Output JSON only.
- type: feat (new feature), fix (bug fix), docs, style, refactor, perf, test, build, ci, chore, or revert.
- scope: the main module or folder, lowercase, or empty.
- subject: imperative mood, no period, at most 72 characters ("add pause menu", not "added pause menu").
- body: 0–5 short lines on what changed and why. Only facts visible in the diff.
- breaking: true only if the diff removes or changes a public API or save format.
=== example_user ===
<stats>2 files, +14 −3</stats>
<diff>
--- a/player.gd
+++ b/player.gd
@@ func _physics_process(delta):
-    velocity.y += 9.8
+    velocity.y += gravity * delta
+@export var gravity: float = 980.0
</diff>
=== example_assistant ===
{"type":"fix","scope":"player","subject":"scale gravity by delta and make it configurable","body":["Gravity was frame-rate dependent.","Exported gravity so it can be tuned per scene."],"breaking":false}
=== user ===
<stats>{{ stats }}</stats>
<diff>
{{ diff }}
</diff>
