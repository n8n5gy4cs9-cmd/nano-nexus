//! USB Studio model operations. All filesystem work uses the validated shared root.

use super::Caller;
use crate::state::{AppCtx, Ctx};
use nexus_core::error::NexusError;
use nexus_models::{
    download::{DownloadProgress, DownloadSpec, Downloader},
    studio,
};
use nexus_store::{
    ModelRow,
    studio::{AssetRole, ModelKind, StudioModel},
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tokio_util::sync::CancellationToken;

/// Explicit model/companion identity shared by imports and downloads.
#[derive(Clone, Debug, Deserialize, Type)]
pub struct ModelInput {
    pub id: String,
    pub name: String,
    pub kind: ModelKind,
    pub role: String,
    pub variant: String,
}

/// Registry view reports missing media and selection without pretending an engine is running.
#[derive(Clone, Debug, Serialize, Type)]
pub struct ModelView {
    pub model: StudioModel,
    pub available: bool,
    pub selected: bool,
}

struct Transfer {
    model_id: String,
    cancel: CancellationToken,
    discard: bool,
}
fn transfers() -> &'static Mutex<HashMap<String, Transfer>> {
    static JOBS: OnceLock<Mutex<HashMap<String, Transfer>>> = OnceLock::new();
    JOBS.get_or_init(Default::default)
}
struct TransferGuard(String, Arc<super::portable_store::Lease>);
impl Drop for TransferGuard {
    fn drop(&mut self) {
        if let Ok(mut jobs) = transfers().lock() {
            if let Some(job) = jobs.remove(&self.0) {
                job.cancel.cancel();
            }
        }
    }
}
fn begin(id: &str, model_id: &str) -> Result<(TransferGuard, CancellationToken), NexusError> {
    if id.is_empty() || id.len() > 256 {
        return Err(invalid("invalid transfer id"));
    }
    let lease = super::portable_store::acquire()?;
    let mut jobs = transfers()
        .lock()
        .map_err(|_| invalid("transfer lock poisoned"))?;
    if jobs.len() >= 8 || jobs.contains_key(id) || jobs.values().any(|job| job.model_id == model_id) {
        return Err(NexusError::Conflict(
            "transfer already running or queue full".into(),
        ));
    }
    let token = CancellationToken::new();
    jobs.insert(
        id.into(),
        Transfer {
            model_id: model_id.into(),
            cancel: token.clone(),
            discard: false,
        },
    );
    Ok((TransferGuard(id.into(), lease), token))
}
fn root(ctx: &AppCtx, caller: &Caller) -> Result<PathBuf, NexusError> {
    if !caller.is_desktop() {
        return Err(NexusError::ForbiddenRemote);
    }
    ctx.paths
        .portable_root
        .clone()
        .ok_or_else(|| invalid("Studio shared storage is available in the USB distribution"))
}
fn validate_input(input: &ModelInput) -> Result<(), NexusError> {
    let valid = |s: &str| {
        !s.is_empty()
            && s.len() <= 160
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if !valid(&input.id)
        || !valid(&input.variant)
        || !valid(&input.role)
        || input.name.trim().is_empty()
        || input.name.len() > 512
    {
        return Err(invalid("enter a model ID, name, kind, variant and asset role"));
    }
    Ok(())
}

/// List shared models; unavailable assets stay visible so users can repair them.
pub fn list(ctx: &AppCtx, caller: &Caller) -> Result<Vec<ModelView>, NexusError> {
    let root = root(ctx, caller)?;
    let settings = ctx.store.list_studio_selections()?;
    Ok(ctx
        .store
        .studio_models()?
        .into_iter()
        .map(|model| {
            let available = model
                .assets
                .iter()
                .all(|r| studio::validate_asset(&root, &r.asset).is_ok());
            let selected = settings
                .iter()
                .any(|(kind, id)| kind == model.kind.as_str() && id == &model.id);
            ModelView {
                model,
                available,
                selected,
            }
        })
        .collect())
}

fn publish(
    ctx: &AppCtx,
    root: &Path,
    input: &ModelInput,
    asset: nexus_store::studio::StudioAsset,
    provenance: serde_json::Value,
) -> Result<StudioModel, NexusError> {
    let mut model = ctx
        .store
        .studio_models()?
        .into_iter()
        .find(|m| m.id == input.id)
        .unwrap_or_else(|| StudioModel {
            id: input.id.clone(),
            name: input.name.clone(),
            kind: input.kind,
            variant: input.variant.clone(),
            metadata: "{}".into(),
            assets: Vec::new(),
        });
    if model.kind != input.kind || model.variant != input.variant {
        return Err(invalid(
            "existing model uses a different kind or variant; choose another ID or reclassify it explicitly",
        ));
    }
    model.name = input.name.clone();
    model.assets.retain(|r| r.role != input.role);
    model.assets.push(AssetRole {
        role: input.role.clone(),
        asset,
    });
    let mut metadata: serde_json::Value =
        serde_json::from_str(&model.metadata).unwrap_or_else(|_| serde_json::json!({}));
    let object = metadata
        .as_object_mut()
        .ok_or_else(|| invalid("stored model provenance is not an object"))?;
    object.insert(input.role.clone(), provenance);
    model.metadata = metadata.to_string();
    studio::register(&ctx.store, root, &model)?;
    project_chat(ctx, root, &model)?;
    Ok(model)
}

/// Publish chat models to the existing chat API without copying their weights.
pub fn project_chat(ctx: &AppCtx, root: &Path, model: &StudioModel) -> Result<(), NexusError> {
    if model.kind != ModelKind::Chat || model.variant != "universal" {
        return Ok(());
    }
    let Some(weights) = model.assets.iter().find(|a| a.role == "weights") else {
        return Ok(());
    };
    let path = studio::validate_asset(root, &weights.asset)?;
    let meta = nexus_models::gguf::parse_file(&path).map_err(|e| invalid(&e.to_string()))?;
    let old = ctx.store.get_model(&projected_id(model))?;
    let mut extra = old
        .as_ref()
        .and_then(|m| m.extra.as_deref())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    extra["storage"] = serde_json::json!("studio");
    extra["studio_id"] = serde_json::json!(model.id);
    ctx.store.upsert_model(&ModelRow {
        id: projected_id(model),
        source: old
            .as_ref()
            .map(|m| m.source.clone())
            .unwrap_or_else(|| "import".into()),
        name: model.name.clone(),
        repo: old.as_ref().and_then(|m| m.repo.clone()),
        revision: old.as_ref().and_then(|m| m.revision.clone()),
        file: path.file_name().map(|s| s.to_string_lossy().into_owned()),
        path: path.display().to_string(),
        sha256: Some(weights.asset.sha256.clone()),
        bytes: weights.asset.bytes,
        quant: meta.quant.clone(),
        catalog_id: old.as_ref().and_then(|m| m.catalog_id.clone()),
        gguf_meta: serde_json::to_string(&meta).ok(),
        license: meta.license.clone(),
        extra: Some(extra.to_string()),
        added_at: old.as_ref().map(|m| m.added_at).unwrap_or(0),
    })?;
    Ok(())
}

/// Import an explicit user-selected source; no external model references in portable mode.
pub async fn import(
    ctx: &Ctx,
    caller: &Caller,
    job_id: &str,
    path: &str,
    input: ModelInput,
    progress: Arc<dyn Fn(DownloadProgress) + Send + Sync>,
) -> Result<StudioModel, NexusError> {
    let root = root(ctx, caller)?;
    validate_input(&input)?;
    idle_model(ctx, &input.id)?;
    let (_guard, cancel) = begin(job_id, &input.id)?;
    let lease = _guard.1.clone();
    let source = PathBuf::from(path);
    // Native picker/drag-drop is user intent; still enforce built-in and user deny rules.
    let source = super::models::reference_read(ctx, &source).await?;
    let ctx = ctx.clone();
    tokio::task::spawn_blocking(move || {
        let _lease = lease;
        studio::validate_format(&source,input.kind,&input.role)?;
        let total=std::fs::metadata(&source)?.len();
        let asset=studio::import_asset(&ctx.store,&root,&source,&cancel,|bytes| progress(DownloadProgress { bytes,total,bytes_per_sec:0.0,eta_secs:None,phase:nexus_models::download::Phase::Verifying }))?;
        publish(&ctx,&root,&input,asset,serde_json::json!({"source":"import","file":source.file_name().map(|n|n.to_string_lossy().into_owned())}))
    }).await.map_err(|e| NexusError::Internal(e.to_string()))?
}

/// Resolve HF revision/digest first, then resume into one USB-local content address.
pub async fn download(
    ctx: &Ctx,
    caller: &Caller,
    job_id: &str,
    url: &str,
    input: ModelInput,
    progress: Arc<dyn Fn(DownloadProgress) + Send + Sync>,
) -> Result<StudioModel, NexusError> {
    let root = root(ctx, caller)?;
    validate_input(&input)?;
    if !ctx.settings_snapshot().privacy_allow_network_downloads {
        return Err(invalid("enable model downloads in Privacy settings first"));
    }
    idle_model(ctx, &input.id)?;
    let (_guard, cancel) = begin(job_id, &input.id)?;
    if url.len() > 8192 {
        return Err(invalid("model URL is too long"));
    }
    let (repo, revision, file) = studio::hugging_face_location(url)?;
    let revision = tokio::select! {
        ()=cancel.cancelled()=>return Err(NexusError::Cancelled),
        result=ctx.hf.resolve_revision(&repo,&revision)=>result.map_err(|e|invalid(&e.to_string()))?,
    };
    let tree = tokio::select! {
        ()=cancel.cancelled()=>return Err(NexusError::Cancelled),
        result=ctx.hf.tree(&repo,&revision.sha)=>result.map_err(|e|invalid(&e.to_string()))?,
    };
    let selected = tree
        .into_iter()
        .find(|f| f.path == file)
        .ok_or_else(|| invalid("repository file not found"))?;
    let sha = selected
        .sha256
        .ok_or_else(|| {
            invalid("repository file has no verifiable SHA-256; import a verified local asset instead")
        })?
        .to_ascii_lowercase();
    if !studio::valid_digest(&sha) || selected.size == 0 || selected.size > i64::MAX as u64 {
        return Err(invalid("invalid repository asset size or checksum"));
    }
    let extension = Path::new(&file)
        .extension()
        .and_then(|s| s.to_str())
        .ok_or_else(|| invalid("asset has no extension"))?;
    if !matches!(
        extension,
        "gguf" | "safetensors" | "bin" | "onnx" | "json" | "txt" | "npy" | "npz" | "model"
    ) {
        return Err(invalid("unsupported model asset extension"));
    }
    let dest = if let Some(asset) = ctx.store.studio_asset(&sha)? {
        studio::asset_destination(&root, &asset)?
    } else {
        nexus_core::portable::ensure_directory(&root, Path::new(&format!("models/blobs/{sha}")))?
            .join(format!("asset.{extension}"))
    };
    let spec = DownloadSpec {
        url: ctx.hf.resolve_url(&repo, &revision.sha, &file),
        token: ctx.hf.token().map(str::to_owned),
        expected_sha256: sha.clone(),
        expected_bytes: selected.size,
        dest: dest.clone(),
        revision: revision.sha.clone(),
    };
    if !dest.exists() {
        let result = Downloader::new()
            .map_err(|e| invalid(&e.to_string()))?
            .download_asset(&spec, progress.as_ref(), &cancel)
            .await;
        if let Err(error) = result {
            let discard = transfers()
                .lock()
                .ok()
                .and_then(|j| j.get(job_id).map(|t| t.discard))
                .unwrap_or(false);
            if discard {
                nexus_models::download::discard_part(&dest);
            }
            return Err(if cancel.is_cancelled() {
                NexusError::Cancelled
            } else {
                invalid(&error.to_string())
            });
        }
    }
    let lease = _guard.1.clone();
    let ctx = ctx.clone();
    tokio::task::spawn_blocking(move || {
        let _lease = lease;
        studio::validate_format(&dest,input.kind,&input.role)?;
        let actual=nexus_models::bundled::hash_file(&dest,&cancel,|_|{}).map_err(|e|invalid(&e.to_string()))?;
        if actual!=sha { return Err(invalid("stored asset failed integrity verification")); }
        let asset=studio::import_asset(&ctx.store,&root,&dest,&cancel,|_|{})?;
        publish(&ctx,&root,&input,asset,serde_json::json!({"source":"huggingface","repo":repo,"revision":revision.sha,"file":file,"license":revision.license}))
    }).await.map_err(|e|NexusError::Internal(e.to_string()))?
}

/// Pause keeps partial bytes for retry; cancel removes partials when the transfer has stopped.
pub fn cancel(caller: &Caller, id: &str, discard: bool) -> Result<bool, NexusError> {
    if !caller.is_desktop() {
        return Err(NexusError::ForbiddenRemote);
    }
    let mut jobs = transfers()
        .lock()
        .map_err(|_| invalid("transfer lock poisoned"))?;
    if let Some(job) = jobs.get_mut(id) {
        job.discard = discard;
        job.cancel.cancel();
        return Ok(true);
    }
    Ok(false)
}
fn invalid(message: &str) -> NexusError {
    NexusError::InvalidInput(message.into())
}

fn get_model(ctx: &AppCtx, id: &str) -> Result<StudioModel, NexusError> {
    ctx.store
        .studio_models()?
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| NexusError::NotFound("Studio model".into()))
}
fn idle_model(ctx: &AppCtx, id: &str) -> Result<(), NexusError> {
    if transfers()
        .lock()
        .map_err(|_| invalid("transfer lock poisoned"))?
        .values()
        .any(|j| j.model_id == id)
    {
        return Err(NexusError::Conflict(
            "wait for this model's transfer to finish".into(),
        ));
    }
    if ctx
        .store
        .list_studio_selections()?
        .iter()
        .any(|(_, selected)| selected == id)
    {
        return Err(NexusError::Conflict(
            "unload this workspace model before changing its assets or category".into(),
        ));
    }
    match ctx.supervisor.state() {
        nexus_core::events::RuntimeState::Starting { .. } | nexus_core::events::RuntimeState::Loading => {
            return Err(NexusError::Conflict(
                "wait for the chat engine to finish loading".into(),
            ));
        }
        nexus_core::events::RuntimeState::Ready { model_id, .. }
            if model_id
                == get_model(ctx, id)
                    .map(|m| projected_id(&m))
                    .unwrap_or_else(|_| format!("studio-{id}")) =>
        {
            return Err(NexusError::Conflict("unload this model first".into()));
        }
        _ => {}
    }
    Ok(())
}

/// Select an available model. Chat starts the existing engine; media selection is configuration only.
pub async fn select(ctx: &Ctx, caller: &Caller, id: &str) -> Result<(), NexusError> {
    let root = root(ctx, caller)?;
    let (_guard, _cancel) = begin(&format!("select-{id}"), id)?;
    let model = get_model(ctx, id)?;
    if !model.assets.iter().any(|a| a.role == "weights") {
        return Err(invalid("add the weights asset before selecting this model"));
    }
    if model.variant != "universal" {
        return Err(invalid(
            "this backend-specific variant needs its workspace engine integration before selection",
        ));
    }
    for role in &model.assets {
        studio::validate_asset(&root, &role.asset)?;
    }
    if model.kind == ModelKind::Chat {
        project_chat(ctx, &root, &model)?;
        super::models::activate(ctx, caller, &projected_id(&model)).await?;
    }
    ctx.store.studio_select(model.kind, Some(id))?;
    Ok(())
}

/// Stop the chat engine or clear a media selection. Never claim an unimplemented media engine ran.
pub async fn unload(ctx: &Ctx, caller: &Caller, kind: ModelKind) -> Result<(), NexusError> {
    root(ctx, caller)?;
    let _lease = super::portable_store::acquire()?;
    if kind == ModelKind::Chat {
        ctx.supervisor.shutdown().await;
        super::settings::set(
            ctx,
            caller,
            "models.active_model_id",
            super::settings::SettingValue::Text("builtin".into()),
        )?;
    }
    ctx.store.studio_select(kind, None)?;
    Ok(())
}

/// Remove a logical entry only. Shared/orphan bytes are retained for recovery; never unlinked blindly.
pub fn remove(ctx: &AppCtx, caller: &Caller, id: &str) -> Result<(), NexusError> {
    root(ctx, caller)?;
    idle_model(ctx, id)?;
    let (_guard, _cancel) = begin(&format!("remove-{id}"), id)?;
    let model = get_model(ctx, id)?;
    ctx.store.studio_remove(id)?;
    ctx.store.delete_model(&projected_id(&model))?;
    Ok(())
}

/// Reclassify after checking all asset formats. Selected or running models cannot be reclassified.
pub fn reclassify(ctx: &AppCtx, caller: &Caller, id: &str, kind: ModelKind) -> Result<(), NexusError> {
    let root = root(ctx, caller)?;
    idle_model(ctx, id)?;
    let (_guard, _cancel) = begin(&format!("reclassify-{id}"), id)?;
    let mut model = get_model(ctx, id)?;
    for role in &model.assets {
        let path = studio::validate_asset(&root, &role.asset)?;
        studio::validate_format(&path, kind, &role.role)?;
    }
    model.kind = kind;
    studio::register(&ctx.store, &root, &model)?;
    ctx.store.delete_model(&projected_id(&get_model(ctx, id)?))?;
    project_chat(ctx, &root, &model)?;
    Ok(())
}

/// Cancel owned transfers during shutdown, before releasing the portable distribution lock.
pub fn cancel_all() {
    if let Ok(jobs) = transfers().lock() {
        for transfer in jobs.values() {
            transfer.cancel.cancel();
        }
    }
}

fn projected_id(model: &StudioModel) -> String {
    serde_json::from_str::<serde_json::Value>(&model.metadata)
        .ok()
        .and_then(|v| v.get("legacy_id").and_then(|s| s.as_str()).map(str::to_owned))
        .unwrap_or_else(|| format!("studio-{}", model.id))
}

/// Restore ephemeral absolute chat projections from mount-independent registry references.
pub fn restore(ctx: &AppCtx) -> Result<(), NexusError> {
    let Some(root) = &ctx.paths.portable_root else {
        return Ok(());
    };
    for model in ctx.store.studio_models()? {
        if let Err(error) = project_chat(ctx, root, &model) {
            tracing::warn!(model_id=%model.id,%error,"shared model unavailable; retained for repair");
        }
    }
    Ok(())
}

/// Summary of explicit legacy adoption or offline-pack import. Sources are never removed.
#[derive(Clone, Debug, Serialize, Type, Default)]
pub struct MigrationResult {
    pub imported: Vec<String>,
    pub skipped: Vec<String>,
    pub failed: Vec<String>,
}

/// Adopt only already-registered files canonically within this stick's models directory.
/// No host profile scan, mount-prefix guessing or implicit external imports.
pub async fn adopt_legacy(
    ctx: &Ctx,
    caller: &Caller,
    job_id: &str,
    progress: Arc<dyn Fn(DownloadProgress) + Send + Sync>,
) -> Result<MigrationResult, NexusError> {
    let root = root(ctx, caller)?;
    if !matches!(
        ctx.supervisor.state(),
        nexus_core::events::RuntimeState::Stopped | nexus_core::events::RuntimeState::Failed { .. }
    ) {
        return Err(invalid("unload chat before adopting existing models"));
    }
    let (_guard, cancel) = begin(job_id, "legacy-adoption")?;
    let lease = _guard.1.clone();
    let ctx = ctx.clone();
    tokio::task::spawn_blocking(move|| {
        let _lease = lease;
        let mut result=MigrationResult::default();
        let models_root=ctx.paths.models.canonicalize()?;
        for row in ctx.store.list_models()? {
            if cancel.is_cancelled() {return Err(NexusError::Cancelled);}
            if row.extra.as_deref().and_then(|s|serde_json::from_str::<serde_json::Value>(s).ok()).is_some_and(|v|v.get("storage").and_then(|v|v.as_str())==Some("studio")) {result.skipped.push(row.id);continue;}
            let id=row.id.clone();
            let adopted=(||->Result<(),NexusError>{
                let source=Path::new(&row.path).canonicalize()?;
                if !source.starts_with(&models_root) {return Err(invalid("outside shared USB models; use explicit file/pack import"));}
                let source=ctx.paths.owned_path(&source)?;
                studio::validate_format(&source,ModelKind::Chat,"weights")?;
                let asset=studio::import_asset(&ctx.store,&root,&source,&cancel,|bytes|progress(DownloadProgress {bytes,total:row.bytes,bytes_per_sec:0.0,eta_secs:None,phase:nexus_models::download::Phase::Verifying}))?;
                if row.sha256.as_ref().is_some_and(|sha|!sha.eq_ignore_ascii_case(&asset.sha256)) {return Err(invalid("legacy checksum mismatch; source retained"));}
                let model=StudioModel {id:studio::legacy_id(&row.id),name:row.name,kind:ModelKind::Chat,variant:"universal".into(),metadata:serde_json::json!({"legacy_id":row.id,"license":row.license,"source":"legacy-adoption"}).to_string(),assets:vec![AssetRole {role:"weights".into(),asset}]};
                studio::register(&ctx.store,&root,&model)?;
                project_chat(&ctx,&root,&model)?;
                Ok(())
            })();
            match adopted {Ok(())=>result.imported.push(id),Err(error)=>result.failed.push(format!("{id}: {error}"))}
        }
        Ok(result)
    }).await.map_err(|e|NexusError::Internal(e.to_string()))?
}

/// Import a legacy schema-1 offline model pack directly into deduplicated USB assets.
pub async fn import_pack(
    ctx: &Ctx,
    caller: &Caller,
    job_id: &str,
    path: &str,
    progress: Arc<dyn Fn(DownloadProgress) + Send + Sync>,
) -> Result<MigrationResult, NexusError> {
    let root = root(ctx, caller)?;
    let dir = super::models::reference_read(ctx, Path::new(path)).await?;
    let manifest = dir.join("pack.json");
    ctx.perms.check("user", &manifest, nexus_core::fs::Access::Read)?;
    if std::fs::metadata(&manifest)?.len() > 4 * 1024 * 1024 {
        return Err(invalid("model pack manifest is too large"));
    }
    let pack = nexus_models::pack::read_pack(&dir).map_err(|e| invalid(&e.to_string()))?;
    if pack.models.len() > 1024 {
        return Err(invalid("too many models in one pack"));
    }
    let mut sources = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for model in pack.models {
        if !ids.insert(model.id.clone()) {
            return Err(invalid("duplicate model ID in pack"));
        }
        let source = studio::pack_asset(&dir, &model.path)?;
        ctx.perms.check("user", &source, nexus_core::fs::Access::Read)?;
        let id = studio::legacy_id(&format!("pack:{}", model.id));
        idle_model(ctx, &id)?;
        sources.push((id, model, source));
    }
    let (_guard, cancel) = begin(job_id, "pack-import")?;
    let lease = _guard.1.clone();
    let ctx = ctx.clone();
    tokio::task::spawn_blocking(move|| {
        let _lease = lease;
        let mut result=MigrationResult::default();
        for (id,entry,source) in sources {
            if cancel.is_cancelled() {return Err(NexusError::Cancelled);}
            let imported=(||->Result<(),NexusError>{
                studio::validate_format(&source,ModelKind::Chat,"weights")?;
                if std::fs::metadata(&source)?.len()!=entry.bytes {return Err(invalid("pack model size mismatch"));}
                let sha=nexus_models::bundled::hash_file(&source,&cancel,|bytes|progress(DownloadProgress {bytes,total:entry.bytes,bytes_per_sec:0.0,eta_secs:None,phase:nexus_models::download::Phase::Verifying})).map_err(|e|invalid(&e.to_string()))?;
                if sha!=entry.sha256 {return Err(invalid("pack checksum mismatch"));}
                if let Ok(existing)=get_model(&ctx,&id) {
                    if existing.assets.iter().any(|a|a.asset.sha256==sha) {result.skipped.push(id.clone());return Ok(());}
                    return Err(NexusError::Conflict("pack model ID already exists with different bytes".into()));
                }
                let asset=studio::import_asset(&ctx.store,&root,&source,&cancel,|bytes|progress(DownloadProgress {bytes,total:entry.bytes,bytes_per_sec:0.0,eta_secs:None,phase:nexus_models::download::Phase::Finalizing}))?;
                let model=StudioModel {id:id.clone(),name:entry.name,kind:ModelKind::Chat,variant:"universal".into(),metadata:serde_json::json!({"source":"model-pack","pack_id":entry.id,"repo":entry.repo,"revision":entry.revision,"license":entry.license}).to_string(),assets:vec![AssetRole {role:"weights".into(),asset}]};
                studio::register(&ctx.store,&root,&model)?;project_chat(&ctx,&root,&model)?;
                result.imported.push(id.clone());Ok(())
            })();
            if let Err(error)=imported {result.failed.push(format!("{id}: {error}"));}
        }
        Ok(result)
    }).await.map_err(|e|NexusError::Internal(e.to_string()))?
}
