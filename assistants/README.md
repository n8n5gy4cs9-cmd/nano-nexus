# Built-in Assistants

Each `*.toml` file here is one assistant. It is embedded into the app at compile time
(`include_dir!`) and validated by the `prompt_lint` test (`crates/nexus-agent`, M06).
Format: [`schemas/assistant.schema.json`](../schemas/assistant.schema.json). Design rules and
their sources: [`docs/architecture/ASSISTANTS.md` §2](../docs/architecture/ASSISTANTS.md).

| File | Name | Workflow | Notes |
|---|---|---|---|
| `nexus.toml` | Nexus | router | Default assistant. Reads granted files and folders via tools |
| `doc-lens.toml` | DocLens | pipeline `doc_qa` | PDF and doc Q&A with `[p. N]` citations, plus summaries |
| `folder-scout.toml` | Folder Scout | pipeline `folder_report` | Folder reports, cleanup plans |
| `rename-forge.toml` | Rename Forge | pipeline `rename_propose` | Renames identifiers, files, and folders in any code project, updating every reference |
| `code-mentor.toml` | Code Mentor | chat | Explain and review code |
| `bug-hunter.toml` | Bug Hunter | chat | Scientific debugging (thinking on by default) |
| `doc-writer.toml` | Doc Writer | chat | Doc comments and READMEs (Diátaxis) |
| `commit-crafter.toml` | Commit Crafter | pipeline `commit_message` | Conventional Commits 1.0.0 |
| `tidy.toml` | Tidy | pipeline `tidy_plan` | Move plans, never deletes |
| `secret-sentinel.toml` | Secret Sentinel | pipeline `secrets_report` | Masked secret and PII findings |
| `log-detective.toml` | Log Detective | pipeline `log_triage` | Error clusters and timeline |
| `data-wrangler.toml` | Data Wrangler | pipeline `data_profile` | CSV/JSON profiling, SQL |
| `tldr.toml` | TL;DR | chat | Dense summaries |
| `scribe.toml` | Scribe | chat | Plain-language writing and proofreading |
| `polyglot.toml` | Polyglot | chat | Translation that keeps placeholders |
| `shell-sage.toml` | Shell Sage | chat | Command explainer with safety ratings, never runs anything |
| `prompt-architect.toml` | Prompt Architect | chat | Writes new assistants as TOML |
| `_TEMPLATE.toml` | — | — | Starting point for your own assistants (not loaded) |
| `regex-smith.example.toml` | Regex Smith | chat | Tutorial example (not loaded; import it to try) |

**Loading rule:** files that start with `_` or end in `.example.toml` are **not** loaded as built-ins.

## File layout rule (important)
TOML assigns every key after a `[table]` header to that table. So the long prompt lives in a
final `[prompt]` table:

```toml
[ui]
starters = ["…"]

[prompt]
system = '''
Your job: …
'''
```
Use **literal** strings (`'''`) so backslashes and quotes in prompts stay as written. Never
write `'''` inside a prompt.

## Budgets (checked by prompt_lint)
- `prompt.system` ≤ 450 tokens (all built-ins are about 200–340). The shared preamble (~90 tokens)
  is added automatically. Don't repeat it.
- 1–4 starters. The icon must be a lucide icon name, and the color one of the nine assistant hues.

## Template variables (MiniJinja)
`{{ name }}`, `{{ os_name }}`, `{{ today }}`, `{{ app_version }}`, `{{ user_name }}`,
`{{ granted_roots }}` (list), `{{ attachments }}` (list of `{kind, path, tokens}`),
`{{ tools }}` (list of `{name, signature, description}`).
