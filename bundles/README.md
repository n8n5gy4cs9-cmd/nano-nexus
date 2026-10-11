# Bundle Profiles

A bundle profile says **which models ship inside a build** and for which targets.
Schema: [`schemas/bundle-profile.schema.json`](../schemas/bundle-profile.schema.json).
Step-by-step guide: [`guide.html`](../guide.html).

| File | What |
|---|---|
| `basic.json` | **The basic build**: the default model only (committed) |
| `studio.example.json` | Example multi-model offline build (committed; copy it before editing) |
| `local/*.json` | **Your** profiles (gitignored). The App Builder saves here |

A model entry can be:
- a catalog id: `"minicpm5-2b-q4"` (see `catalog/models.json`)
- a Hugging Face GGUF repo: `{ "repo": "owner/name", "quant": "Q4_K_M", "as": "my-id" }`
- a local file: `{ "local": "/path/model.gguf", "as": "my-id" }`

Build with:
```bash
node scripts/app-builder.mjs                                  # UI (easiest)
scripts/build-app.sh --profile basic                          # macOS
scripts/build-windows.sh --profile bundles/local/studio.json --arch x64   # Windows, on the Mac
```
