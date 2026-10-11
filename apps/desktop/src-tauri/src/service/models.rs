//! Models: what is installed, the curated catalog, Hugging Face search, the one-at-a-time download
//! queue, activation, import, and deletion.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nexus_core::error::NexusError;
use nexus_core::events::{NotificationAction, NotificationLevel};
use nexus_core::fs::Access;
use nexus_models::bundled::{self, VerifyResult};
use nexus_models::catalog::{Catalog, CatalogEntry};
use nexus_models::download::{self, DownloadError, DownloadProgress, DownloadSpec, Downloader, Phase};
use nexus_models::fit::{self, Fit, MemoryInfo};
use nexus_models::gguf::{self, GgufMeta};
use nexus_models::hf::{self, GgufFile, HfError, SearchHit};
use nexus_store::{DownloadRow, ModelRow, NewNotification};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

use crate::state::{AppCtx, AppEvent, Ctx};

use super::Caller;
use super::settings::SettingValue;

// ---- views ------------------------------------------------------------------------------------

/// A download as the UI shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct DownloadView {
    pub id: String,
    pub catalog_id: Option<String>,
    pub repo: String,
    pub file: String,
    pub bytes: u64,
    pub done_bytes: u64,
    /// `queued | running | paused | failed | done | cancelled`
    pub status: String,
    pub error: Option<String>,
    pub bytes_per_sec: f64,
    pub eta_secs: Option<u64>,
    pub phase: Option<Phase>,
}

/// How a model can be removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Removal {
    /// The file is deleted.
    Delete,
    /// Only the entry goes away (the file is somewhere the user keeps it).
    Unlink,
    /// Part of the install (the bundle).
    Protected,
}

#[derive(Clone, Debug, PartialEq, Serialize, Type)]
pub struct InstalledModel {
    pub id: String,
    /// `builtin | download | import | discovered`
    pub source: String,
    pub name: String,
    pub repo: Option<String>,
    pub file: Option<String>,
    pub path: String,
    pub file_exists: bool,
    pub bytes: u64,
    pub quant: Option<String>,
    pub catalog_id: Option<String>,
    pub license: Option<String>,
    pub capabilities: Vec<String>,
    pub supports_thinking_toggle: bool,
    pub has_chat_template: bool,
    pub context_length: Option<u64>,
    pub active: bool,
    pub removal: Removal,
    pub fit: Fit,
    pub gguf: Option<GgufMeta>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Type)]
pub struct CatalogCard {
    pub entry: CatalogEntry,
    pub installed_id: Option<String>,
    pub download: Option<DownloadView>,
    pub fit: Fit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Type)]
pub struct RepoFiles {
    pub repo: String,
    pub sha: String,
    pub license: Option<String>,
    pub gated: bool,
    pub files: Vec<GgufFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct ActivateResult {
    pub id: String,
    /// For example "This model has no chat template; replies may be poor."
    pub warning: Option<String>,
}

// ---- download manager -------------------------------------------------------------------------

const INTENT_NONE: u8 = 0;
const INTENT_PAUSE: u8 = 1;
const INTENT_CANCEL: u8 = 2;

struct Running {
    id: String,
    cancel: CancellationToken,
    intent: Arc<AtomicU8>,
}

#[derive(Clone, Copy)]
struct Live {
    bytes_per_sec: f64,
    eta_secs: Option<u64>,
    phase: Phase,
}

/// At most one download runs; the rest wait in the database queue.
#[derive(Default)]
pub struct DownloadManager {
    running: Mutex<Option<Running>>,
    live: Mutex<HashMap<String, Live>>,
}

fn hf_err(e: HfError) -> NexusError {
    match e {
        HfError::Offline(m) => NexusError::Network(m),
        HfError::NotFound => NexusError::NotFound("repository or file on Hugging Face".into()),
        HfError::TokenRequired => NexusError::PermissionDenied(
            "this repository needs a Hugging Face token (set the HF_TOKEN environment variable)".into(),
        ),
        HfError::Gated => NexusError::PermissionDenied(
            "this repository is gated: accept its terms on huggingface.co, then set HF_TOKEN".into(),
        ),
        other => NexusError::Network(other.to_string()),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_owned()
}

fn view(ctx: &AppCtx, d: &DownloadRow) -> DownloadView {
    let live = ctx.downloads.live.lock().ok().and_then(|m| m.get(&d.id).copied());
    DownloadView {
        id: d.id.clone(),
        catalog_id: d.catalog_id.clone(),
        repo: d.repo.clone(),
        file: d.file.clone(),
        bytes: d.bytes,
        done_bytes: d.done_bytes,
        status: d.status.clone(),
        error: d.error.clone(),
        bytes_per_sec: live.map_or(0.0, |l| l.bytes_per_sec),
        eta_secs: live.and_then(|l| l.eta_secs),
        phase: live.map(|l| l.phase),
    }
}

fn emit_download(ctx: &AppCtx, id: &str) {
    if let Ok(Some(d)) = ctx.store.get_download(id) {
        let _ = ctx.bus.send(AppEvent::DownloadProgress(view(ctx, &d)));
    }
}

fn notify(
    ctx: &AppCtx,
    level: NotificationLevel,
    title: String,
    body: Option<String>,
    action: Option<NotificationAction>,
) {
    let _ = ctx.bus.send(AppEvent::Notify(NewNotification {
        level,
        kind: "download".into(),
        title,
        body,
        action,
    }));
}

// ---- listing ----------------------------------------------------------------------------------

/// The id of the model the runtime should use right now.
pub fn active_model_id(ctx: &AppCtx) -> Option<String> {
    let saved = ctx.settings_snapshot().models_active_model_id;
    let saved = (saved != "builtin" && !saved.is_empty()).then_some(saved);
    let default = crate::runtime_paths::read_manifest(&ctx.resource_dir.join("models")).map(|m| m.default);
    bundled::resolve_active(&ctx.store, saved.as_deref(), default.as_deref())
        .ok()
        .flatten()
}

fn removal(source: &str) -> Removal {
    match source {
        "builtin" => Removal::Protected,
        "discovered" => Removal::Unlink,
        _ => Removal::Delete,
    }
}

fn installed_view(ctx: &AppCtx, m: &ModelRow, active: Option<&str>, mem: &MemoryInfo) -> InstalledModel {
    let meta: Option<GgufMeta> = m.gguf_meta.as_deref().and_then(|t| serde_json::from_str(t).ok());
    let extra: serde_json::Value = m
        .extra
        .as_deref()
        .and_then(|t| serde_json::from_str(t).ok())
        .unwrap_or(serde_json::Value::Null);
    let catalog = m.catalog_id.as_deref().and_then(|id| Catalog::embedded().get(id));
    let capabilities: Vec<String> = catalog
        .map(|c| c.capabilities.clone())
        .or_else(|| {
            extra
                .get("capabilities")
                .and_then(|c| serde_json::from_value(c.clone()).ok())
        })
        .unwrap_or_default();
    let ctx_size = ctx.settings_snapshot().runtime_ctx_size;
    InstalledModel {
        id: m.id.clone(),
        source: m.source.clone(),
        name: m.name.clone(),
        repo: m.repo.clone(),
        file: m.file.clone(),
        path: m.path.clone(),
        file_exists: Path::new(&m.path).is_file(),
        bytes: m.bytes,
        quant: m.quant.clone(),
        catalog_id: m.catalog_id.clone(),
        license: m.license.clone(),
        capabilities,
        supports_thinking_toggle: catalog.map(|c| c.supports_thinking_toggle).unwrap_or(false)
            || meta.as_ref().is_some_and(|g| g.supports_thinking_toggle),
        has_chat_template: meta.as_ref().is_none_or(|g| g.has_chat_template),
        context_length: meta.as_ref().and_then(|g| g.context_length),
        active: active == Some(m.id.as_str()),
        removal: removal(&m.source),
        fit: fit::estimate(meta.as_ref(), m.bytes, ctx_size, mem),
        gguf: meta,
    }
}

pub fn list_installed(ctx: &AppCtx) -> Result<Vec<InstalledModel>, NexusError> {
    let active = active_model_id(ctx);
    let mem = fit::system_memory();
    Ok(ctx
        .store
        .list_models()?
        .iter()
        .map(|m| installed_view(ctx, m, active.as_deref(), &mem))
        .collect())
}

pub fn catalog(ctx: &AppCtx) -> Result<Vec<CatalogCard>, NexusError> {
    let installed = ctx.store.list_models()?;
    let downloads = ctx.store.list_downloads()?;
    let mem = fit::system_memory();
    let ctx_size = ctx.settings_snapshot().runtime_ctx_size;
    Ok(Catalog::embedded()
        .models
        .iter()
        .map(|e| {
            let installed_id = installed
                .iter()
                .find(|m| m.catalog_id.as_deref() == Some(e.id.as_str()) && Path::new(&m.path).is_file())
                .map(|m| m.id.clone());
            let download = downloads
                .iter()
                .find(|d| {
                    d.catalog_id.as_deref() == Some(e.id.as_str())
                        && !matches!(d.status.as_str(), "done" | "cancelled")
                })
                .map(|d| view(ctx, d));
            CatalogCard {
                fit: fit::estimate(
                    None,
                    e.approx_bytes,
                    ctx_size.min(e.context_default.max(512)),
                    &mem,
                ),
                entry: e.clone(),
                installed_id,
                download,
            }
        })
        .collect())
}

pub fn downloads(ctx: &AppCtx) -> Result<Vec<DownloadView>, NexusError> {
    Ok(ctx.store.list_downloads()?.iter().map(|d| view(ctx, d)).collect())
}

pub fn system_memory() -> MemoryInfo {
    fit::system_memory()
}

pub async fn search(ctx: &AppCtx, query: &str) -> Result<Vec<SearchHit>, NexusError> {
    if query.trim().len() < 2 {
        return Ok(Vec::new());
    }
    check_network(ctx)?;
    ctx.hf.search(query, 30).await.map_err(hf_err)
}

/// A repo's downloadable GGUF files, grouped by quant.
pub async fn repo_files(ctx: &AppCtx, repo: &str) -> Result<RepoFiles, NexusError> {
    check_network(ctx)?;
    validate_repo(repo)?;
    let rev = ctx.hf.resolve_revision(repo, "main").await.map_err(hf_err)?;
    let tree = ctx.hf.tree(repo, &rev.sha).await.map_err(hf_err)?;
    Ok(RepoFiles {
        repo: repo.to_owned(),
        sha: rev.sha,
        license: rev.license,
        gated: rev.gated,
        files: hf::group_gguf(&tree),
    })
}

fn validate_repo(repo: &str) -> Result<(), NexusError> {
    let ok = repo.split('/').count() == 2
        && repo
            .split('/')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)));
    if ok {
        Ok(())
    } else {
        Err(NexusError::InvalidInput(
            "a Hugging Face repo looks like `owner/name`".into(),
        ))
    }
}

fn check_network(ctx: &AppCtx) -> Result<(), NexusError> {
    if ctx.settings_snapshot().privacy_allow_network_downloads {
        Ok(())
    } else {
        Err(NexusError::PermissionDenied(
            "downloads are turned off in Settings → Privacy".into(),
        ))
    }
}

// ---- starting downloads -----------------------------------------------------------------------

pub async fn start_catalog(ctx: &Ctx, catalog_id: &str) -> Result<DownloadView, NexusError> {
    let entry = Catalog::embedded()
        .get(catalog_id)
        .ok_or_else(|| NexusError::NotFound("catalog model".into()))?
        .clone();
    start(ctx, &entry.repo, &entry.revision, Pick::Catalog(&entry)).await
}

pub async fn start_hf(ctx: &Ctx, repo: &str, file: &str) -> Result<DownloadView, NexusError> {
    validate_repo(repo)?;
    start(ctx, repo, "main", Pick::File(file)).await
}

enum Pick<'a> {
    Catalog(&'a CatalogEntry),
    File(&'a str),
}

async fn start(ctx: &Ctx, repo: &str, revision: &str, pick: Pick<'_>) -> Result<DownloadView, NexusError> {
    check_network(ctx)?;
    let rev = ctx.hf.resolve_revision(repo, revision).await.map_err(hf_err)?;
    let tree = ctx.hf.tree(repo, &rev.sha).await.map_err(hf_err)?;
    let (file, catalog_id) = match &pick {
        Pick::Catalog(e) => (
            e.pick_file(tree.iter().map(|t| t.path.as_str()))
                .ok_or_else(|| NexusError::NotFound(format!("a `{}` file in {repo}", e.quant)))?,
            Some(e.id.clone()),
        ),
        Pick::File(f) => ((*f).to_owned(), None),
    };
    let grouped = hf::group_gguf(&tree);
    if grouped.iter().any(|g| g.name == file && g.parts.len() > 1) {
        return Err(NexusError::InvalidInput(
            "this model is split into several files, which is not supported yet".into(),
        ));
    }
    let tf = tree
        .iter()
        .find(|t| t.path == file)
        .ok_or_else(|| NexusError::NotFound(format!("{file} in {repo}")))?;
    let sha256 = tf.sha256.clone().ok_or_else(|| {
        NexusError::InvalidInput(
            "Hugging Face gave no checksum for this file, so it cannot be verified".into(),
        )
    })?;
    let stem = file.trim_end_matches(".gguf");
    let id = format!("{}__{}", slug(&repo.replace('/', "__")), slug(stem));
    if let Some(m) = ctx.store.get_model(&id)?
        && Path::new(&m.path).is_file()
    {
        return Err(NexusError::Conflict("this model is already installed".into()));
    }
    if let Some(existing) = ctx.store.get_download(&id)? {
        if matches!(existing.status.as_str(), "queued" | "running") {
            return Ok(view(ctx, &existing));
        }
        ctx.store.delete_download(&id)?;
    }
    let dest = ctx.paths.models.join(repo.replace('/', "__")).join(&file);
    let now = now_ms();
    let row = DownloadRow {
        id: id.clone(),
        catalog_id,
        repo: repo.to_owned(),
        revision: rev.sha,
        file,
        dest: dest.display().to_string(),
        sha256,
        bytes: tf.size,
        done_bytes: download::part_bytes(&dest),
        status: "queued".into(),
        error: None,
        created_at: now,
        updated_at: now,
    };
    ctx.store.insert_download(&row)?;
    emit_download(ctx, &id);
    pump(ctx);
    Ok(view(ctx, &ctx.store.get_download(&id)?.unwrap_or(row)))
}

// ---- running the queue ------------------------------------------------------------------------

/// Start the next queued download if none is running.
pub fn pump(ctx: &Ctx) {
    let Ok(mut guard) = ctx.downloads.running.lock() else {
        return;
    };
    if guard.is_some() {
        return;
    }
    let Ok(Some(row)) = ctx.store.next_queued_download() else {
        return;
    };
    let cancel = CancellationToken::new();
    let intent = Arc::new(AtomicU8::new(INTENT_NONE));
    *guard = Some(Running {
        id: row.id.clone(),
        cancel: cancel.clone(),
        intent: intent.clone(),
    });
    drop(guard);
    let _ = ctx.store.set_download_status(&row.id, "running", None);
    emit_download(ctx, &row.id);
    let ctx = Arc::clone(ctx);
    tokio::spawn(async move {
        run_download(&ctx, row, cancel, intent).await;
        if let Ok(mut g) = ctx.downloads.running.lock() {
            *g = None;
        }
        pump(&ctx);
    });
}

async fn run_download(ctx: &Ctx, row: DownloadRow, cancel: CancellationToken, intent: Arc<AtomicU8>) {
    let spec = DownloadSpec {
        url: ctx.hf.resolve_url(&row.repo, &row.revision, &row.file),
        token: ctx.hf.token().map(str::to_owned),
        expected_sha256: row.sha256.clone(),
        expected_bytes: row.bytes,
        dest: PathBuf::from(&row.dest),
        revision: row.revision.clone(),
    };
    let last_db = Mutex::new(Instant::now() - Duration::from_secs(10));
    let progress = {
        let ctx = Arc::clone(ctx);
        let id = row.id.clone();
        move |p: DownloadProgress| {
            if let Ok(mut l) = ctx.downloads.live.lock() {
                l.insert(
                    id.clone(),
                    Live {
                        bytes_per_sec: p.bytes_per_sec,
                        eta_secs: p.eta_secs,
                        phase: p.phase,
                    },
                );
            }
            if let Ok(mut t) = last_db.lock()
                && t.elapsed() >= Duration::from_secs(2)
            {
                let _ = ctx.store.set_download_progress(&id, p.bytes);
                *t = Instant::now();
            }
            let _ = ctx.bus.send(AppEvent::DownloadProgress(DownloadView {
                id: id.clone(),
                catalog_id: None,
                repo: String::new(),
                file: String::new(),
                bytes: p.total,
                done_bytes: p.bytes,
                status: "running".into(),
                error: None,
                bytes_per_sec: p.bytes_per_sec,
                eta_secs: p.eta_secs,
                phase: Some(p.phase),
            }));
        }
    };
    let downloader = match Downloader::new() {
        Ok(d) => d,
        Err(e) => return finish_failed(ctx, &row, &e),
    };
    let result = downloader.download(&spec, &progress, &cancel).await;
    if let Ok(mut l) = ctx.downloads.live.lock() {
        l.remove(&row.id);
    }
    match result {
        Ok(done) => {
            let _ = ctx.store.set_download_progress(&row.id, done.bytes);
            match install(ctx, &row, &done.sha256) {
                Ok(name) => {
                    let _ = ctx.store.set_download_status(&row.id, "done", None);
                    notify(
                        ctx,
                        NotificationLevel::Success,
                        format!("{name} is ready"),
                        Some("Downloaded and verified.".into()),
                        Some(NotificationAction {
                            label: "Use it".into(),
                            route: format!("/models?focus={}", row.id),
                        }),
                    );
                }
                Err(e) => {
                    let _ = ctx
                        .store
                        .set_download_status(&row.id, "failed", Some(&e.to_string()));
                    notify(
                        ctx,
                        NotificationLevel::Error,
                        "Could not install the model".into(),
                        Some(e.to_string()),
                        None,
                    );
                }
            }
        }
        Err(DownloadError::Cancelled) => {
            if intent.load(Ordering::SeqCst) == INTENT_CANCEL {
                download::discard_part(Path::new(&row.dest));
                let _ = ctx.store.set_download_status(&row.id, "cancelled", None);
            } else {
                let _ = ctx
                    .store
                    .set_download_progress(&row.id, download::part_bytes(Path::new(&row.dest)));
                let _ = ctx.store.set_download_status(&row.id, "paused", None);
            }
        }
        Err(e) => finish_failed(ctx, &row, &e),
    }
    emit_download(ctx, &row.id);
}

fn finish_failed(ctx: &AppCtx, row: &DownloadRow, e: &DownloadError) {
    let message = match e {
        DownloadError::ChecksumMismatch => {
            "The file did not match its checksum and was removed. Try again.".to_owned()
        }
        other => other.to_string(),
    };
    let _ = ctx.store.set_download_status(&row.id, "failed", Some(&message));
    notify(
        ctx,
        NotificationLevel::Error,
        format!("Download failed: {}", row.file),
        Some(message),
        Some(NotificationAction {
            label: "Open Models".into(),
            route: "/models".into(),
        }),
    );
}

/// Record a finished download as an installed model. Returns its display name.
fn install(ctx: &AppCtx, row: &DownloadRow, sha256: &str) -> Result<String, NexusError> {
    let path = Path::new(&row.dest);
    let meta = gguf::parse_file(path).map_err(|e| NexusError::InvalidInput(e.to_string()))?;
    let catalog = row
        .catalog_id
        .as_deref()
        .and_then(|id| Catalog::embedded().get(id));
    let name = catalog
        .map(|c| c.name.clone())
        .or_else(|| meta.name.clone())
        .unwrap_or_else(|| row.file.trim_end_matches(".gguf").to_owned());
    let extra = catalog
        .map(|c| serde_json::json!({"sampling": c.sampling, "capabilities": c.capabilities}).to_string());
    ctx.store.upsert_model(&ModelRow {
        id: row.id.clone(),
        source: "download".into(),
        name: name.clone(),
        repo: Some(row.repo.clone()),
        revision: Some(row.revision.clone()),
        file: Some(row.file.clone()),
        path: row.dest.clone(),
        sha256: Some(sha256.to_owned()),
        bytes: row.bytes,
        quant: meta.quant.clone().or_else(|| catalog.map(|c| c.quant.clone())),
        catalog_id: row.catalog_id.clone(),
        gguf_meta: serde_json::to_string(&meta).ok(),
        license: catalog.map(|c| c.license.clone()),
        extra,
        added_at: now_ms(),
    })?;
    Ok(name)
}

// ---- controls ---------------------------------------------------------------------------------

fn desktop_only(caller: &Caller) -> Result<(), NexusError> {
    if caller.is_desktop() {
        Ok(())
    } else {
        Err(NexusError::ForbiddenRemote)
    }
}

fn signal_running(ctx: &AppCtx, id: &str, intent: u8) -> bool {
    let guard = ctx.downloads.running.lock().ok();
    match guard.as_deref().and_then(Option::as_ref) {
        Some(r) if r.id == id => {
            r.intent.store(intent, Ordering::SeqCst);
            r.cancel.cancel();
            true
        }
        _ => false,
    }
}

/// Pause keeps the part file so Resume continues.
pub fn pause(ctx: &AppCtx, caller: &Caller, id: &str) -> Result<(), NexusError> {
    desktop_only(caller)?;
    if signal_running(ctx, id, INTENT_PAUSE) {
        return Ok(());
    }
    let d = ctx
        .store
        .get_download(id)?
        .ok_or_else(|| NexusError::NotFound("download".into()))?;
    if d.status == "queued" {
        ctx.store.set_download_status(id, "paused", None)?;
        emit_download(ctx, id);
    }
    Ok(())
}

pub fn resume(ctx: &Ctx, caller: &Caller, id: &str) -> Result<(), NexusError> {
    desktop_only(caller)?;
    check_network(ctx)?;
    let d = ctx
        .store
        .get_download(id)?
        .ok_or_else(|| NexusError::NotFound("download".into()))?;
    if matches!(d.status.as_str(), "paused" | "failed" | "cancelled") {
        ctx.store.set_download_status(id, "queued", None)?;
        emit_download(ctx, id);
        pump(ctx);
    }
    Ok(())
}

/// Cancel deletes the part file.
pub fn cancel(ctx: &AppCtx, caller: &Caller, id: &str) -> Result<(), NexusError> {
    desktop_only(caller)?;
    if signal_running(ctx, id, INTENT_CANCEL) {
        return Ok(());
    }
    let d = ctx
        .store
        .get_download(id)?
        .ok_or_else(|| NexusError::NotFound("download".into()))?;
    download::discard_part(Path::new(&d.dest));
    ctx.store.set_download_status(id, "cancelled", None)?;
    emit_download(ctx, id);
    Ok(())
}

// ---- activation, deletion, import ---------------------------------------------------------------

/// Make `id` the model the runtime uses, and restart it.
pub async fn activate(ctx: &Ctx, caller: &Caller, id: &str) -> Result<ActivateResult, NexusError> {
    desktop_only(caller)?;
    let row = ctx
        .store
        .get_model(id)?
        .ok_or_else(|| NexusError::NotFound("model".into()))?;
    if !Path::new(&row.path).is_file() {
        return Err(NexusError::NotFound(format!(
            "The model file is missing: {}",
            row.path
        )));
    }
    super::settings::set(
        ctx,
        caller,
        "models.active_model_id",
        SettingValue::Text(id.to_owned()),
    )?;
    let meta: Option<GgufMeta> = row
        .gguf_meta
        .as_deref()
        .and_then(|t| serde_json::from_str(t).ok());
    let warning = meta
        .as_ref()
        .is_some_and(|m| !m.has_chat_template)
        .then(|| "This model has no chat template; replies may be poor.".to_owned());
    let _ = ctx.bus.send(AppEvent::ModelChanged(id.to_owned()));
    super::runtime::restart(ctx, caller).await?;
    Ok(ActivateResult {
        id: id.to_owned(),
        warning,
    })
}

/// Delete a downloaded or copied model, or unlink a referenced one. The bundle is protected.
pub fn delete(ctx: &AppCtx, caller: &Caller, id: &str) -> Result<(), NexusError> {
    desktop_only(caller)?;
    let row = ctx
        .store
        .get_model(id)?
        .ok_or_else(|| NexusError::NotFound("model".into()))?;
    match removal(&row.source) {
        Removal::Protected => {
            return Err(NexusError::InvalidInput(
                "the built-in model is part of the app and cannot be deleted".into(),
            ));
        }
        _ if active_model_id(ctx).as_deref() == Some(id) => {
            return Err(NexusError::Conflict(
                "this model is in use; switch to another model first".into(),
            ));
        }
        Removal::Delete => {
            let p = Path::new(&row.path);
            let _ = std::fs::remove_file(p);
            if let Some(dir) = p.parent()
                && dir.starts_with(&ctx.paths.models)
                && dir != ctx.paths.models
            {
                let _ = std::fs::remove_dir(dir); // only succeeds when empty
            }
        }
        Removal::Unlink => {}
    }
    ctx.store.delete_model(id)?;
    let _ = ctx.store.delete_download(id);
    Ok(())
}

/// Add a local `.gguf`: copied into the app's folder, or referenced in place (needs a Read grant).
pub async fn import_local(
    ctx: &Ctx,
    caller: &Caller,
    path: &str,
    copy: bool,
) -> Result<InstalledModel, NexusError> {
    desktop_only(caller)?;
    let src = PathBuf::from(path);
    let ctx2 = Arc::clone(ctx);
    let id = tokio::task::spawn_blocking(move || -> Result<String, NexusError> {
        let meta = gguf::parse_file(&src)
            .map_err(|e| NexusError::InvalidInput(format!("{}: {e}", src.display())))?;
        let stem = src
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "model".into());
        let (final_path, sha) = if copy {
            let dir = ctx2.paths.models.join("local");
            std::fs::create_dir_all(&dir)?;
            let mut dest = dir.join(src.file_name().unwrap_or_default());
            let mut n = 2;
            while dest.exists() {
                dest = dir.join(format!("{stem}-{n}.gguf"));
                n += 1;
            }
            std::fs::copy(&src, &dest)?;
            let sha = bundled::hash_file(&dest, &CancellationToken::new(), |_| {})
                .map_err(|e| NexusError::Internal(e.to_string()))?;
            (dest, Some(sha))
        } else {
            let canon = ctx2.perms.check("user", &src, Access::Read)?;
            (canon.into_path_buf(), None)
        };
        let bytes = std::fs::metadata(&final_path)?.len();
        let id = format!(
            "import-{}",
            bundled::discovered_id(&final_path).trim_start_matches("local-")
        );
        ctx2.store.upsert_model(&ModelRow {
            id: id.clone(),
            source: "import".into(),
            name: meta.name.clone().unwrap_or(stem),
            repo: None,
            revision: None,
            file: final_path.file_name().map(|n| n.to_string_lossy().into_owned()),
            path: final_path.display().to_string(),
            sha256: sha,
            bytes,
            quant: meta.quant.clone(),
            catalog_id: None,
            gguf_meta: serde_json::to_string(&meta).ok(),
            license: meta.license.clone(),
            extra: None,
            added_at: now_ms(),
        })?;
        Ok(id)
    })
    .await
    .map_err(|e| NexusError::Internal(e.to_string()))??;
    list_installed(ctx)?
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| NexusError::NotFound("model".into()))
}

// ---- startup, folders, verify -----------------------------------------------------------------

/// Register the bundle, scan extra folders, and pause downloads that were running at shutdown.
pub fn init(ctx: &AppCtx) {
    match bundled::register_bundled(&ctx.store, &ctx.resource_dir.join("models")) {
        Ok(r) => {
            if !r.missing.is_empty() {
                tracing::warn!(missing = ?r.missing, "bundled models are missing on disk");
            }
        }
        Err(e) => tracing::warn!("could not register bundled models: {e}"),
    }
    if let Err(e) = rescan(ctx) {
        tracing::warn!("could not scan extra model folders: {e}");
    }
    match ctx.store.pause_interrupted_downloads() {
        Ok(n) if n > 0 => tracing::info!(n, "paused downloads interrupted by the last shutdown"),
        Ok(_) => {}
        Err(e) => tracing::warn!("could not pause interrupted downloads: {e}"),
    }
}

/// Re-read the user's extra model folders. Returns how many models they hold.
pub fn rescan(ctx: &AppCtx) -> Result<usize, NexusError> {
    let dirs =
        crate::runtime_paths::extra_model_dirs(&ctx.exe_dir, &ctx.settings_snapshot().models_extra_dirs);
    bundled::sync_extra_folders(&ctx.store, &dirs).map_err(|e| NexusError::Internal(e.to_string()))
}

/// Re-hash the bundled models. Heavy: run off the UI thread.
pub async fn verify_bundled(ctx: &Ctx, caller: &Caller) -> Result<Vec<VerifyResult>, NexusError> {
    desktop_only(caller)?;
    let ctx = Arc::clone(ctx);
    tokio::task::spawn_blocking(move || {
        bundled::verify_bundled(&ctx.store, &CancellationToken::new(), |_, _, _| {})
            .map_err(|e| NexusError::Internal(e.to_string()))
    })
    .await
    .map_err(|e| NexusError::Internal(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use nexus_core::error::ErrorCode;
    use nexus_models::gguf::fixture;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use wiremock::matchers::{method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn model_bytes() -> Vec<u8> {
        let mut v = fixture::sample();
        v.resize(400_000, 5);
        v
    }

    fn sha(d: &[u8]) -> String {
        Sha256::digest(d).iter().map(|b| format!("{b:02x}")).collect()
    }

    async fn hub(data: &[u8], file: &str) -> MockServer {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/models/[^/]+/[^/]+/revision/main$"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"sha": "c0ffee", "cardData": {"license": "mit"}})),
            )
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/models/[^/]+/[^/]+/tree/c0ffee$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"type": "file", "path": file, "size": 100, "lfs": {"oid": sha(data), "size": data.len()}}])))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/o/r/resolve/c0ffee/{file}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(data.to_vec()))
            .mount(&s)
            .await;
        s
    }

    async fn wait_status(ctx: &Ctx, id: &str, want: &str) {
        for _ in 0..200 {
            if ctx
                .store
                .get_download(id)
                .unwrap()
                .is_some_and(|d| d.status == want)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!(
            "download never reached {want}: {:?}",
            ctx.store.get_download(id).unwrap()
        );
    }

    #[tokio::test]
    async fn a_hugging_face_download_runs_installs_and_notifies() {
        let data = model_bytes();
        let server = hub(&data, "tiny-Q4_K_M.gguf").await;
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests_full(dir.path(), None, Some(server.uri()));
        let mut rx = ctx.bus.subscribe();
        let v = start_hf(&ctx, "o/r", "tiny-Q4_K_M.gguf").await.unwrap();
        assert_eq!(v.id, "o__r__tiny-q4_k_m");
        wait_status(&ctx, &v.id, "done").await;
        let m = ctx.store.get_model(&v.id).unwrap().unwrap();
        assert_eq!(
            (m.source.as_str(), m.quant.as_deref(), m.sha256.as_deref()),
            ("download", Some("Q4_K_M"), Some(sha(&data).as_str()))
        );
        assert!(Path::new(&m.path).is_file());
        let mut saw_ready = false;
        while let Ok(e) = rx.try_recv() {
            if let AppEvent::Notify(n) = e {
                saw_ready |=
                    n.title.ends_with("is ready") && n.action.is_some_and(|a| a.route.starts_with("/models"));
            }
        }
        assert!(saw_ready, "a 'ready' notification with a Use-it action");
        let listed = list_installed(&ctx).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].removal, Removal::Delete);
        // Starting it again is a conflict.
        assert!(matches!(
            start_hf(&ctx, "o/r", "tiny-Q4_K_M.gguf").await,
            Err(NexusError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn a_bad_checksum_fails_and_leaves_nothing_behind() {
        let data = model_bytes();
        let server = hub(&data, "bad-Q8_0.gguf").await;
        // Serve different bytes than the hash promises.
        server.reset().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/models/[^/]+/[^/]+/revision/main$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"sha": "c0ffee"})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/models/[^/]+/[^/]+/tree/c0ffee$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"type": "file", "path": "bad-Q8_0.gguf", "size": 1, "lfs": {"oid": sha(&data), "size": data.len()}}])))
            .mount(&server)
            .await;
        let mut other = data.clone();
        other[200_000] ^= 1;
        Mock::given(method("GET"))
            .and(path("/o/r/resolve/c0ffee/bad-Q8_0.gguf"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(other))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests_full(dir.path(), None, Some(server.uri()));
        let v = start_hf(&ctx, "o/r", "bad-Q8_0.gguf").await.unwrap();
        wait_status(&ctx, &v.id, "failed").await;
        let d = ctx.store.get_download(&v.id).unwrap().unwrap();
        assert!(d.error.unwrap().contains("checksum"));
        assert!(ctx.store.get_model(&v.id).unwrap().is_none());
        assert_eq!(download::part_bytes(Path::new(&d.dest)), 0);
    }

    #[tokio::test]
    async fn downloads_respect_the_privacy_switch_and_validate_input() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        super::super::settings::set(
            &ctx,
            &Caller::Desktop,
            "privacy.allow_network_downloads",
            SettingValue::Bool(false),
        )
        .unwrap();
        assert_eq!(
            start_catalog(&ctx, "minicpm5-1b-q4").await.unwrap_err().code(),
            ErrorCode::PermissionDenied
        );
        assert_eq!(
            search(&ctx, "minicpm").await.unwrap_err().code(),
            ErrorCode::PermissionDenied
        );
        assert_eq!(
            start_hf(&ctx, "not a repo", "f.gguf").await.unwrap_err().code(),
            ErrorCode::InvalidInput
        );
        assert_eq!(
            start_catalog(&ctx, "nope").await.unwrap_err().code(),
            ErrorCode::NotFound
        );
        assert!(search(&ctx, "m").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn offline_search_and_catalog_still_work() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path()); // hub = a closed port
        assert_eq!(
            search(&ctx, "minicpm").await.unwrap_err().code(),
            ErrorCode::Network
        );
        let cards = catalog(&ctx).unwrap();
        assert_eq!(cards.len(), 12);
        assert!(
            cards
                .iter()
                .all(|c| c.installed_id.is_none() && c.download.is_none())
        );
    }

    #[tokio::test]
    async fn pause_resume_and_cancel_manage_the_part_file() {
        let data = model_bytes();
        let server = hub(&data, "p-Q4_K_M.gguf").await;
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests_full(dir.path(), None, Some(server.uri()));
        // Queue a download without running the pump by inserting the row directly as paused.
        let dest = ctx.paths.models.join("o__r").join("p-Q4_K_M.gguf");
        ctx.store
            .insert_download(&DownloadRow {
                id: "o__r__p-q4_k_m".into(),
                catalog_id: None,
                repo: "o/r".into(),
                revision: "c0ffee".into(),
                file: "p-Q4_K_M.gguf".into(),
                dest: dest.display().to_string(),
                sha256: sha(&data),
                bytes: data.len() as u64,
                done_bytes: 0,
                status: "paused".into(),
                error: None,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(download::part_path(&dest), b"partial").unwrap();
        cancel(&ctx, &Caller::Desktop, "o__r__p-q4_k_m").unwrap();
        assert_eq!(
            ctx.store.get_download("o__r__p-q4_k_m").unwrap().unwrap().status,
            "cancelled"
        );
        assert!(!download::part_path(&dest).exists());
        resume(&ctx, &Caller::Desktop, "o__r__p-q4_k_m").unwrap();
        wait_status(&ctx, "o__r__p-q4_k_m", "done").await;
        assert!(dest.is_file());
        let remote = Caller::Remote {
            device_id: "d".into(),
            policy: nexus_core::settings::SharePolicy::Full,
        };
        assert_eq!(
            pause(&ctx, &remote, "x").unwrap_err().code(),
            ErrorCode::ForbiddenRemote
        );
    }

    #[tokio::test]
    async fn the_bundled_model_is_protected_and_the_active_one_cannot_be_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        let models = ctx.resource_dir.join("models");
        std::fs::create_dir_all(models.join("b")).unwrap();
        let data = model_bytes();
        std::fs::write(models.join("b/b.gguf"), &data).unwrap();
        std::fs::write(
            models.join("manifest.json"),
            json!({"schema": 1, "profile": "t", "built_at": "x", "default": "b", "models": [{"id": "b", "name": "Bundled", "path": "b/b.gguf", "sha256": sha(&data), "bytes": data.len()}]}).to_string(),
        )
        .unwrap();
        init(&ctx);
        let list = list_installed(&ctx).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].active && list[0].removal == Removal::Protected && list[0].file_exists);
        assert_eq!(
            delete(&ctx, &Caller::Desktop, "b").unwrap_err().code(),
            ErrorCode::InvalidInput
        );

        // A downloaded model becomes active, then cannot be deleted until another is chosen.
        let dl_dir = ctx.paths.models.join("o__r");
        std::fs::create_dir_all(&dl_dir).unwrap();
        std::fs::write(dl_dir.join("d.gguf"), &data).unwrap();
        ctx.store
            .upsert_model(&ModelRow {
                id: "d".into(),
                source: "download".into(),
                name: "D".into(),
                repo: None,
                revision: None,
                file: None,
                path: dl_dir.join("d.gguf").display().to_string(),
                sha256: None,
                bytes: 1,
                quant: None,
                catalog_id: None,
                gguf_meta: None,
                license: None,
                extra: None,
                added_at: 5,
            })
            .unwrap();
        super::super::settings::set(
            &ctx,
            &Caller::Desktop,
            "models.active_model_id",
            SettingValue::Text("d".into()),
        )
        .unwrap();
        assert_eq!(
            delete(&ctx, &Caller::Desktop, "d").unwrap_err().code(),
            ErrorCode::Conflict
        );
        super::super::settings::set(
            &ctx,
            &Caller::Desktop,
            "models.active_model_id",
            SettingValue::Text("builtin".into()),
        )
        .unwrap();
        delete(&ctx, &Caller::Desktop, "d").unwrap();
        assert!(
            !dl_dir.join("d.gguf").exists() && !dl_dir.exists(),
            "file and its empty folder are removed"
        );
        assert!(list_installed(&ctx).unwrap().len() == 1);
    }

    #[tokio::test]
    async fn activating_a_missing_file_fails_gracefully() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        assert_eq!(
            activate(&ctx, &Caller::Desktop, "nope").await.unwrap_err().code(),
            ErrorCode::NotFound
        );
        ctx.store
            .upsert_model(&ModelRow {
                id: "gone".into(),
                source: "import".into(),
                name: "Gone".into(),
                repo: None,
                revision: None,
                file: None,
                path: dir.path().join("gone.gguf").display().to_string(),
                sha256: None,
                bytes: 1,
                quant: None,
                catalog_id: None,
                gguf_meta: None,
                license: None,
                extra: None,
                added_at: 1,
            })
            .unwrap();
        assert_eq!(
            activate(&ctx, &Caller::Desktop, "gone").await.unwrap_err().code(),
            ErrorCode::NotFound
        );
        assert_eq!(
            ctx.settings_snapshot().models_active_model_id,
            "builtin",
            "a failed activation changes nothing"
        );
    }

    #[tokio::test]
    async fn local_files_are_imported_by_copy_and_extra_folders_are_scanned() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        let src = dir.path().join("downloads/my-model.gguf");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, model_bytes()).unwrap();
        let m = import_local(&ctx, &Caller::Desktop, src.to_str().unwrap(), true)
            .await
            .unwrap();
        assert_eq!((m.source.as_str(), m.removal), ("import", Removal::Delete));
        assert!(Path::new(&m.path).starts_with(&ctx.paths.models) && src.exists());
        std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
        assert!(
            import_local(
                &ctx,
                &Caller::Desktop,
                dir.path().join("notes.txt").to_str().unwrap(),
                true
            )
            .await
            .is_err()
        );
        // Referencing in place needs a grant.
        assert_eq!(
            import_local(&ctx, &Caller::Desktop, src.to_str().unwrap(), false)
                .await
                .unwrap_err()
                .code(),
            ErrorCode::PermissionDenied
        );

        let nas = dir.path().join("nas");
        std::fs::create_dir_all(&nas).unwrap();
        std::fs::write(nas.join("shared.gguf"), model_bytes()).unwrap();
        super::super::settings::set(
            &ctx,
            &Caller::Desktop,
            "models.extra_dirs",
            SettingValue::List(vec![nas.display().to_string()]),
        )
        .unwrap();
        assert_eq!(rescan(&ctx).unwrap(), 1);
        let found = list_installed(&ctx)
            .unwrap()
            .into_iter()
            .find(|m| m.source == "discovered")
            .unwrap();
        assert_eq!(found.removal, Removal::Unlink);
        delete(&ctx, &Caller::Desktop, &found.id).unwrap();
        assert!(
            nas.join("shared.gguf").exists(),
            "unlinking never deletes the user's file"
        );
    }
}
