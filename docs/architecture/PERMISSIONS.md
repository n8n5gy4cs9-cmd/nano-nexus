# Permissions and Safe File Operations (`crates/nexus-fs`)

> Invariant: **no byte on disk changes unless (1) a grant allows it, (2) the user saw a preview
> and clicked Apply, and (3) the original is in the journal.** Deletion is always "move to Trash".

## 1. Two layers

| Layer | Who enforces | What |
|---|---|---|
| **App grants** (authoritative) | `nexus-fs::PermissionEngine` (Rust) | Which folders an assistant/tool may read, write, create, move, or trash |
| **OS consent** | macOS TCC / Windows Controlled Folder Access / POSIX perms | Whether the *process* may touch protected locations at all |

An operation needs **both**. The app layer is checked first (it gives a clear message). The OS
layer shows up as `EPERM`/`EACCES`, which we detect and explain.

## 2. Grants

```rust
pub enum GrantLevel { Read, ReadWrite, Full }        // Full = ReadWrite + Create + Move + Trash
pub enum Access { Read, Write, Create, Move, Trash } // what an operation needs
pub enum GrantScope { Persistent, Session }          // Session grants vanish on app quit

pub struct Grant {
    pub id: GrantId,
    pub root: PathBuf,            // canonical, absolute, no trailing slash
    pub level: GrantLevel,
    pub recursive: bool,          // default true
    pub scope: GrantScope,
    pub expires_at: Option<OffsetDateTime>,
    pub label: Option<String>,    // e.g. "Old Godot game"
    pub created_at: OffsetDateTime,
}
```

Level → allowed access:

| Access ↓ / Level → | Read | ReadWrite | Full |
|---|---|---|---|
| Read (read file, list dir, metadata) | ✅ | ✅ | ✅ |
| Write (modify existing file) | ❌ | ✅ | ✅ |
| Create (new file/dir) | ❌ | ❌ | ✅ |
| Move / rename | ❌ | ❌ | ✅ |
| Trash | ❌ | ❌ | ✅ |

## 3. The check algorithm (`PermissionEngine::check(path, access)`)

```
1. Make absolute (relative paths are rejected; tools must pass absolute paths).
2. Canonicalize:
   - exists      → std::fs::canonicalize (resolves symlinks, .., case on macOS)
   - not exists  → canonicalize the nearest existing ancestor, then re-append the remaining
                   components, rejecting any remaining component that is "..", "." or empty
   - Windows: use dunce::canonicalize (no \\?\ prefix); compare case-insensitively
3. Deny list: if the canonical path is inside any deny root → Err(Denied{rule}). Deny ALWAYS wins.
4. Find grants whose root is a component-wise ancestor of the path (Path::starts_with, never
   string prefix: "/a/bc" must not match the grant "/a/b"). Non-recursive grants only match
   direct children.
5. Skip expired grants. Take the max level among matches.
6. If the level allows the access → Ok(CanonicalPath). Otherwise → Err(NoGrant{path, needed}).
7. Audit (allowed or denied) → audit_log (batched insert).
```

**Symlink policy:** because we canonicalize first, a symlink inside a granted folder that points
outside resolves to the outside path and fails step 4. Scans use `follow_links(false)` and report
symlinks as entries without following them.

**TOCTOU:** writes happen only inside `Journal::apply`, which re-checks immediately before each
op. Base hashes detect concurrent edits.

### Default deny list (macOS; extend per OS)

```
~/.ssh  ~/.gnupg  ~/.aws  ~/.config/gcloud  ~/.kube  ~/.docker/config.json  ~/.netrc
~/Library/Keychains  ~/Library/Cookies  ~/Library/Mail  ~/Library/Messages
~/Library/Application Support/com.apple.TCC  ~/Library/Safari
/System  /usr  /bin  /sbin  /private/etc  /private/var/db  /Library/Keychains
<app data dir>                     (the app's own DB and journal: only internal code paths touch these)
```
Windows: `C:\Windows`, `C:\Program Files*`, `%APPDATA%\Microsoft\Credentials`, `%USERPROFILE%\.ssh`.
Linux: `/etc`, `/usr`, `/bin`, `/sbin`, `/boot`, `/proc`, `/sys`, `~/.ssh`, `~/.gnupg`.

Users can add their own deny entries in Settings → Privacy. Built-in entries can't be removed.

**Note:** Secret Sentinel *reads* project folders to find leaked secrets. That's fine; the deny
list protects the real credential stores. Scanning `~/.ssh` is blocked on purpose.

## 4. Permission prompts (UI flow)

When a tool needs access the user hasn't granted:

```
nexus-agent tool → PermissionEngine::check → Err(NoGrant{path, needed})
  → TurnSink::request_permission(PermissionRequest{ id, assistant, path, needed, reason })
  → app emits `permission:request` → UI modal:

  ┌──────────────────────────────────────────────────────────┐
  │ 🔐  Folder Scout wants to read                           │
  │     ~/Downloads                                          │
  │     "to build a size and clutter report"                 │
  │                                                          │
  │  [Deny]   [Allow once]   [Allow folder: Read ▾]          │
  └──────────────────────────────────────────────────────────┘
  (the dropdown offers Read · Read & Write · Full)

  → invoke permission_respond(id, decision) → engine.add_grant(...) or deny
  → the tool continues or returns PermissionDenied (the LLM is told "user denied access")
```

"Allow once" creates a `Session` grant for exactly that path (non-recursive if it's a file).
Requests time out after 5 minutes and count as deny.

## 5. Change sets (ADR-005)

```rust
pub struct ChangeSet { pub id: ChangeSetId, pub title: String, pub origin: Origin /* assistant id or "editor" */,
                       pub ops: Vec<FileOp>, pub created_at: OffsetDateTime }

pub enum FileOp {
    Edit   { path: PathBuf, base_hash: Blake3, edits: Vec<TextEdit> },   // byte-range edits, sorted, non-overlapping
    Write  { path: PathBuf, base_hash: Option<Blake3>, contents: String }, // full replace (editor save)
    Create { path: PathBuf, contents: String },
    Mkdir  { path: PathBuf },
    Move   { from: PathBuf, to: PathBuf },
    Trash  { path: PathBuf },
}
pub struct TextEdit { pub start: usize, pub end: usize, pub new_text: String } // UTF-8 byte offsets
```

**Preview** (`ChangeSet::preview(&engine) -> ChangePreview`):
- For each op: required access, current hash vs `base_hash` (conflict if different), and a
  unified diff (`similar` crate, 3 lines of context) for Edit/Write/Create. Move/Trash show
  path, size, and a type icon.
- Validates that edits don't overlap and fall on UTF-8 boundaries, that `Move` targets don't
  exist, and that `Create` doesn't overwrite.
- Returns `{ files[], totals{files, insertions, deletions, moves, trashes, bytes}, conflicts[], missing_grants[] }`.
  The UI uses `missing_grants` to ask for permission before Apply is enabled.

**Apply** (`Journal::apply(changeset) -> AppliedChangeSet`):
```
1. Re-check every op (grant + deny + base hash). Any failure → abort, nothing written.
2. mkdir <data>/journal/<id>/ ; write manifest.json {ops, status:"applying"}
3. For each op, in order:
   Edit/Write: copy original → journal/<id>/files/<n>.orig ; write temp in same dir ; fsync ; rename over
   Create:     record "created"; write temp + rename (fail if exists)
   Mkdir:      record "created_dir"
   Move:       record from/to ; std::fs::rename (cross-device → copy + verify + trash source)
   Trash:      record original path + size + hash ; trash::delete(path) (system Trash)
   Record post_hash for each written file.
4. manifest.status = "applied" ; DB changesets.status = applied ; audit entries ; notification.
5. If op k fails: roll back ops k-1..0 from the journal, set status "rolled_back", and surface the error.
```

**Undo** (`Journal::undo(id)`):
- For each applied op, in reverse: if the current hash ≠ post_hash, mark it a **conflict** and ask
  the user (keep theirs / restore ours anyway / skip). Restore originals, delete created files
  (to Trash), reverse moves, and for trashed files, show "Restore from Trash manually"
  (best effort: macOS has no reliable public API for Put Back, so we keep a journal copy of
  trashed files **under 50 MB** and can restore those directly).
- Status → `undone`.

Retention: journals are kept for 30 days or 2 GB (oldest pruned first), configurable.

## 6. OS layer

### macOS (primary)
- **Not sandboxed** (ADR-010). TCC protects Desktop, Documents, Downloads, iCloud Drive,
  removable and network volumes, plus other app containers.
- The first access to a protected location shows the system prompt, using our Info.plist usage
  strings (see BUNDLING.md). Picking a folder through `NSOpenPanel` (tauri-plugin-dialog) also
  counts as consent for that selection.
- **Detection:** an `io::ErrorKind::PermissionDenied` with raw OS error `1` (EPERM) on a path
  under a TCC-protected root means OS denial (EACCES = 13 means POSIX perms instead). Map EPERM
  to `ErrorCode::OsPermissionDenied { location }`.
- **Status probes** (`os_permissions_status` command): try `read_dir` on `~/Desktop`,
  `~/Documents`, `~/Downloads`, and for Full Disk Access, on
  `~/Library/Application Support/com.apple.TCC` (readable only with FDA). Report
  `granted | denied | not_determined` (not-determined can't be read without triggering a prompt,
  so only probe on user action, never at startup).
- **Deep links** (`opener` plugin):
  - Files & Folders: `x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders`
  - Full Disk Access: `x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles`
- **Stable signing identity** keeps TCC grants across rebuilds (see BUNDLING.md §5).
- The child `llama-server` only reads the GGUF in the bundle or App Support, which aren't
  TCC-protected, so it needs no grants.

### Windows (optional)
- No TCC. **Controlled Folder Access** (Defender) can block writes to Documents/Desktop:
  `ERROR_ACCESS_DENIED` (5) on write while reads work → show guidance ("Allow an app through
  Controlled folder access").
- Use `dunce` for canonical paths, compare case-insensitively, and handle `\\?\` long paths.
- `trash` crate → Recycle Bin.

### Linux (optional)
- POSIX perms (`EACCES`) → plain message. Trash uses the freedesktop spec (`trash` crate).
- Flatpak/Snap sandboxes aren't targeted. If added later, use the XDG Desktop Portal file chooser.

## 7. What tools may do (enforced in `nexus-agent`)

| Tool | Access | Notes |
|---|---|---|
| `read_file` | Read | max 256 KB of text into the LLM (chunked beyond that); binary → metadata only |
| `list_dir` | Read | max 500 entries, sorted dirs first, sizes included |
| `search_files` | Read | name glob/substring, max 200 hits, respects `.gitignore` if chosen |
| `grep_text` | Read | literal or regex, max 200 matches, 1 line of context each |
| `scan_folder` | Read | runs the analyzer and returns **facts** (not the raw list) to the LLM |
| `file_info` | Read | metadata + type-specific quick facts |
| `doc_search` | Read | BM25 over an attached and extracted document |
| `calculate` | none | deterministic arithmetic (`meval`-style evaluator); small models are bad at math |
| `now` | none | current date/time/timezone |
| *(none)* | Write/Create/Move/Trash | **Only pipelines create change sets; the user applies them** |

## 8. Tests (required for M6)

- Grant matching: component boundaries (`/a/b` vs `/a/bc`), non-recursive grants, expiry, max level.
- Symlink escape: a link inside the grant pointing outside → denied. A link inside pointing inside → ok.
- Non-existent targets: `Create` under a granted dir → ok. `..` in the remainder → rejected.
- The deny list beats a broader grant (grant `~` → `~/.ssh/id_ed25519` denied).
- Change set: overlapping edits rejected, base-hash conflict detected, mid-apply failure rolls
  back, undo restores byte-identical originals (property test with random edits via `proptest`).
- Windows-only tests behind `#[cfg(windows)]`: case-insensitivity, UNC handling.
