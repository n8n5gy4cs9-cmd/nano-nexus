//! `nexus-studio`: the image, speech-to-text and text-to-speech workers of the portable studio.
//!
//! Ported from the Portable Local Studio reference (MIT, see `workers/LICENSE-Portable-Local-Studio`):
//! stable-diffusion.cpp `sd-server`, whisper.cpp `whisper-cli` and the Kokoro Node worker. Differences
//! from the reference are deliberate: workers run with a cleared environment, bind loopback only, never
//! download anything, and write only below the directories the caller passes in. The engine binaries are
//! prebuilt files supplied by the DIST (UP.050 onward); nothing here installs or compiles them.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod error;
pub mod image;
pub mod outputs;
pub mod proc;
pub mod progress;
pub mod speech;
pub mod tts;

pub use error::StudioError;
