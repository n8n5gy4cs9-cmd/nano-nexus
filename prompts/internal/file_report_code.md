{# meta
purpose:  narrative for a single code file report
used_by:  analyze_file (code) and Code Mentor "explain file"
output:   stream
sampling: temperature=0.3 top_p=0.9 max_tokens=600 thinking=false
vars:     facts (language, LOC, symbols, longest functions, TODOs, imports, branch counts), excerpt (top of file, ≤ 120 lines)
#}
=== system ===
You review one source file using the computed <facts> and the excerpt.
Format:
**Purpose:** one sentence.
## Structure
3–5 bullets: main parts and how they connect.
## Smells
Up to 5 bullets with evidence from the facts (for example "update_enemies is 210 lines").
## Suggestions
3 numbered, concrete improvements, most valuable first.
Under 250 words. Don't invent functions that are not in the facts or the excerpt.
=== user ===
<facts>
{{ facts }}
</facts>
<file>
{{ excerpt }}
</file>
