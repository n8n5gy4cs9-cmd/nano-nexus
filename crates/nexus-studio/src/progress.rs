//! Parsers for engine output, ported from the reference server's stdout handling.
//!
//! stable-diffusion.cpp draws progress bars with carriage returns and ANSI colours; whisper.cpp prints
//! `progress = N%`. These functions are pure so the rules live in one place.

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Remove ANSI escape sequences.
pub fn strip_ansi(s: &str) -> String {
    match Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]") {
        Ok(re) => re.replace_all(s, "").into_owned(),
        Err(_) => s.to_owned(),
    }
}

/// Model-loading progress of the image server.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct LoadState {
    pub active: bool,
    pub phase: String,
    /// 0 to 99 while loading.
    pub progress: u32,
    pub current: u32,
    pub total: u32,
    pub speed: String,
}

/// Sampling progress of one image.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct GenState {
    pub active: bool,
    pub step: u32,
    pub steps: u32,
    /// For example `1.23it/s`.
    pub speed: String,
    /// The VAE is decoding; step counts no longer move.
    pub decoding: bool,
}

/// What a single output line changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SdEvent {
    Ready,
    LoadingWeights,
    InitializingModel,
    Loading { current: u32, total: u32, label: String },
    GenerationStarted,
    Decoding,
    Step { step: u32, steps: u32, speed: String },
    GenerationDone,
    Device(String),
    Error(String),
}

/// Interpret one (already split) output line of `sd-server`.
pub fn parse_sd_line(raw: &str) -> Vec<SdEvent> {
    let line = strip_ansi(raw);
    let line = line.trim();
    let mut out = Vec::new();
    if line.is_empty() {
        return out;
    }
    if line.contains("listening on") {
        out.push(SdEvent::Ready);
    }
    if line.contains("loading model from") {
        out.push(SdEvent::LoadingWeights);
    }
    if line.contains("model files processing completed") {
        out.push(SdEvent::InitializingModel);
    }
    if line.contains("generate_image completed") {
        out.push(SdEvent::GenerationDone);
        return out;
    }
    if line.contains("generate_image") || line.contains("generating image") {
        out.push(SdEvent::GenerationStarted);
    }
    if line.contains("decoding") {
        out.push(SdEvent::Decoding);
    }
    if let Ok(step) = Regex::new(r"\|\s*[^|]*\s*\|\s*(\d+)/(\d+)\s*-\s*([\d.]+\s*(?:it/s|s/it))")
        && let Some(c) = step.captures(line)
        && let (Ok(step), Ok(steps)) = (c[1].parse(), c[2].parse())
    {
        out.push(SdEvent::Step {
            step,
            steps,
            speed: c[3].trim().to_owned(),
        });
    }
    if !line.contains("it/s")
        && !line.contains("s/it")
        && let Ok(load) = Regex::new(r"\|\s*(\d+)/(\d+)\s*-\s*([^|]+)$")
        && let Some(c) = load.captures(line)
        && let (Ok(current), Ok(total)) = (c[1].parse(), c[2].parse())
    {
        out.push(SdEvent::Loading {
            current,
            total,
            label: c[3].trim().to_owned(),
        });
    }
    if let Ok(dev) = Regex::new(r"(?:Device\s+\d+:|GPU name:)\s*([^,\r\n]+)")
        && let Some(c) = dev.captures(line)
    {
        out.push(SdEvent::Device(c[1].trim().to_owned()));
    }
    if line.contains("[ERROR]") {
        out.push(SdEvent::Error(line.to_owned()));
    }
    out
}

impl LoadState {
    /// Fold one event into the loading state.
    pub fn apply(&mut self, e: &SdEvent) {
        match e {
            SdEvent::LoadingWeights => {
                self.active = true;
                self.phase = "Loading model weights...".into();
                self.progress = self.progress.max(1);
            }
            SdEvent::InitializingModel => {
                self.active = true;
                self.phase = "Initializing model...".into();
                self.progress = self.progress.max(95);
            }
            SdEvent::Loading {
                current,
                total,
                label,
            } => {
                let pct = if *total > 0 { current * 100 / total } else { 0 };
                self.active = true;
                self.phase = "Loading model weights...".into();
                self.speed = label.clone();
                self.current = *current;
                self.total = *total;
                self.progress = self.progress.max(pct.min(99));
            }
            SdEvent::Ready => {
                self.active = false;
                self.progress = 100;
                self.phase = "Ready".into();
            }
            _ => {}
        }
    }
}

impl GenState {
    /// Fold one event into the sampling state.
    pub fn apply(&mut self, e: &SdEvent) {
        match e {
            SdEvent::GenerationStarted => {
                *self = Self {
                    active: true,
                    ..Self::default()
                }
            }
            SdEvent::Decoding if self.active => self.decoding = true,
            SdEvent::Step { step, steps, speed } => {
                self.active = true;
                if !self.decoding {
                    self.step = *step;
                    self.steps = *steps;
                    self.speed = speed.clone();
                }
            }
            SdEvent::GenerationDone => *self = Self::default(),
            _ => {}
        }
    }

    /// Whole-image progress, 0 to 99; the last step still leaves the VAE decode to do.
    pub fn percent(&self) -> Option<u32> {
        if self.steps == 0 {
            return None;
        }
        Some((self.step * 100 / self.steps).min(99))
    }
}

/// `progress = 42%` from `whisper-cli -pp`.
pub fn parse_whisper_progress(raw: &str) -> Option<u32> {
    let re = Regex::new(r"progress\s*=\s*(\d{1,3})\s*%").ok()?;
    let n: u32 = re.captures(raw)?[1].parse().ok()?;
    Some(n.min(100))
}

/// The Kokoro worker reports `@@progress {json}` on stderr.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TtsProgress {
    pub phase: String,
    pub done: u32,
    pub total: u32,
}

pub fn parse_tts_progress(line: &str) -> Option<TtsProgress> {
    let json = line.strip_prefix("@@progress ")?;
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    Some(TtsProgress {
        phase: v.get("phase")?.as_str()?.to_owned(),
        done: u32::try_from(v.get("done").and_then(serde_json::Value::as_u64).unwrap_or(0)).unwrap_or(0),
        total: u32::try_from(v.get("total").and_then(serde_json::Value::as_u64).unwrap_or(0)).unwrap_or(0),
    })
}
