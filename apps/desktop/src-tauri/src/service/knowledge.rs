//! Knowledge libraries (M17): add / index / search / remove, plus the glue that feeds `kb_search` and
//! chat context injection. Thin over `nexus-knowledge`; nothing here runs unless a library exists and is enabled.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use nexus_agent::tools::{KnowledgeHits, KnowledgeSource};
use nexus_core::error::NexusError;
use nexus_knowledge::extract::{is_supported, list_files};
use nexus_knowledge::{
    DocumentRow, IndexOpts, KbError, Library, LibrarySource, Passage, SearchOpts, build_context,
    index_library, repo, search,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

use crate::state::AppCtx;

/// Progress of an index run (sent after every file).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct KbProgress {
    pub library_id: String,
    pub files_done: u32,
    pub files_total: u32,
    pub current: String,
    pub chunks: u32,
    pub skipped: u32,
}

/// What an index run did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct KbIndexResult {
    pub library_id: String,
    pub indexed: u32,
    pub unchanged: u32,
    pub skipped: u32,
    pub failed: u32,
    pub removed: u32,
    pub chunks: u32,
    /// The library size cap stopped the run early.
    pub truncated: bool,
}

/// A library with its documents (the Library page's detail view).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct KbLibraryDetail {
    pub library: Library,
    pub documents: Vec<DocumentRow>,
}

fn jobs() -> &'static Mutex<HashMap<String, CancellationToken>> {
    static JOBS: OnceLock<Mutex<HashMap<String, CancellationToken>>> = OnceLock::new();
    JOBS.get_or_init(Default::default)
}

fn kb(e: KbError) -> NexusError {
    NexusError::from(e)
}

fn knowledge_root(ctx: &AppCtx) -> PathBuf {
    ctx.paths.data.join("knowledge")
}

/// Every library.
pub fn list(ctx: &AppCtx) -> Result<Vec<Library>, NexusError> {
    repo::list_libraries(&ctx.store).map_err(kb)
}

/// One library with its documents (skipped and failed files carry their notes).
pub fn detail(ctx: &AppCtx, id: &str) -> Result<KbLibraryDetail, NexusError> {
    Ok(KbLibraryDetail {
        library: repo::get_library(&ctx.store, id).map_err(kb)?,
        documents: repo::list_documents(&ctx.store, id).map_err(kb)?,
    })
}

/// Does any enabled library exist? (When not, chat and assistants behave exactly as before.)
pub fn any_enabled(ctx: &AppCtx) -> bool {
    list(ctx)
        .map(|l| l.iter().any(|l| l.enabled && l.chunk_count > 0))
        .unwrap_or(false)
}

/// The user picked `path` (dialog or drop). Protected locations are still refused.
fn check_allowed(ctx: &AppCtx, path: &Path) -> Result<PathBuf, NexusError> {
    let canon = nexus_fs::canonicalize(path)
        .map_err(|e| NexusError::InvalidInput(format!("{}: {e}", path.display())))?;
    if let Some(rule) = ctx
        .perms
        .deny_rules()
        .into_iter()
        .find(|r| r.matches(canon.as_path()))
    {
        return Err(NexusError::PathDenied(format!(
            "{} ({})",
            path.display(),
            rule.label
        )));
    }
    Ok(canon.into_path_buf())
}

fn copy_into(dest: &Path, sources: &[PathBuf]) -> Result<usize, NexusError> {
    std::fs::create_dir_all(dest)?;
    let mut n = 0;
    for src in sources {
        if src.is_dir() {
            let base = src
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            for (file, rel) in list_files(src) {
                let to = dest.join(&base).join(&rel);
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&file, &to)?;
                n += 1;
            }
        } else if src.is_file() && is_supported(src) {
            let name = src.file_name().map(|s| s.to_owned()).unwrap_or_default();
            std::fs::copy(src, dest.join(name))?;
            n += 1;
        }
    }
    Ok(n)
}

/// Create a library from folders and/or files. `reference` reads one folder in place; otherwise the
/// supported files are copied into the app data folder. Indexing is a separate call ([`reindex`]).
pub async fn add(
    ctx: &Arc<AppCtx>,
    name: Option<String>,
    paths: Vec<String>,
    reference: bool,
) -> Result<Library, NexusError> {
    if paths.is_empty() {
        return Err(NexusError::InvalidInput("choose a folder or files to add".into()));
    }
    let mut sources = Vec::new();
    for p in &paths {
        sources.push(check_allowed(ctx, Path::new(p))?);
    }
    let default_name = sources
        .first()
        .and_then(|p| p.file_stem().or_else(|| p.file_name()))
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Library".into());
    let name = name.filter(|n| !n.trim().is_empty()).unwrap_or(default_name);
    if reference {
        let [only] = sources.as_slice() else {
            return Err(NexusError::InvalidInput(
                "reference in place works with exactly one folder".into(),
            ));
        };
        if !only.is_dir() {
            return Err(NexusError::InvalidInput(
                "reference in place needs a folder".into(),
            ));
        }
        return repo::create_library(
            &ctx.store,
            &name,
            LibrarySource::User,
            &only.to_string_lossy(),
            "reference",
        )
        .map_err(kb);
    }
    let dest = knowledge_root(ctx).join(uuid_dir());
    let copy_dest = dest.clone();
    let copied = tokio::task::spawn_blocking(move || copy_into(&copy_dest, &sources))
        .await
        .map_err(|e| NexusError::Internal(e.to_string()))?;
    let copied = match copied {
        Ok(n) => n,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(e);
        }
    };
    if copied == 0 {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(NexusError::InvalidInput(
            "no supported files found (md, txt, json, html, pdf, epub, docx, csv, source files)".into(),
        ));
    }
    repo::create_library(
        &ctx.store,
        &name,
        LibrarySource::User,
        &dest.to_string_lossy(),
        "copy",
    )
    .map_err(kb)
}

fn uuid_dir() -> String {
    // Folder names only need to be unique and filesystem-safe.
    format!(
        "lib-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

/// Index (or incrementally re-index) a library on the blocking pool. `job_id` is the handle for [`cancel`].
pub async fn reindex(
    ctx: &Arc<AppCtx>,
    library_id: &str,
    job_id: &str,
    progress: Arc<dyn Fn(KbProgress) + Send + Sync>,
) -> Result<KbIndexResult, NexusError> {
    let cancel = CancellationToken::new();
    if let Ok(mut m) = jobs().lock() {
        m.insert(job_id.to_owned(), cancel.clone());
    }
    let store = ctx.store.clone();
    let id = library_id.to_owned();
    let lib_id = id.clone();
    let res = tokio::task::spawn_blocking(move || {
        index_library(&store, &id, &IndexOpts::default(), &cancel, |p| {
            progress(KbProgress {
                library_id: id.clone(),
                files_done: p.files_done as u32,
                files_total: p.files_total as u32,
                current: p.current.clone(),
                chunks: p.chunks as u32,
                skipped: p.skipped as u32,
            })
        })
    })
    .await;
    if let Ok(mut m) = jobs().lock() {
        m.remove(job_id);
    }
    match res.map_err(|e| NexusError::Internal(e.to_string()))? {
        Ok(s) => Ok(KbIndexResult {
            library_id: lib_id,
            indexed: s.indexed as u32,
            unchanged: s.unchanged as u32,
            skipped: s.skipped as u32,
            failed: s.failed as u32,
            removed: s.removed as u32,
            chunks: s.chunks as u32,
            truncated: s.truncated,
        }),
        Err(e) => Err(kb(e)),
    }
}

/// Stop a running index job.
pub fn cancel(job_id: &str) -> bool {
    match jobs().lock().ok().and_then(|m| m.get(job_id).cloned()) {
        Some(t) => {
            t.cancel();
            true
        }
        None => false,
    }
}

pub fn set_enabled(ctx: &AppCtx, id: &str, enabled: bool) -> Result<(), NexusError> {
    repo::set_enabled(&ctx.store, id, enabled).map_err(kb)
}

/// Remove a library. Files the app copied are deleted; referenced folders and bundled libraries are never touched.
pub fn remove(ctx: &AppCtx, id: &str) -> Result<(), NexusError> {
    let lib = repo::get_library(&ctx.store, id).map_err(kb)?;
    if lib.source == LibrarySource::Bundled {
        return Err(NexusError::InvalidInput(
            "bundled libraries cannot be removed; disable them instead".into(),
        ));
    }
    repo::remove_library(&ctx.store, id).map_err(kb)?;
    let path = PathBuf::from(&lib.path);
    if lib.mode == "copy" && path.starts_with(knowledge_root(ctx)) && path != knowledge_root(ctx) {
        let _ = std::fs::remove_dir_all(&path);
    }
    Ok(())
}

/// Passage search for the Library page ("what does the model see?").
pub fn search_passages(
    ctx: &AppCtx,
    query: &str,
    library_ids: Option<Vec<String>>,
    k: u32,
) -> Result<Vec<Passage>, NexusError> {
    let opts = SearchOpts {
        library_ids,
        k: (k as usize).clamp(1, 20),
        ..Default::default()
    };
    search(&ctx.store, query, &opts).map_err(kb)
}

/// One passage in full (a citation was clicked).
pub fn passage(ctx: &AppCtx, chunk_id: i64) -> Result<Passage, NexusError> {
    repo::get_passage(&ctx.store, chunk_id).map_err(kb)
}

// ---- bundled libraries ------------------------------------------------------------------------

/// `resources/knowledge/manifest.json` (written by `stage-bundle`, BUNDLING §9).
#[derive(Debug, Deserialize)]
struct Manifest {
    libraries: Vec<ManifestLibrary>,
}

#[derive(Debug, Deserialize)]
struct ManifestLibrary {
    id: String,
    name: String,
    /// Folder name below `knowledge/`.
    dir: String,
}

/// Register the libraries shipped in the app resources (read-only) and index any that are new or
/// changed. Safe to call on every start: unchanged files are skipped by hash.
pub async fn sync_bundled(ctx: &Arc<AppCtx>) {
    let root = ctx.resource_dir.join("knowledge");
    let Ok(text) = std::fs::read_to_string(root.join("manifest.json")) else {
        return;
    };
    let manifest: Manifest = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("knowledge manifest is unreadable: {e}");
            return;
        }
    };
    for m in manifest.libraries {
        // Manifest names are untrusted path input: one plain component only.
        if m.dir.is_empty() || m.dir.contains(['/', '\\']) || m.dir == ".." || m.dir == "." {
            continue;
        }
        let dir = root.join(&m.dir);
        let path = dir.to_string_lossy().into_owned();
        let existing = list(ctx)
            .ok()
            .and_then(|l| l.into_iter().find(|l| l.path == path));
        let lib = match existing {
            Some(l) => l,
            None => {
                match repo::create_library(&ctx.store, &m.name, LibrarySource::Bundled, &path, "reference") {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::warn!(library = %m.id, "could not register bundled library: {e}");
                        continue;
                    }
                }
            }
        };
        let job = format!("bundled-{}", m.id);
        if let Err(e) = reindex(ctx, &lib.id, &job, Arc::new(|_| {})).await {
            tracing::warn!(library = %m.id, "could not index bundled library: {e}");
        }
    }
}

// ---- chat glue --------------------------------------------------------------------------------

/// `KnowledgeSource` over the app store. Remembers the passages of the last search so the finished
/// answer's `[n]` citations can be verified against exactly what the model saw.
pub struct AppKnowledge {
    ctx: Arc<AppCtx>,
    last: Mutex<Vec<Passage>>,
}

impl AppKnowledge {
    /// `None` when no library is enabled and indexed: callers then change nothing.
    pub fn if_enabled(ctx: &Arc<AppCtx>) -> Option<Arc<Self>> {
        any_enabled(ctx).then(|| {
            Arc::new(Self {
                ctx: ctx.clone(),
                last: Mutex::new(Vec::new()),
            })
        })
    }

    /// Run retrieval for a chat turn. Returns the context block, or `None` when nothing relevant was found.
    pub fn context_for(&self, query: &str, max_tokens: u32) -> Option<String> {
        let hits = KnowledgeSource::search(self, query, None, max_tokens).ok()?;
        (hits.passages > 0).then_some(hits.context)
    }

    /// Passages the model was given by the last search.
    pub fn last_passages(&self) -> Vec<Passage> {
        self.last.lock().map(|l| l.clone()).unwrap_or_default()
    }

    /// Verify the answer's `[n]` markers: drop invented ones and append a "Sources" list for the valid ones.
    pub fn finalize_answer(&self, answer: &str) -> String {
        let passages = self.last_passages();
        if passages.is_empty() {
            return answer.to_owned();
        }
        let cleaned = nexus_knowledge::prompt::strip_invalid_citations(answer, passages.len());
        let report = nexus_knowledge::verify_citations(&cleaned, passages.len());
        if report.cited.is_empty() {
            return cleaned;
        }
        let mut out = cleaned.trim_end().to_owned();
        out.push_str("\n\n**Sources**\n");
        for n in report.cited {
            let p = &passages[n - 1];
            let page = p.page.map(|pg| format!(", p. {pg}")).unwrap_or_default();
            // A markdown link the chat turns into "open this passage" (see features/library/types.ts).
            let label = format!("{}{page}", p.source()).replace(['[', ']'], " ");
            out.push_str(&format!(
                "- [[{n}] {label}](https://nanonexus.local/passage/{})\n",
                p.chunk_id
            ));
        }
        out
    }
}

impl KnowledgeSource for AppKnowledge {
    fn search(&self, query: &str, library: Option<&str>, max_tokens: u32) -> Result<KnowledgeHits, String> {
        let libs = list(&self.ctx).map_err(|e| e.to_string())?;
        let library_ids = match library.map(str::trim).filter(|s| !s.is_empty()) {
            None => None,
            Some(want) => {
                let ids: Vec<String> = libs
                    .iter()
                    .filter(|l| l.id == want || l.name.eq_ignore_ascii_case(want))
                    .map(|l| l.id.clone())
                    .collect();
                if ids.is_empty() {
                    return Err(format!("There is no library called `{want}`."));
                }
                Some(ids)
            }
        };
        let opts = SearchOpts {
            library_ids,
            max_tokens: max_tokens as usize,
            ..Default::default()
        };
        let passages = search(&self.ctx.store, query, &opts).map_err(|e| e.to_string())?;
        let (context, kept) = build_context(&passages, max_tokens as usize);
        let n = kept.len();
        if let Ok(mut last) = self.last.lock() {
            *last = kept;
        }
        Ok(KnowledgeHits { context, passages: n })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn add_index_search_remove_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppCtx::for_tests(dir.path());
        let src = dir.path().join("books");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("guide.md"),
            "# Guide\n\n## Sends\n\nA send routes audio to a bus.",
        )
        .unwrap();
        std::fs::write(src.join("pic.png"), [0u8, 1, 2]).unwrap();

        let lib = add(
            &ctx,
            Some("Guide".into()),
            vec![src.to_string_lossy().into_owned()],
            false,
        )
        .await
        .unwrap();
        assert!(
            lib.path
                .starts_with(&knowledge_root(&ctx).to_string_lossy().into_owned())
        );
        let res = reindex(&ctx, &lib.id, "job-1", Arc::new(|_| {})).await.unwrap();
        assert_eq!(res.indexed, 1);

        let hits = search_passages(&ctx, "how does a send work", None, 4).unwrap();
        assert!(!hits.is_empty());
        assert!(passage(&ctx, hits[0].chunk_id).is_ok());

        let source = AppKnowledge::if_enabled(&ctx).expect("enabled");
        let found = KnowledgeSource::search(&*source, "send routes audio", None, 1200).unwrap();
        assert_eq!(found.passages, 1);
        let answer = source.finalize_answer("It routes audio to a bus [1][4].");
        assert!(answer.contains("[1]") && !answer.contains("[4]") && answer.contains("**Sources**"));
        assert!(KnowledgeSource::search(&*source, "x", Some("nope"), 1200).is_err());

        set_enabled(&ctx, &lib.id, false).unwrap();
        assert!(AppKnowledge::if_enabled(&ctx).is_none());
        remove(&ctx, &lib.id).unwrap();
        assert!(!Path::new(&lib.path).exists());
        assert!(list(&ctx).unwrap().is_empty());
    }

    #[tokio::test]
    async fn adding_nothing_supported_or_nothing_at_all_fails() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppCtx::for_tests(dir.path());
        assert!(add(&ctx, None, vec![], false).await.is_err());
        let empty = dir.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(
            add(&ctx, None, vec![empty.to_string_lossy().into_owned()], false)
                .await
                .is_err()
        );
        assert!(!cancel("missing"));
    }
}
