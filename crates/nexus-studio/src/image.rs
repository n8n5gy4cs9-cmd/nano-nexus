//! Image engine lifecycle: start/stop `sd-server`, generate, report progress, cancel.
//!
//! Port of the reference `startBackend` / generation proxy. `sd-server` speaks an A1111-style HTTP API
//! on loopback; it cannot abort a running sample, so cancelling kills the server and the next request
//! starts it again (the model reload cost is the price of a real cancel).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use nexus_models::media::ImageRequest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::error::StudioError;
use crate::proc::{LineSink, Spawn, Worker, library_env};
use crate::progress::{GenState, LoadState, SdEvent, parse_sd_line};

/// Where the sampling runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ImageBackend {
    Cpu,
    Vulkan,
    Cuda,
    Rocm,
    Metal,
}

/// Everything needed to start the server.
#[derive(Clone, Debug)]
pub struct ImageEngineConfig {
    pub bin: PathBuf,
    pub model: PathBuf,
    pub backend: ImageBackend,
    pub threads: u32,
    pub steps: u32,
    pub cfg_scale: f32,
    pub sampler: String,
    pub flash_attn: bool,
    pub vae_tiling: bool,
    pub vae_on_cpu: bool,
    /// Whether the binary knows `--backend` / `--params-backend` (see [`binary_supports_backend_flags`]).
    pub supports_backend_flags: bool,
    /// Vulkan device name such as `vulkan0`; `None` = `vulkan0`.
    pub vulkan_device: Option<String>,
    /// Redirected HOME/TMP/cache variables from the portable layout.
    pub env: Vec<(String, String)>,
}

/// Arguments for `sd-server`, ported from the reference. Pure so the rules can be read in one place.
pub fn build_args(cfg: &ImageEngineConfig, port: u16) -> Vec<String> {
    // The reference caps GPU runs at 4 CPU threads.
    let threads = if cfg.backend == ImageBackend::Cpu {
        cfg.threads
    } else {
        cfg.threads.min(4)
    }
    .max(1);
    let mut a: Vec<String> = vec![
        "--listen-ip".into(),
        "127.0.0.1".into(),
        "--listen-port".into(),
        port.to_string(),
        "--model".into(),
        cfg.model.to_string_lossy().into_owned(),
        "--steps".into(),
        cfg.steps.to_string(),
        "--cfg-scale".into(),
        cfg.cfg_scale.to_string(),
        "--sampling-method".into(),
        cfg.sampler.clone(),
        "--threads".into(),
        threads.to_string(),
    ];
    let flags = cfg.supports_backend_flags;
    match cfg.backend {
        ImageBackend::Cpu => backend_args(&mut a, flags, Some("cpu"), "cpu"),
        ImageBackend::Vulkan => {
            backend_args(
                &mut a,
                flags,
                Some(cfg.vulkan_device.as_deref().unwrap_or("vulkan0")),
                "cpu",
            );
        }
        ImageBackend::Cuda => backend_args(&mut a, flags, Some("cuda0"), "cuda"),
        ImageBackend::Rocm => backend_args(&mut a, flags, Some("rocm0"), "cpu"),
        ImageBackend::Metal => backend_args(&mut a, flags, None, "cpu"),
    }
    if cfg.vae_tiling {
        a.push("--vae-tiling".into());
    }
    if cfg.vae_on_cpu {
        a.push("--vae-on-cpu".into());
    }
    if cfg.flash_attn {
        a.push(
            if cfg.backend == ImageBackend::Cuda {
                "--diffusion-fa"
            } else {
                "--fa"
            }
            .into(),
        );
    }
    a
}

fn backend_args(a: &mut Vec<String>, flags: bool, name: Option<&str>, rng: &str) {
    if let (true, Some(name)) = (flags, name) {
        a.extend([
            "--backend".into(),
            name.into(),
            "--params-backend".into(),
            name.into(),
        ]);
    }
    a.extend(["--rng".into(), rng.into(), "--sampler-rng".into(), rng.into()]);
}

/// Ask the binary whether it knows the backend-selection flags (older builds reject them).
pub async fn binary_supports_backend_flags(bin: &std::path::Path) -> bool {
    let out = tokio::process::Command::new(bin)
        .arg("--help")
        .env_clear()
        .envs(library_env(bin))
        .stdin(std::process::Stdio::null())
        .output();
    match tokio::time::timeout(Duration::from_secs(15), out).await {
        Ok(Ok(o)) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            text.contains("--params-backend")
        }
        _ => false,
    }
}

/// Snapshot for the UI.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ImageStatus {
    pub running: bool,
    pub ready: bool,
    pub load: LoadState,
    pub generation: GenState,
    pub device: String,
    pub error: Option<String>,
}

/// A finished image.
#[derive(Clone, Debug)]
pub struct GeneratedImage {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub seed: i64,
    pub duration_ms: u64,
}

#[derive(Default)]
struct Inner {
    ready: bool,
    load: LoadState,
    gen_state: GenState,
    device: String,
    error: Option<String>,
}

/// One image server. Cheap to clone; clones share the same server.
#[derive(Clone, Default)]
pub struct ImageEngine {
    inner: Arc<Mutex<Inner>>,
    worker: Arc<tokio::sync::Mutex<Option<(Worker, u16)>>>,
}

fn lock(m: &Mutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// First bytes decide the image type; the server's claim is not trusted.
pub fn image_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

impl ImageEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn status(&self) -> ImageStatus {
        let running = self.worker.lock().await.is_some();
        let i = lock(&self.inner);
        ImageStatus {
            running,
            ready: i.ready,
            load: i.load.clone(),
            generation: i.gen_state.clone(),
            device: i.device.clone(),
            error: i.error.clone(),
        }
    }

    fn sink(&self) -> LineSink {
        let inner = self.inner.clone();
        Arc::new(move |line: &str, _stderr: bool| {
            let mut i = lock(&inner);
            for e in parse_sd_line(line) {
                i.load.apply(&e);
                i.gen_state.apply(&e);
                match e {
                    SdEvent::Ready => i.ready = true,
                    SdEvent::Device(d) => i.device = d,
                    SdEvent::Error(m) => i.error = Some(describe_error(&m)),
                    _ => {}
                }
            }
        })
    }

    /// Start the server and wait until it answers. Any running server is stopped first.
    pub async fn start(&self, cfg: &ImageEngineConfig) -> Result<(), StudioError> {
        self.stop().await;
        if !cfg.model.is_file() {
            return Err(StudioError::Invalid(
                "The selected image model file is missing.".into(),
            ));
        }
        *lock(&self.inner) = Inner {
            load: LoadState {
                active: true,
                phase: "Starting engine...".into(),
                ..LoadState::default()
            },
            ..Inner::default()
        };
        let port = nexus_llm::port::find_free_port()?;
        let mut env = library_env(&cfg.bin);
        env.extend(cfg.env.iter().cloned());
        let worker = Worker::spawn(
            Spawn {
                bin: cfg.bin.clone(),
                args: build_args(cfg, port),
                env,
                cwd: None,
                stdin: None,
            },
            self.sink(),
        )
        .await?;
        *self.worker.lock().await = Some((worker, port));

        // Large models on a slow CPU can take minutes to load (the reference waits up to 12 minutes).
        let limit = if cfg.backend == ImageBackend::Cpu {
            720
        } else {
            360
        };
        let started = Instant::now();
        let client = reqwest::Client::new();
        loop {
            {
                let mut w = self.worker.lock().await;
                if let Some((worker, _)) = w.as_mut()
                    && let Some(code) = worker.exited()
                {
                    let tail = worker.log_tail(8).join("\n");
                    let msg = lock(&self.inner)
                        .error
                        .clone()
                        .unwrap_or_else(|| format!("The image engine exited with code {code}.\n{tail}"));
                    *w = None;
                    lock(&self.inner).load.active = false;
                    return Err(StudioError::NotReady(msg));
                }
            }
            let answered = client
                .get(format!("http://127.0.0.1:{port}/"))
                .timeout(Duration::from_secs(2))
                .send()
                .await
                .is_ok();
            if answered || lock(&self.inner).ready {
                let mut i = lock(&self.inner);
                i.ready = true;
                i.load = LoadState {
                    phase: "Ready".into(),
                    progress: 100,
                    ..LoadState::default()
                };
                return Ok(());
            }
            if started.elapsed() > Duration::from_secs(limit) {
                self.stop().await;
                return Err(StudioError::NotReady(format!("No answer after {limit} seconds.")));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Stop the server (also clears progress).
    pub async fn stop(&self) {
        if let Some((mut w, _)) = self.worker.lock().await.take() {
            w.kill().await;
        }
        let mut i = lock(&self.inner);
        i.ready = false;
        i.load = LoadState::default();
        i.gen_state = GenState::default();
    }

    /// Generate one image. Cancelling kills the server; call [`ImageEngine::start`] again afterwards.
    pub async fn generate(
        &self,
        req: &ImageRequest,
        init_image_png: Option<&[u8]>,
        denoising_strength: f32,
        cancel: &CancellationToken,
    ) -> Result<GeneratedImage, StudioError> {
        let port = match self.worker.lock().await.as_ref() {
            Some((_, p)) if lock(&self.inner).ready => *p,
            _ => return Err(StudioError::NotReady("Start the image engine first.".into())),
        };
        let seed = req
            .seed
            .unwrap_or_else(|| i64::from(getrandom::u32().unwrap_or(0x5eed)));
        let (path, body) = match init_image_png {
            None => (
                "txt2img",
                json!({
                    "prompt": req.prompt, "negative_prompt": req.negative_prompt,
                    "width": req.width, "height": req.height, "steps": req.steps,
                    "cfg_scale": req.cfg_scale, "seed": seed,
                    "sampler_name": req.sampler, "batch_size": req.batch,
                }),
            ),
            Some(png) => (
                "img2img",
                json!({
                    "init_images": [base64::engine::general_purpose::STANDARD.encode(png)],
                    "prompt": req.prompt, "negative_prompt": req.negative_prompt,
                    "denoising_strength": denoising_strength.clamp(0.0, 1.0),
                    "width": req.width, "height": req.height, "steps": req.steps,
                    "cfg_scale": req.cfg_scale, "seed": seed,
                    "sampler_name": req.sampler, "sample_method": req.sampler,
                    "batch_size": req.batch, "n_iter": 1, "send_images": true, "save_images": false,
                }),
            ),
        };
        let started = Instant::now();
        lock(&self.inner).gen_state = GenState {
            active: true,
            ..GenState::default()
        };
        // No timeout: CPU sampling and VAE decoding can take many minutes on small machines.
        let send = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{port}/sdapi/v1/{path}"))
            .json(&body)
            .send();
        let resp = tokio::select! {
            () = cancel.cancelled() => {
                self.stop().await;
                return Err(StudioError::Cancelled);
            }
            r = send => r,
        };
        lock(&self.inner).gen_state = GenState::default();
        let resp = resp.map_err(|e| {
            StudioError::Failed(format!(
                "The image engine is not responding or crashed ({}). Start it again from the Image page.",
                e.without_url()
            ))
        })?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let detail = resp
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|v| {
                    v.get("detail")
                        .and_then(|d| d.as_str().map(str::to_owned))
                        .or_else(|| {
                            v.pointer("/error/message")
                                .and_then(|m| m.as_str().map(str::to_owned))
                        })
                })
                .unwrap_or_default();
            return Err(StudioError::Failed(
                format!("Generation failed (HTTP {code}). {detail}")
                    .trim()
                    .to_owned(),
            ));
        }
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|_| StudioError::Failed("The image engine returned an unreadable answer.".into()))?;
        let b64 = v
            .pointer("/data/0/b64_json")
            .or_else(|| v.pointer("/images/0"))
            .and_then(|b| b.as_str())
            .ok_or_else(|| StudioError::Failed("The image engine did not return an image.".into()))?;
        let b64 = b64.split_once(',').map_or(b64, |(_, rest)| rest);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|_| StudioError::Failed("The image data was not valid.".into()))?;
        let mime = image_kind(&bytes).ok_or_else(|| {
            StudioError::Failed("The engine returned data that is not a PNG, JPEG or WebP image.".into())
        })?;
        let seed = v
            .pointer("/data/0/seed")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(seed);
        Ok(GeneratedImage {
            bytes,
            mime,
            seed,
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }
}

/// Plain-language versions of the engine's most common failures (ported from the reference).
pub fn describe_error(raw: &str) -> String {
    if raw.contains("new_sd_ctx_t failed") {
        "The engine could not load this model. The file may be incomplete, damaged or not a complete checkpoint.".into()
    } else if raw.to_lowercase().contains("out of memory") || raw.contains("failed to allocate") {
        "The engine ran out of memory. Use a smaller image size, enable VAE tiling, or pick a smaller model."
            .into()
    } else {
        raw.trim().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(backend: ImageBackend) -> ImageEngineConfig {
        ImageEngineConfig {
            bin: "sd-server".into(),
            model: "m.safetensors".into(),
            backend,
            threads: 8,
            steps: 20,
            cfg_scale: 7.0,
            sampler: "euler_a".into(),
            flash_attn: true,
            vae_tiling: true,
            vae_on_cpu: false,
            supports_backend_flags: true,
            vulkan_device: None,
            env: vec![],
        }
    }

    #[test]
    fn args_bind_loopback_and_follow_the_backend() {
        let a = build_args(&cfg(ImageBackend::Cuda), 9000);
        assert!(a.windows(2).any(|w| w == ["--listen-ip", "127.0.0.1"]));
        assert!(a.windows(2).any(|w| w == ["--backend", "cuda0"]));
        assert!(a.contains(&"--diffusion-fa".to_owned()) && a.contains(&"--vae-tiling".to_owned()));
        assert!(
            a.windows(2).any(|w| w == ["--threads", "4"]),
            "GPU runs cap CPU threads"
        );
        let c = build_args(&cfg(ImageBackend::Cpu), 9000);
        assert!(c.windows(2).any(|w| w == ["--threads", "8"]));
        assert!(c.contains(&"--fa".to_owned()));
    }

    #[test]
    fn old_binaries_get_no_backend_flags() {
        let mut c = cfg(ImageBackend::Vulkan);
        c.supports_backend_flags = false;
        assert!(!build_args(&c, 1).contains(&"--backend".to_owned()));
    }

    #[test]
    fn image_type_comes_from_the_bytes() {
        assert_eq!(image_kind(&[0x89, b'P', b'N', b'G', 0, 0]), Some("image/png"));
        assert_eq!(image_kind(b"<html>"), None);
    }
}
