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
    cancel: CancellationToken,
    discard: bool,
}
fn transfers() -> &'static Mutex<HashMap<String, Transfer>> {
    static JOBS: OnceLock<Mutex<HashMap<String, Transfer>>> = OnceLock::new();
    JOBS.get_or_init(Default::default)
}
struct TransferGuard(String);
impl Drop for TransferGuard {
    fn drop(&mut self) {
        if let Ok(mut jobs) = transfers().lock() {
            jobs.remove(&self.0);
        }
    }
}
fn begin(id: &str) -> Result<(TransferGuard, CancellationToken), NexusError> {
    if id.is_empty() || id.len() > 128 {
        return Err(invalid("invalid transfer id"));
    }
    let mut jobs = transfers()
        .lock()
        .map_err(|_| invalid("transfer lock poisoned"))?;
    if jobs.len() >= 8 || jobs.contains_key(id) {
        return Err(NexusError::Conflict(
            "transfer already running or queue full".into(),
        ));
    }
    let token = CancellationToken::new();
    jobs.insert(
        id.into(),
        Transfer {
            cancel: token.clone(),
            discard: false,
        },
    );
    Ok((TransferGuard(id.into()), token))
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
    metadata[&input.role] = provenance;
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
    ctx.store.upsert_model(&ModelRow {
        id: format!("studio-{}", model.id),
        source: "import".into(),
        name: model.name.clone(),
        repo: None,
        revision: None,
        file: path.file_name().map(|s| s.to_string_lossy().into_owned()),
        path: path.display().to_string(),
        sha256: Some(weights.asset.sha256.clone()),
        bytes: weights.asset.bytes,
        quant: meta.quant.clone(),
        catalog_id: None,
        gguf_meta: serde_json::to_string(&meta).ok(),
        license: meta.license.clone(),
        extra: Some(
            serde_json::json!({"storage":"studio","studio_id":model.id,"capabilities":["chat"]}).to_string(),
        ),
        added_at: 0,
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
    let (_guard, cancel) = begin(job_id)?;
    let source = PathBuf::from(path);
    // Native picker/drag-drop is user intent; still enforce built-in and user deny rules.
    let source = super::models::reference_read(ctx, &source).await?;
    let ctx = ctx.clone();
    tokio::task::spawn_blocking(move || {
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
    let (_guard, cancel) = begin(job_id)?;
    if url.len() > 8192 {
        return Err(invalid("model URL is too long"));
    }
    let (repo, revision, file) = studio::hugging_face_location(url)?;
    let revision = ctx
        .hf
        .resolve_revision(&repo, &revision)
        .await
        .map_err(|e| invalid(&e.to_string()))?;
    let tree = ctx
        .hf
        .tree(&repo, &revision.sha)
        .await
        .map_err(|e| invalid(&e.to_string()))?;
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
        studio::validate_asset(&root, &asset)?
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
    let ctx = ctx.clone();
    tokio::task::spawn_blocking(move || {
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
