//! Validated request types for the image, speech-to-text and text-to-speech workspaces (UP.041/043/044).
//!
//! This is the checked input layer only. No engine is started here: the prebuilt workers do not exist yet
//! (UP.050 onward), so nothing in this module claims that generation or transcription works. Limits are
//! conservative so a bad request fails with a message before any process is spawned.

use nexus_core::error::NexusError;
use serde::{Deserialize, Serialize};

use crate::fit::MemoryInfo;

/// Longest prompt accepted by the image and speech workspaces, in characters.
pub const MAX_PROMPT_CHARS: usize = 4000;
/// Longest text sent to speech synthesis in one request, in characters.
pub const MAX_TTS_CHARS: usize = 20_000;
/// Largest audio file accepted for transcription.
pub const MAX_AUDIO_BYTES: u64 = 512 * 1024 * 1024;

fn bad(msg: impl Into<String>) -> NexusError {
    NexusError::InvalidInput(msg.into())
}

/// Checkpoint families the image engine can load.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ImageFamily {
    Sd15,
    Sdxl,
}

impl ImageFamily {
    /// Native edge length; sizes far from it give poor results and use much more memory.
    fn native(self) -> u32 {
        match self {
            Self::Sd15 => 512,
            Self::Sdxl => 1024,
        }
    }

    /// Rough memory the engine needs on top of the weights, in bytes, at the native size.
    fn working_bytes(self) -> u64 {
        match self {
            Self::Sd15 => 2 * 1024 * 1024 * 1024,
            Self::Sdxl => 4 * 1024 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ImageRequest {
    pub prompt: String,
    pub negative_prompt: String,
    pub width: u32,
    pub height: u32,
    pub steps: u32,
    pub cfg_scale: f32,
    /// `None` = pick a random seed (the engine records the one it used).
    pub seed: Option<i64>,
    pub batch: u32,
    pub sampler: String,
}

/// Samplers the engine accepts. Anything else is rejected rather than passed to a command line.
pub const SAMPLERS: &[&str] = &["euler", "euler_a", "heun", "dpm2", "dpm++2m", "lcm"];

impl ImageRequest {
    /// Check ranges and the model family. Returns the request unchanged on success.
    pub fn validate(&self, family: ImageFamily) -> Result<(), NexusError> {
        if self.prompt.trim().is_empty() {
            return Err(bad("Write a prompt first."));
        }
        if self.prompt.chars().count() > MAX_PROMPT_CHARS
            || self.negative_prompt.chars().count() > MAX_PROMPT_CHARS
        {
            return Err(bad(format!(
                "Prompts can be at most {MAX_PROMPT_CHARS} characters."
            )));
        }
        for (name, v) in [("Width", self.width), ("Height", self.height)] {
            if !(256..=2048).contains(&v) || v % 64 != 0 {
                return Err(bad(format!(
                    "{name} must be a multiple of 64 between 256 and 2048."
                )));
            }
        }
        if u64::from(self.width) * u64::from(self.height) > 2048 * 2048 {
            return Err(bad(
                "That image is too large; keep width × height under 4 megapixels.",
            ));
        }
        if !(1..=150).contains(&self.steps) {
            return Err(bad("Steps must be between 1 and 150."));
        }
        if !self.cfg_scale.is_finite() || !(0.0..=30.0).contains(&self.cfg_scale) {
            return Err(bad("Guidance (CFG) must be between 0 and 30."));
        }
        if !(1..=8).contains(&self.batch) {
            return Err(bad("Batch must be between 1 and 8 images."));
        }
        if !SAMPLERS.contains(&self.sampler.as_str()) {
            return Err(bad(format!(
                "Unknown sampler. Choose one of: {}.",
                SAMPLERS.join(", ")
            )));
        }
        let native = family.native();
        if self.width.max(self.height) > native * 2 {
            return Err(bad(format!(
                "This model works best near {native} px; sizes above {} px are refused.",
                native * 2
            )));
        }
        Ok(())
    }
}

/// Will the engine fit? `weights_bytes` is the checkpoint size.
///
/// Returns `Err` with a plain message when the estimate exceeds the usable memory. The estimate is
/// approximate (working memory scales with pixel count and batch) and not measured on real hardware.
pub fn check_image_fit(
    family: ImageFamily,
    weights_bytes: u64,
    req: &ImageRequest,
    mem: &MemoryInfo,
) -> Result<(), NexusError> {
    let native = u64::from(family.native());
    let pixels = u64::from(req.width) * u64::from(req.height);
    let scale = (pixels * 100 / (native * native)).max(100);
    let working = family.working_bytes().saturating_mul(scale) / 100;
    let need = weights_bytes.saturating_add(working);
    let budget = crate::fit::budget(mem);
    if need > budget {
        return Err(bad(format!(
            "This image needs about {} MB but only about {} MB is usable. Use a smaller size or a smaller model.",
            need / (1024 * 1024),
            budget / (1024 * 1024)
        )));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TranscribeRequest {
    /// File name only (used for the saved transcript's label), never a path.
    pub file_name: String,
    pub bytes: u64,
    /// BCP-47-like short code (`en`, `fi`) or `auto`.
    pub language: String,
    pub translate_to_english: bool,
}

impl TranscribeRequest {
    pub fn validate(&self) -> Result<(), NexusError> {
        if self.file_name.is_empty() || self.file_name.contains(['/', '\\']) || self.file_name.contains("..")
        {
            return Err(bad("The audio file name is not valid."));
        }
        if self.bytes == 0 {
            return Err(bad("The audio file is empty."));
        }
        if self.bytes > MAX_AUDIO_BYTES {
            return Err(bad("The audio file is larger than 512 MB."));
        }
        let lang = self.language.as_str();
        let ok =
            lang == "auto" || ((2..=3).contains(&lang.len()) && lang.bytes().all(|b| b.is_ascii_lowercase()));
        if !ok {
            return Err(bad("Language must be `auto` or a short code such as `en`."));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct SpeakRequest {
    pub text: String,
    /// Voice id from the installed voice asset, never a path.
    pub voice: String,
    pub speed: f32,
}

impl SpeakRequest {
    pub fn validate(&self) -> Result<(), NexusError> {
        if self.text.trim().is_empty() {
            return Err(bad("Write some text to speak."));
        }
        if self.text.chars().count() > MAX_TTS_CHARS {
            return Err(bad(format!(
                "Text can be at most {MAX_TTS_CHARS} characters per clip."
            )));
        }
        let v = &self.voice;
        if v.is_empty()
            || v.len() > 64
            || !v
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(bad("The voice name is not valid."));
        }
        if !self.speed.is_finite() || !(0.5..=2.0).contains(&self.speed) {
            return Err(bad("Speed must be between 0.5 and 2."));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img() -> ImageRequest {
        ImageRequest {
            prompt: "a lighthouse".into(),
            negative_prompt: String::new(),
            width: 512,
            height: 512,
            steps: 20,
            cfg_scale: 7.0,
            seed: None,
            batch: 1,
            sampler: "euler_a".into(),
        }
    }

    #[test]
    fn a_normal_image_request_is_accepted() {
        assert!(img().validate(ImageFamily::Sd15).is_ok());
    }

    #[test]
    fn bad_image_parameters_are_rejected_with_messages() {
        let mut r = img();
        r.prompt = "  ".into();
        assert!(r.validate(ImageFamily::Sd15).is_err());
        for edit in [
            (|r: &mut ImageRequest| r.width = 500) as fn(&mut ImageRequest),
            |r| r.steps = 0,
            |r| r.cfg_scale = f32::NAN,
            |r| r.batch = 9,
            |r| r.sampler = "rm -rf".into(),
            |r| {
                r.width = 2048;
                r.height = 2048;
            },
        ] {
            let mut r = img();
            edit(&mut r);
            assert!(r.validate(ImageFamily::Sd15).is_err());
        }
    }

    #[test]
    fn large_images_do_not_fit_small_machines() {
        let mem = MemoryInfo {
            total_bytes: 8 * 1024 * 1024 * 1024,
            available_bytes: 8 * 1024 * 1024 * 1024,
            unified: false,
        };
        let mut r = img();
        assert!(check_image_fit(ImageFamily::Sd15, 2 * 1024 * 1024 * 1024, &r, &mem).is_ok());
        r.width = 1024;
        r.height = 1024;
        assert!(check_image_fit(ImageFamily::Sdxl, 7 * 1024 * 1024 * 1024, &r, &mem).is_err());
    }

    #[test]
    fn transcription_requests_reject_paths_empty_and_huge_files() {
        let ok = TranscribeRequest {
            file_name: "talk.wav".into(),
            bytes: 10,
            language: "auto".into(),
            translate_to_english: false,
        };
        assert!(ok.validate().is_ok());
        for bad in [
            TranscribeRequest {
                file_name: "../x.wav".into(),
                ..ok.clone()
            },
            TranscribeRequest {
                bytes: 0,
                ..ok.clone()
            },
            TranscribeRequest {
                bytes: MAX_AUDIO_BYTES + 1,
                ..ok.clone()
            },
            TranscribeRequest {
                language: "English".into(),
                ..ok.clone()
            },
        ] {
            assert!(bad.validate().is_err());
        }
    }

    #[test]
    fn speech_requests_validate_text_voice_and_speed() {
        let ok = SpeakRequest {
            text: "Hello".into(),
            voice: "af_heart".into(),
            speed: 1.0,
        };
        assert!(ok.validate().is_ok());
        for bad in [
            SpeakRequest {
                text: String::new(),
                ..ok.clone()
            },
            SpeakRequest {
                voice: "../voice".into(),
                ..ok.clone()
            },
            SpeakRequest {
                speed: 3.0,
                ..ok.clone()
            },
            SpeakRequest {
                text: "x".repeat(MAX_TTS_CHARS + 1),
                ..ok.clone()
            },
        ] {
            assert!(bad.validate().is_err());
        }
    }
}
