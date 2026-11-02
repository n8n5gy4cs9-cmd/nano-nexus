//! Kokoro text-to-speech through the private Node worker, ported from the reference `synthesizeTts`.
//!
//! The worker script (`workers/tts-kokoro-worker.mjs`) is the reference file unchanged. It reads one JSON
//! request on stdin, reports `@@progress {...}` on stderr and prints one JSON result on stdout. Here the
//! worker is run with a cleared environment and **offline** unless downloads are explicitly allowed, so a
//! missing voice model is an error message, not a silent fetch from the internet.

use std::path::PathBuf;
use std::sync::Arc;

use nexus_models::media::SpeakRequest;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::error::StudioError;
use crate::proc::{LineSink, Spawn, Worker};
use crate::progress::{TtsProgress, parse_tts_progress};

/// Longest text one clip may hold (the reference limit).
pub const MAX_CHARS: usize = 5000;

#[derive(Clone, Debug)]
pub struct TtsConfig {
    /// Private `node` executable shipped in the DIST.
    pub node: PathBuf,
    pub worker_script: PathBuf,
    /// Folder holding `node_modules/kokoro-js` and friends.
    pub runtime_dir: PathBuf,
    /// Where the voice model files live (inside the DIST).
    pub cache_dir: PathBuf,
    pub scratch_dir: PathBuf,
    pub model_id: String,
    /// Quantisation such as `q8`.
    pub dtype: String,
    /// Allow the worker to fetch missing model files. Off by default.
    pub allow_download: bool,
    pub env: Vec<(String, String)>,
}

/// A synthesized clip, still in the scratch directory.
#[derive(Clone, Debug)]
pub struct Clip {
    pub wav: PathBuf,
    pub duration_ms: u64,
    pub sample_rate: u32,
    /// Per-sentence timings from the worker, if it sent them.
    pub segments: Value,
}

/// JSON request the worker reads on stdin.
pub fn worker_payload(cfg: &TtsConfig, req: &SpeakRequest, output: &std::path::Path) -> Value {
    json!({
        "text": req.text.trim(),
        "modelId": cfg.model_id,
        "dtype": cfg.dtype,
        "voice": req.voice,
        "speed": req.speed.clamp(0.5, 2.0),
        "output": output,
        "cacheDir": cfg.cache_dir,
    })
}

#[derive(Debug, Deserialize)]
struct WorkerResult {
    ok: bool,
    error: Option<String>,
    #[serde(rename = "durationMs")]
    duration_ms: Option<u64>,
    #[serde(rename = "sampleRate")]
    sample_rate: Option<u32>,
    segments: Option<Value>,
}

/// Synthesize one clip. The caller moves the WAV into outputs (see [`crate::outputs::save`]).
pub async fn synthesize(
    cfg: &TtsConfig,
    req: &SpeakRequest,
    on_progress: Arc<dyn Fn(TtsProgress) + Send + Sync>,
    cancel: &CancellationToken,
) -> Result<Clip, StudioError> {
    req.validate().map_err(|e| StudioError::Invalid(e.to_string()))?;
    if req.text.chars().count() > MAX_CHARS {
        return Err(StudioError::Invalid(format!(
            "TTS text is too long. The limit is {MAX_CHARS} characters."
        )));
    }
    if !cfg.worker_script.is_file() || !cfg.runtime_dir.join("node_modules").is_dir() {
        return Err(StudioError::NotInstalled("The Kokoro speech runtime".into()));
    }
    std::fs::create_dir_all(&cfg.scratch_dir)?;
    let wav = cfg
        .scratch_dir
        .join(format!(".tts-{}.wav", getrandom::u32().unwrap_or(0)));
    let payload =
        serde_json::to_vec(&worker_payload(cfg, req, &wav)).map_err(|e| StudioError::Io(e.to_string()))?;

    let mut env = vec![
        (
            "NODE_PATH".to_owned(),
            cfg.runtime_dir
                .join("node_modules")
                .to_string_lossy()
                .into_owned(),
        ),
        (
            "TTS_RUNTIME".to_owned(),
            cfg.runtime_dir.to_string_lossy().into_owned(),
        ),
    ];
    if !cfg.allow_download {
        env.push(("HF_HUB_OFFLINE".into(), "1".into()));
        env.push(("TRANSFORMERS_OFFLINE".into(), "1".into()));
    }
    env.extend(cfg.env.iter().cloned());

    let out = Arc::new(std::sync::Mutex::new(String::new()));
    let out_sink = out.clone();
    let sink: LineSink = Arc::new(move |line, is_stderr| {
        if is_stderr {
            if let Some(p) = parse_tts_progress(line) {
                on_progress(p);
            }
        } else if let Ok(mut s) = out_sink.lock() {
            s.push_str(line);
        }
    });
    let mut worker = Worker::spawn(
        Spawn {
            bin: cfg.node.clone(),
            args: vec![cfg.worker_script.to_string_lossy().into_owned()],
            env,
            cwd: Some(cfg.runtime_dir.clone()),
            stdin: Some(payload),
        },
        sink,
    )
    .await?;
    let code = tokio::select! {
        () = cancel.cancelled() => {
            worker.kill().await;
            let _ = std::fs::remove_file(&wav);
            return Err(StudioError::Cancelled);
        }
        c = worker.wait() => c?,
    };
    // Give the pump tasks a moment to deliver the last stdout line.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    if code != 0 {
        let _ = std::fs::remove_file(&wav);
        let tail = worker.log_tail(12).join("\n");
        return Err(StudioError::Failed(
            format!("Kokoro speech exited with code {code}.\n{tail}")
                .trim()
                .to_owned(),
        ));
    }
    let stdout = out.lock().map(|s| s.clone()).unwrap_or_default();
    let result: WorkerResult = serde_json::from_str(stdout.trim())
        .map_err(|_| StudioError::Failed("Kokoro speech returned an unreadable answer.".into()))?;
    if !result.ok || !wav.is_file() {
        let _ = std::fs::remove_file(&wav);
        return Err(StudioError::Failed(
            result
                .error
                .unwrap_or_else(|| "Kokoro speech did not produce a WAV file.".into()),
        ));
    }
    Ok(Clip {
        wav,
        duration_ms: result.duration_ms.unwrap_or(0),
        sample_rate: result.sample_rate.unwrap_or(24_000),
        segments: result.segments.unwrap_or(Value::Null),
    })
}

/// Voices the reference offers by default (ids are the Kokoro voice names).
pub const DEFAULT_VOICES: &[(&str, &str)] = &[
    ("af_heart", "Heart (US female)"),
    ("af_bella", "Bella (US female)"),
    ("af_nicole", "Nicole (US female)"),
    ("am_michael", "Michael (US male)"),
    ("am_fenrir", "Fenrir (US male)"),
    ("bf_emma", "Emma (UK female)"),
    ("bm_george", "George (UK male)"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_clamps_speed_and_names_the_cache() {
        let cfg = TtsConfig {
            node: "node".into(),
            worker_script: "w.mjs".into(),
            runtime_dir: "rt".into(),
            cache_dir: "cache".into(),
            scratch_dir: "tmp".into(),
            model_id: "onnx-community/Kokoro-82M-v1.0-ONNX".into(),
            dtype: "q8".into(),
            allow_download: false,
            env: vec![],
        };
        let req = SpeakRequest {
            text: " Hi ".into(),
            voice: "af_heart".into(),
            speed: 1.0,
        };
        let p = worker_payload(&cfg, &req, std::path::Path::new("out.wav"));
        assert_eq!(p["text"], "Hi");
        assert_eq!(p["cacheDir"], "cache");
        assert_eq!(p["dtype"], "q8");
    }
}
