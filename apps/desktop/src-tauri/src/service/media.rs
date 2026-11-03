//! Image, speech-to-text and text-to-speech workspaces (UP.041-044). Desktop-only; engines are the
//! prebuilt workers of the DIST (`runtimes/<platform>/...`), run through `nexus-studio`.
//!
//! Only one heavy engine job runs at a time (as in the reference), so a small machine is never asked to
//! hold an image model and a speech model in memory together. Outputs go to the USB `outputs/` area; user
//! facing deletion moves files to the application Trash, never unlinks them.

use super::Caller;
use crate::state::Ctx;
use nexus_core::error::NexusError;
use nexus_core::fs::Access;
use nexus_fs::Trasher;
use nexus_models::{
    fit::system_memory,
    hardware::{self, Backend},
    media::{self, ImageFamily, ImageRequest, SpeakRequest, TranscribeRequest},
    studio,
};
use nexus_store::studio::ModelKind;
use nexus_studio::{
    image::{ImageBackend, ImageEngine, ImageEngineConfig, ImageStatus},
    outputs::{self, OutputRecord},
    speech::{self, TranscribeConfig},
    tts::{self, TtsConfig},
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio_util::sync::CancellationToken;

fn invalid(m: impl Into<String>) -> NexusError {
    NexusError::InvalidInput(m.into())
}

/// Which saved-output collection a call refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    Images,
    Audio,
    Transcripts,
}

impl OutputKind {
    fn dir(self, ctx: &Ctx) -> PathBuf {
        ctx.paths.outputs.join(match self {
            Self::Images => "images",
            Self::Audio => "audio",
            Self::Transcripts => "transcripts",
        })
    }
}

// ---- shared state ------------------------------------------------------------------------------------

struct Shared {
    closing: AtomicBool,
    image: ImageEngine,
    jobs: Mutex<HashMap<String, CancellationToken>>,
    busy: Mutex<Option<&'static str>>,
}

fn shared() -> &'static Shared {
    static S: OnceLock<Shared> = OnceLock::new();
    S.get_or_init(|| Shared {
        closing: AtomicBool::new(false),
        image: ImageEngine::new(),
        jobs: Mutex::default(),
        busy: Mutex::default(),
    })
}

/// Holds the "one heavy job" slot and the job's cancel token; both are released on drop.
struct JobGuard {
    id: String,
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        if let Ok(mut j) = shared().jobs.lock() {
            j.remove(&self.id);
        }
        if let Ok(mut b) = shared().busy.lock() {
            *b = None;
        }
    }
}

fn begin(id: &str, what: &'static str) -> Result<(JobGuard, CancellationToken), NexusError> {
    if id.is_empty() || id.len() > 256 {
        return Err(invalid("invalid job id"));
    }
    let mut busy = shared().busy.lock().map_err(|_| invalid("job lock poisoned"))?;
    if shared().closing.load(Ordering::SeqCst) {
        return Err(NexusError::Busy("NanoNexus is shutting down".into()));
    }
    if let Some(running) = *busy {
        return Err(NexusError::Busy(format!(
            "{running} is running. Wait for it to finish or cancel it first."
        )));
    }
    let token = CancellationToken::new();
    shared()
        .jobs
        .lock()
        .map_err(|_| invalid("job lock poisoned"))?
        .insert(id.to_owned(), token.clone());
    *busy = Some(what);
    Ok((JobGuard { id: id.to_owned() }, token))
}

/// Cancel a running image, transcription or speech job.
pub fn cancel(caller: &Caller, id: &str) -> Result<bool, NexusError> {
    if !caller.is_desktop() {
        return Err(NexusError::ForbiddenRemote);
    }
    match shared().jobs.lock().ok().and_then(|j| j.get(id).cloned()) {
        Some(t) => {
            t.cancel();
            Ok(true)
        }
        None => Ok(false),
    }
}

/// App exit: cancel jobs and stop the image server.
pub async fn shutdown() {
    if let Ok(_busy) = shared().busy.lock() {
        shared().closing.store(true, Ordering::SeqCst);
    }
    if let Ok(j) = shared().jobs.lock() {
        for t in j.values() {
            t.cancel();
        }
    }
    shared().image.stop().await;
    let drained = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if shared().jobs.lock().is_ok_and(|jobs| jobs.is_empty()) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await;
    if drained.is_err() {
        tracing::error!("media shutdown timed out; output recovery may be needed");
    }
}

// ---- DIST layout -------------------------------------------------------------------------------------

fn root(ctx: &Ctx, caller: &Caller) -> Result<PathBuf, NexusError> {
    if !caller.is_desktop() {
        return Err(NexusError::ForbiddenRemote);
    }
    ctx.paths
        .portable_root
        .clone()
        .ok_or_else(|| invalid("Image, speech and voice tools are available in the USB distribution"))
}

/// `runtimes/<os>-<arch>` as in update-plan.md.
fn platform_dir(root: &Path) -> PathBuf {
    let os = std::env::consts::OS;
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    root.join("runtimes").join(format!("{os}-{arch}"))
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

fn backend_dir(b: ImageBackend) -> &'static str {
    match b {
        ImageBackend::Cpu => "cpu",
        ImageBackend::Vulkan => "vulkan",
        ImageBackend::Cuda => "cuda",
        ImageBackend::Rocm => "rocm",
        ImageBackend::Metal => "metal",
    }
}

fn find_engine(root: &Path, kind: &str, variants: &[&str], bin: &str) -> Option<(PathBuf, String)> {
    let base = platform_dir(root).join(kind);
    // An x86-64 CPU without AVX2/FMA must never be handed the optimized `cpu` build.
    let cpu = hardware::detect_cpu();
    let old_x86 = cpu.arch == "x86_64" && !(cpu.avx2 && cpu.fma);
    variants.iter().find_map(|v| {
        let v = if old_x86 && *v == "cpu" { "cpu-baseline" } else { *v };
        let p = base.join(v).join(exe(bin));
        if p.is_file() {
            return Some((p, v.to_owned()));
        }
        // A `cpu` folder that carries all CPU variants (ggml-cpu-sse42.dll) is safe on older CPUs too.
        if old_x86 && v == "cpu-baseline" && base.join("cpu").join("ggml-cpu-sse42.dll").is_file() {
            let p = base.join("cpu").join(exe(bin));
            return p.is_file().then(|| (p, "cpu".to_owned()));
        }
        None
    })
}

fn selected_model(ctx: &Ctx, kind: ModelKind) -> Result<nexus_store::studio::StudioModel, NexusError> {
    let id = ctx
        .store
        .list_studio_selections()?
        .into_iter()
        .find(|(k, _)| k == kind.as_str())
        .map(|(_, id)| id)
        .ok_or(NexusError::ModelMissing)?;
    ctx.store
        .studio_models()?
        .into_iter()
        .find(|m| m.id == id)
        .ok_or(NexusError::ModelMissing)
}

fn asset_path(
    root: &Path,
    model: &nexus_store::studio::StudioModel,
    role: &str,
) -> Result<PathBuf, NexusError> {
    let a = model
        .assets
        .iter()
        .find(|r| r.role == role)
        .ok_or_else(|| invalid(format!("The selected model has no `{role}` file.")))?;
    studio::validate_asset(root, &a.asset)
}

fn worker_env(ctx: &Ctx) -> Vec<(String, String)> {
    ctx.paths
        .worker_environment()
        .map(|m| m.into_iter().map(|(k, v)| (k, v.display().to_string())).collect())
        .unwrap_or_default()
}

// ---- status --------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Type)]
pub struct EngineInfo {
    pub name: String,
    pub installed: bool,
    /// Variants found under `runtimes/<platform>/<kind>/`, for example `cpu`, `vulkan`.
    pub variants: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct MediaStatus {
    pub platform: String,
    pub engines: Vec<EngineInfo>,
    pub image: ImageStatus,
    pub busy: Option<String>,
    pub recommended_backend: Option<String>,
    pub notes: Vec<String>,
    /// Realistic limits for images, transcription and voice on this computer's memory.
    pub media_limits: Vec<String>,
    pub cpu: hardware::CpuInfo,
    pub total_memory_bytes: u64,
    /// Graphics devices reported by an installed chat engine. Empty = none reported or unknown.
    pub gpus: Vec<hardware::GpuInfo>,
    pub threads: u32,
    pub ctx_size: u32,
}

/// Live CPU and memory load.
pub fn telemetry(caller: &Caller) -> Result<hardware::Telemetry, NexusError> {
    if !caller.is_desktop() {
        return Err(NexusError::ForbiddenRemote);
    }
    Ok(hardware::sample_telemetry())
}

fn variants_of(root: &Path, kind: &str, bin: &str) -> Vec<String> {
    let base = platform_dir(root).join(kind);
    let mut v: Vec<String> = std::fs::read_dir(&base)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.path().join(exe(bin)).is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// What is installed and running. Missing engines are reported, not hidden.
pub async fn status(ctx: &Ctx, caller: &Caller) -> Result<MediaStatus, NexusError> {
    let root = root(ctx, caller)?;
    let image_v = variants_of(&root, "image", "sd-server");
    let speech_v = variants_of(&root, "speech", "whisper-cli");
    let tts_installed = platform_dir(&root).join("tts").join("node_modules").is_dir()
        && platform_dir(&root).join("tts").join(exe("node")).is_file();
    let cpu = hardware::detect_cpu();
    let memory = system_memory();
    let gpus: Vec<hardware::GpuInfo> = Vec::new();
    let rec = hardware::current_os().map(|os| hardware::recommend(os, &cpu, &memory, &gpus));
    let threads = rec.as_ref().map_or_else(
        || cpu.physical_cores.unwrap_or(cpu.logical_cores / 2).max(1),
        |r| r.threads,
    );
    let ctx_size = rec.as_ref().map_or(2048, |r| r.ctx_size);
    let media_limits = rec.as_ref().map(|r| r.media_limits.clone()).unwrap_or_default();
    Ok(MediaStatus {
        media_limits,
        cpu,
        total_memory_bytes: memory.total_bytes,
        gpus,
        threads,
        ctx_size,
        platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        engines: vec![
            EngineInfo {
                name: "image".into(),
                installed: !image_v.is_empty(),
                variants: image_v,
            },
            EngineInfo {
                name: "speech".into(),
                installed: !speech_v.is_empty(),
                variants: speech_v,
            },
            EngineInfo {
                name: "voice".into(),
                installed: tts_installed,
                variants: Vec::new(),
            },
        ],
        image: shared().image.status().await,
        busy: shared().busy.lock().ok().and_then(|b| b.map(str::to_owned)),
        recommended_backend: rec
            .as_ref()
            .and_then(|r| r.backend)
            .map(|b| format!("{b:?}").to_lowercase()),
        notes: rec.map(|r| r.notes).unwrap_or_default(),
    })
}

// ---- image -------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize, Type)]
pub struct ImageStartInput {
    /// `None` = follow the hardware recommendation.
    pub backend: Option<ImageBackend>,
    pub family: ImageFamily,
    pub steps: u32,
    pub cfg_scale: f32,
    pub sampler: String,
    pub threads: Option<u32>,
    pub flash_attn: bool,
    pub vae_tiling: bool,
    pub vae_on_cpu: bool,
}

fn default_image_backend() -> ImageBackend {
    let rec = hardware::current_os()
        .map(|os| hardware::recommend(os, &hardware::detect_cpu(), &system_memory(), &[]));
    match rec.and_then(|r| r.backend) {
        Some(Backend::Metal) => ImageBackend::Metal,
        Some(Backend::Vulkan) => ImageBackend::Vulkan,
        Some(Backend::Cuda) => ImageBackend::Cuda,
        _ => ImageBackend::Cpu,
    }
}

/// Start the image engine on the selected model. Falls back to the CPU build when the requested
/// accelerated build is not in the DIST, and says so in the returned note.
pub async fn image_start(ctx: &Ctx, caller: &Caller, input: ImageStartInput) -> Result<String, NexusError> {
    let root = root(ctx, caller)?;
    let model = selected_model(ctx, ModelKind::Image)?;
    let weights = asset_path(&root, &model, "weights")?;
    let mem = system_memory();
    let bytes = std::fs::metadata(&weights)?.len();
    if bytes > nexus_models::fit::budget(&mem) {
        return Err(invalid(
            "This image model is larger than the memory this computer can use. Pick a smaller model.",
        ));
    }
    let want = input.backend.unwrap_or_else(default_image_backend);
    let mut order = vec![backend_dir(want)];
    if want != ImageBackend::Cpu {
        order.push("cpu");
    }
    let (bin, found) = find_engine(&root, "image", &order, "sd-server").ok_or_else(|| {
        NexusError::RuntimeUnavailable("The image engine is not installed in this distribution.".into())
    })?;
    let backend = match found.as_str() {
        "vulkan" => ImageBackend::Vulkan,
        "cuda" => ImageBackend::Cuda,
        "rocm" => ImageBackend::Rocm,
        "metal" => ImageBackend::Metal,
        _ => ImageBackend::Cpu,
    };
    let note = if backend == want {
        format!("Using the {found} build.")
    } else {
        format!(
            "The {} build is not installed; using the CPU build instead.",
            backend_dir(want)
        )
    };
    let cfg = ImageEngineConfig {
        supports_backend_flags: nexus_studio::image::binary_supports_backend_flags(&bin).await,
        bin,
        model: weights,
        backend,
        threads: input
            .threads
            .unwrap_or_else(|| hardware::detect_cpu().physical_cores.unwrap_or(4)),
        steps: input.steps.clamp(1, 150),
        cfg_scale: input.cfg_scale,
        sampler: if media::SAMPLERS.contains(&input.sampler.as_str()) {
            input.sampler
        } else {
            "euler_a".into()
        },
        flash_attn: input.flash_attn,
        vae_tiling: input.vae_tiling,
        vae_on_cpu: input.vae_on_cpu,
        vulkan_device: None,
        env: worker_env(ctx),
    };
    shared().image.start(&cfg).await?;
    Ok(note)
}

pub async fn image_stop(caller: &Caller) -> Result<(), NexusError> {
    if !caller.is_desktop() {
        return Err(NexusError::ForbiddenRemote);
    }
    shared().image.stop().await;
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Type)]
pub struct ImageGenerateInput {
    pub request: ImageRequest,
    pub family: ImageFamily,
    /// Base64 PNG/JPEG for image-to-image.
    pub init_image: Option<String>,
    pub denoising_strength: Option<f32>,
}

/// Generate and save one batch element. The engine must be started; cancelling stops it.
pub async fn image_generate(
    ctx: &Ctx,
    caller: &Caller,
    job_id: &str,
    input: ImageGenerateInput,
) -> Result<OutputRecord, NexusError> {
    let root = root(ctx, caller)?;
    input.request.validate(input.family)?;
    let model = selected_model(ctx, ModelKind::Image)?;
    let weights = asset_path(&root, &model, "weights")?;
    media::check_image_fit(
        input.family,
        std::fs::metadata(&weights)?.len(),
        &input.request,
        &system_memory(),
    )?;
    let init = match &input.init_image {
        None => None,
        Some(b64) => {
            use base64::Engine as _;
            let raw = b64.split_once(',').map_or(b64.as_str(), |(_, r)| r);
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(raw.trim())
                .map_err(|_| invalid("The input image is not valid."))?;
            if bytes.len() > 25 * 1024 * 1024 || nexus_studio::image::image_kind(&bytes).is_none() {
                return Err(invalid(
                    "The input image must be a PNG, JPEG or WebP under 25 MB.",
                ));
            }
            Some(bytes)
        }
    };
    let (_guard, cancel) = begin(job_id, "Image generation")?;
    let image = shared()
        .image
        .generate(
            &input.request,
            init.as_deref(),
            input.denoising_strength.unwrap_or(0.7),
            &cancel,
        )
        .await?;
    let ext = match image.mime {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => "png",
    };
    let r = &input.request;
    let details = serde_json::json!({
        "mode": if init.is_some() { "img2img" } else { "txt2img" },
        "prompt": r.prompt, "negativePrompt": r.negative_prompt,
        "width": r.width, "height": r.height, "steps": r.steps, "cfgScale": r.cfg_scale,
        "sampler": r.sampler, "seed": image.seed, "model": model.name, "modelId": model.id,
        "durationMs": image.duration_ms,
    });
    let dir = OutputKind::Images.dir(ctx);
    let seed = image.seed.to_string();
    let bytes = image.bytes;
    tokio::task::spawn_blocking(move || outputs::save(&dir, "output", Some(&seed), ext, &bytes, details))
        .await
        .map_err(|e| NexusError::Internal(e.to_string()))?
        .map_err(Into::into)
}

// ---- speech to text ----------------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize, Type)]
pub struct TranscribeInput {
    /// Absolute path of a WAV file the user chose.
    pub path: String,
    pub language: String,
    pub translate_to_english: bool,
    pub threads: Option<u32>,
    /// `false` = dictation: return the text without saving a transcript.
    pub save: bool,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct TranscribeOutput {
    pub text: String,
    pub duration_ms: u64,
    pub saved: Option<OutputRecord>,
}

async fn user_read(ctx: &Ctx, path: &Path, reason: &str) -> Result<PathBuf, NexusError> {
    match ctx.perms.check("user", path, Access::Read) {
        Ok(c) => return Ok(c.into_path_buf()),
        Err(nexus_fs::FsError::NoGrant { .. }) => {}
        Err(e) => return Err(e.into()),
    }
    let decision = super::permissions::ask(
        ctx,
        super::permissions::PermissionRequest {
            id: nexus_core::ids::MessageId::new().0,
            assistant: "Voice".into(),
            path: path.display().to_string(),
            needed: Access::Read,
            reason: Some(reason.into()),
            requested_by: None,
        },
    )
    .await;
    if decision == super::permissions::Decision::Deny {
        return Err(NexusError::PermissionDenied(
            "Read access is required to use this file.".into(),
        ));
    }
    Ok(ctx.perms.check("user", path, Access::Read)?.into_path_buf())
}

pub async fn transcribe(
    ctx: &Ctx,
    caller: &Caller,
    job_id: &str,
    input: TranscribeInput,
    progress: Arc<dyn Fn(Option<u32>) + Send + Sync>,
) -> Result<TranscribeOutput, NexusError> {
    let root = root(ctx, caller)?;
    let source = user_read(ctx, Path::new(&input.path), "to transcribe this audio file").await?;
    let meta = std::fs::metadata(&source)?;
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    TranscribeRequest {
        file_name: name.clone(),
        bytes: meta.len(),
        language: input.language.clone(),
        translate_to_english: input.translate_to_english,
    }
    .validate()?;
    let model = selected_model(ctx, ModelKind::Speech)?;
    let weights = asset_path(&root, &model, "weights")?;
    let (cli, variant) = find_engine(&root, "speech", &["metal", "cpu"], "whisper-cli")
        .or_else(|| find_engine(&root, "speech", &["vulkan", "cpu-baseline"], "whisper-cli"))
        .ok_or_else(|| {
            NexusError::RuntimeUnavailable("The speech engine is not installed in this distribution.".into())
        })?;
    // Shut the image engine first: a small machine cannot hold both models.
    shared().image.stop().await;
    let (_guard, cancel) = begin(job_id, "Transcription")?;
    let cfg = TranscribeConfig {
        cli,
        model: weights,
        wav: source,
        work_dir: ctx.paths.cache.join("speech"),
        language: input.language.clone(),
        translate: input.translate_to_english,
        threads: input
            .threads
            .unwrap_or_else(|| hardware::detect_cpu().physical_cores.unwrap_or(4)),
        env: worker_env(ctx),
    };
    let t = speech::transcribe(&cfg, progress, &cancel).await?;
    let saved = if input.save {
        let details = serde_json::json!({
            "text": t.text, "segments": t.segments, "model": model.name, "modelId": model.id,
            "language": input.language, "translated": input.translate_to_english,
            "engine": variant, "sourceFile": name, "durationMs": t.duration_ms,
        });
        let dir = OutputKind::Transcripts.dir(ctx);
        let body = t.text.clone().into_bytes();
        let stem = Path::new(&name)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Some(
            tokio::task::spawn_blocking(move || {
                outputs::save(&dir, "transcript", Some(&stem), "txt", &body, details)
            })
            .await
            .map_err(|e| NexusError::Internal(e.to_string()))??,
        )
    } else {
        None
    };
    Ok(TranscribeOutput {
        text: t.text,
        duration_ms: t.duration_ms,
        saved,
    })
}

// ---- text to speech ----------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Type)]
pub struct VoiceInfo {
    pub id: String,
    pub name: String,
}

pub fn voices() -> Vec<VoiceInfo> {
    tts::DEFAULT_VOICES
        .iter()
        .map(|(id, name)| VoiceInfo {
            id: (*id).into(),
            name: (*name).into(),
        })
        .collect()
}

pub async fn speak(
    ctx: &Ctx,
    caller: &Caller,
    job_id: &str,
    req: SpeakRequest,
    progress: Arc<dyn Fn(nexus_studio::progress::TtsProgress) + Send + Sync>,
) -> Result<OutputRecord, NexusError> {
    let root = root(ctx, caller)?;
    req.validate()?;
    let model = selected_model(ctx, ModelKind::Tts)?;
    let meta: serde_json::Value = serde_json::from_str(&model.metadata).unwrap_or_default();
    let rt = platform_dir(&root).join("tts");
    shared().image.stop().await;
    let (_guard, cancel) = begin(job_id, "Speech generation")?;
    let cfg = TtsConfig {
        node: rt.join(exe("node")),
        worker_script: tts::ensure_worker_script(&ctx.paths.cache.join("tts"))?,
        runtime_dir: rt,
        cache_dir: root.join("models").join("tts-cache"),
        scratch_dir: ctx.paths.cache.join("tts"),
        model_id: meta
            .get("modelId")
            .and_then(|v| v.as_str())
            .unwrap_or("onnx-community/Kokoro-82M-v1.0-ONNX")
            .to_owned(),
        dtype: meta
            .get("dtype")
            .and_then(|v| v.as_str())
            .unwrap_or("q8")
            .to_owned(),
        allow_download: false,
        env: worker_env(ctx),
    };
    let clip = tts::synthesize(&cfg, &req, progress, &cancel).await?;
    let dir = OutputKind::Audio.dir(ctx);
    let details = serde_json::json!({
        "text": req.text, "voice": req.voice, "speed": req.speed, "model": model.name,
        "durationMs": clip.duration_ms, "sampleRate": clip.sample_rate, "segments": clip.segments,
    });
    let voice = req.voice.clone();
    let wav = clip.wav.clone();
    let saved = tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&wav)?;
        let r = outputs::save(&dir, "speech", Some(&voice), "wav", &bytes, details);
        let _ = std::fs::remove_file(&wav);
        r
    })
    .await
    .map_err(|e| NexusError::Internal(e.to_string()))??;
    Ok(saved)
}

// ---- gallery -----------------------------------------------------------------------------------------

pub fn list_outputs(ctx: &Ctx, caller: &Caller, kind: OutputKind) -> Result<Vec<OutputRecord>, NexusError> {
    root(ctx, caller)?;
    Ok(outputs::list(&kind.dir(ctx)))
}

/// File bytes as standard base64 (for thumbnails, playback and export by the UI).
pub fn read_output(ctx: &Ctx, caller: &Caller, kind: OutputKind, name: &str) -> Result<String, NexusError> {
    use base64::Engine as _;
    root(ctx, caller)?;
    let path = outputs::resolve(&kind.dir(ctx), name)?;
    let len = std::fs::metadata(&path)?.len();
    if len > 64 * 1024 * 1024 {
        return Err(invalid("That file is too large to preview."));
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(std::fs::read(path)?))
}

/// Move outputs to the application Trash (recoverable); the files and their metadata go together.
pub fn delete_outputs(
    ctx: &Ctx,
    caller: &Caller,
    kind: OutputKind,
    names: Vec<String>,
) -> Result<usize, NexusError> {
    root(ctx, caller)?;
    if names.is_empty() || names.len() > 500 {
        return Err(invalid("choose between 1 and 500 outputs"));
    }
    let plan = outputs::removal_paths(&kind.dir(ctx), &names)?;
    let trash = nexus_fs::MoveToDirTrash::new(ctx.paths.trash.join("outputs"));
    let mut moved = 0;
    for p in plan {
        let moved_result = trash.trash(&p);
        nexus_core::write_audit::record(
            "outputs",
            "move-to-trash",
            &p,
            None,
            moved_result.as_ref().map(|_| ()).map_err(String::as_str),
        );
        moved_result.map_err(NexusError::Internal)?;
        moved += 1;
    }
    Ok(moved)
}
