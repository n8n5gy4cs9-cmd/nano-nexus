# Internal Prompts (pipelines and helpers)

These are the prompts NanoNexus uses **inside** pipelines (not the assistant personas, which
live in `assistants/`). They're embedded at compile time and rendered with MiniJinja.

## File format
```
{# meta
purpose:  one line
used_by:  pipeline / feature
output:   stream | json:<schema file in schemas/llm/>
sampling: temperature=0.2 top_p=0.9 max_tokens=400 thinking=false
vars:     list of template variables
#}
=== system ===
…system message template…
=== example_user ===        (optional, repeatable, must be paired with example_assistant)
…
=== example_assistant ===
…
=== user ===
…user message template…
```
- The loader splits on lines that are exactly `=== <section> ===`. Section order is
  system → (example_user, example_assistant)* → user.
- Examples are sent as real prior turns (few-shot works better than examples in prose for small models).
- **Budgets:** system ≤ 350 tokens. Examples ≤ 300 tokens total. The `user` part carries the data.
- Data always goes inside tags (`<facts>`, `<doc>`, `<sym>`, `<diff>`, `<file>`) and is never
  mixed with instructions.
- JSON outputs are additionally constrained by the schema (grammar), so the prompt only needs
  to explain the *meaning* of the fields.

| File | Output | Used by |
|---|---|---|
| `router.md` | json `router-action` (enum built at runtime) | router workflow |
| `title.md` | json `title` | conversation auto-title |
| `history_summary.md` | stream | context budgeting |
| `reask.md` | (user message) | structured-output retry |
| `folder_report_narrative.md` | stream | folder_report |
| `file_classify.md` | json `file-classify` | folder_report (AI categories) |
| `doc_map.md` | json `doc-map-notes` | doc_summarize (map) |
| `doc_reduce.md` | stream | doc_summarize (reduce) |
| `doc_qa.md` | stream | doc_qa |
| `rename_propose.md` | json `rename-proposals` | rename_propose |
| `strings_improve.md` | json `strings-proposals` | strings_improve |
| `tidy_classify.md` | json `tidy-classify` | tidy_plan |
| `secrets_summary.md` | stream | secrets_report |
| `log_triage.md` | stream | log_triage |
| `data_profile.md` | stream | data_profile |
| `commit_message.md` | json `commit-message` | commit_message |
| `file_report_code.md` | stream | file report (code) |
| `editor_rewrite.md` | stream (one code fence) | editor AI actions |
