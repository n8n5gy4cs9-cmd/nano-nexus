//! Whisper transcription through the private `whisper-cli`, ported from the reference `transcribeWavBuffer`.
//!
//! WAV input only (as in the reference): decoding other formats needs a bundled decoder that the DIST does
//! not ship yet, so other files are refused with a message instead of being passed to the engine.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::error::StudioError;
use crate::outputs::safe_name;
use crate::proc::{Spawn, Worker, library_env};
use crate::progress::parse_whisper_progress;

#[derive(Clone, Debug)]
pub struct TranscribeConfig {
    pub cli: PathBuf,
    pub model: PathBuf,
    pub wav: PathBuf,
    /// Scratch directory on the USB (never the host temp directory).
    pub work_dir: PathBuf,
    /// `auto` or a short language code.
    pub language: String,
    pub translate: bool,
    pub threads: u32,
    pub env: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Transcription {
    pub text: String,
    pub segments: Vec<Segment>,
    pub duration_ms: u64,
}

/// RIFF/WAVE header check.
pub fn is_wave(head: &[u8]) -> bool {
    head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WAVE"
}

/// Arguments for `whisper-cli`.
pub fn build_args(cfg: &TranscribeConfig, out_base: &Path) -> Vec<String> {
    let mut a = vec![
        "-m".into(),
        cfg.model.to_string_lossy().into_owned(),
        "-f".into(),
        cfg.wav.to_string_lossy().into_owned(),
        "-otxt".into(),
        "-oj".into(),
        "-of".into(),
        out_base.to_string_lossy().into_owned(),
        "-t".into(),
        cfg.threads.clamp(1, 32).to_string(),
        // Print progress so the UI can show a bar; the reference could only show a spinner.
        "-pp".into(),
        "-l".into(),
        if cfg.language.is_empty() {
            "auto".into()
        } else {
            cfg.language.clone()
        },
    ];
    if cfg.translate {
        a.push("-tr".into());
    }
    a
}

/// Parse `whisper-cli -oj` output into segments.
pub fn parse_segments(json: &str) -> Vec<Segment> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    v.get("transcription")
        .and_then(|t| t.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|s| {
                    Some(Segment {
                        start_ms: s.pointer("/offsets/from")?.as_u64()?,
                        end_ms: s.pointer("/offsets/to")?.as_u64()?,
                        text: s.get("text")?.as_str()?.trim().to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Run a transcription. `on_progress(Some(pct))` as whisper reports it, `None` while indeterminate.
pub async fn transcribe(
    cfg: &TranscribeConfig,
    on_progress: Arc<dyn Fn(Option<u32>) + Send + Sync>,
    cancel: &CancellationToken,
) -> Result<Transcription, StudioError> {
    let mut head = [0u8; 12];
    {
        use std::io::Read as _;
        let mut f = std::fs::File::open(&cfg.wav)?;
        let n = f.read(&mut head)?;
        if !is_wave(&head[..n]) {
            return Err(StudioError::Invalid(
                "Speech transcription currently accepts WAV audio only. Convert the file to WAV first."
                    .into(),
            ));
        }
    }
    if !cfg.model.is_file() {
        return Err(StudioError::Invalid(
            "The selected speech model file is missing.".into(),
        ));
    }
    std::fs::create_dir_all(&cfg.work_dir)?;
    let stem = cfg
        .wav
        .file_stem()
        .map(|s| safe_name(&s.to_string_lossy()))
        .unwrap_or_default();
    let out_base = cfg
        .work_dir
        .join(format!(".speech-{stem}-{}", getrandom::u32().unwrap_or(0)));
    let started = std::time::Instant::now();

    let progress = on_progress.clone();
    let sink: crate::proc::LineSink = Arc::new(move |line, _| {
        if let Some(p) = parse_whisper_progress(line) {
            progress(Some(p));
        }
    });
    on_progress(None);
    let mut env = library_env(&cfg.cli);
    env.extend(cfg.env.iter().cloned());
    let mut worker = Worker::spawn(
        Spawn {
            bin: cfg.cli.clone(),
            args: build_args(cfg, &out_base),
            env,
            cwd: None,
            stdin: None,
        },
        sink,
    )
    .await?;
    let code = tokio::select! {
        () = cancel.cancelled() => {
            worker.kill().await;
            cleanup(&out_base);
            return Err(StudioError::Cancelled);
        }
        c = worker.wait() => c?,
    };
    let txt = out_base.with_extension("txt");
    let json = out_base.with_extension("json");
    let result = if code == 0 {
        let text = std::fs::read_to_string(&txt)
            .unwrap_or_default()
            .trim()
            .to_owned();
        let segments = std::fs::read_to_string(&json)
            .map(|j| parse_segments(&j))
            .unwrap_or_default();
        Ok(Transcription {
            text,
            segments,
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    } else {
        let tail = worker.log_tail(12).join("\n");
        Err(StudioError::Failed(
            format!("whisper.cpp exited with code {code}.\n{tail}")
                .trim()
                .to_owned(),
        ))
    };
    cleanup(&out_base);
    on_progress(Some(100));
    result
}

fn cleanup(out_base: &Path) {
    let _ = std::fs::remove_file(out_base.with_extension("txt"));
    let _ = std::fs::remove_file(out_base.with_extension("json"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_header_and_segments() {
        assert!(is_wave(b"RIFF\0\0\0\0WAVEfmt "));
        assert!(!is_wave(b"ID3\x03"));
        let j = r#"{"transcription":[{"offsets":{"from":0,"to":1500},"text":" Hello"},{"offsets":{"from":1500,"to":3000},"text":" world "}]}"#;
        let s = parse_segments(j);
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].text, "world");
        assert_eq!(s[1].end_ms, 3000);
    }
}
