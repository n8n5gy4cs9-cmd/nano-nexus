# Opt-in web search (UP.047)

Source audit and change: 2026-11-02. Source only; nothing was built, run or tested, and no network request was made.

## Decision

The reference (`scripts/search/*.js`) searches DuckDuckGo's HTML endpoint, fetches pages and caches results on
disk. NanoNexus already ships the same provider behind ADR-016 (`nexus_models::web::WebClient`, tools
`search_web` and `fetch_url`). The NanoNexus path is stricter than the reference, so it is kept and the
reference code is not ported.

| Concern | Reference | NanoNexus (kept) |
|---|---|---|
| Consent | Chat toggle | Settings, Tools, "Allow web access" (`tools.web_enabled`), off by default; tools unavailable until on |
| Network boundary | Plain `https.get`, follows redirects | Public http(s) only, DNS pinned and re-checked on every redirect, no loopback/LAN/credentials in URLs, 12 s timeout, 1.5 MB cap |
| Output handling | Prompt text appended to user message | Wrapped in `<web_results>` / `<web_page>` inside `<tool_result>` as data; injection can at worst propose a change a user must approve |
| Credentials | None (no key) | None. No provider keys exist, so none are stored |
| Cache | Disk cache under `app/cache/search` | Not ported. A disk cache would be another write destination to audit (UP.070); results are not cached |

## Added in this task

- `TimeFilter` (day, week, month, year) sent as DuckDuckGo `df`, exposed as an optional `time` argument of `search_web`; unknown values are rejected with a message the model can act on.
- `WebClient::search_within`; `search` keeps its signature. `WebSource::search_within` has a default that ignores the filter, so other sources and test fakes need no change.
- Queries are trimmed, empty queries are refused, and queries are capped at 300 characters so a long prompt is not sent off the computer whole.
- Double quotes in the query are neutralised in the `<web_results query="...">` attribute.

## Not done

- A chat-composer web toggle and time-filter control (reference UI). Assistants use the Settings opt-in; a per-chat toggle needs an agreed UI and is not implemented.
- Providers beyond DuckDuckGo, and any provider needing a key (would need memory-only secrets, UP.080).
- Parallel fetch of top results with a source-citation block. The model can call `fetch_url` itself.
- Tests were added (`web.rs`, `tools.rs`) but have not been run.
