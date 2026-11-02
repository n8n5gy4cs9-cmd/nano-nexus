# Reference capability inventory

Source audit: 2026-11-02. This records shipped source behavior, not verified runtime behavior.
Source: `REFE/Portable-Local-Studio-main`; no reference code has been executed.

## Workspaces and behavior

| Capability | Source | Conversion task / preservation rule |
|---|---|---|
| Streaming/non-streaming chat, persistent conversations, Markdown/math/code, attachments and optional vision companion | TextChat.jsx, server chat handlers | UP.040; preserve NanoNexus SQLite history/stream cancellation rather than parallel JSON history |
| Per-model inference settings, thinking toggle, context/output tokens, sampling, threads, GPU layers, benchmark history | Settings.jsx, TextChat.jsx, llm handlers | UP.040/045; benchmark UI may be ported but never run under current no-testing instruction |
| Prompt/negative prompt, image input, sizes/aspect ratios, steps, sampler, CFG, seed, batch and progress | Generator.jsx, Settings.jsx, generation proxy | UP.041; validate backend-specific parameter limits |
| Gallery, prompt/parameter reuse, image preview/download/save/delete and metadata | Generator.jsx, output handlers | UP.042; export/delete through existing Journal contract |
| Microphone/file transcription, language/model/settings, saved transcripts and deletion | SpeechTranscriber.jsx, speech handlers | UP.043; permission-denied/device-missing and cancellation states required |
| Text-to-speech model/voice/speed, saved clips, playback speed, word highlighting, truncation warning | TextToSpeech.jsx, tts-kokoro-worker.mjs | UP.044; distinguish estimated versus measured word timing |
| Catalog/HF search/filter/pagination/URL download; local file picker/drop/import, link, delete, reclassify | ModelManager.jsx, model-kind.cjs, api.js | UP.030–033; external links conflict with self-contained USB requirement: offer import into shared store instead |
| Engine status/start/stop/restart, backend downloads and selection, hardware recommendations | server, llm-backends.json | UP.045; signed/pinned runtime artifacts only, no source-build fallback on target |
| CPU/RAM/GPU/VRAM live telemetry and model fit suggestions | TopStatusBar.jsx, server | UP.045/046; unavailable measurements shown as unavailable, never zero |
| Theme selection and workspace preferences | themes.js, App.jsx, Settings.jsx | UP.046; adapt NanoNexus tokens/accessibility, preserve persisted preferences |
| Search providers, result/content retrieval and caching | scripts/search/*.js | UP.047; opt-in network, bounded responses and existing SSRF defenses |
| Diagnostics, cleanup candidates, reset/update/setup | scripts/server and scripts/setup/reset | UP.055/070/071; no destructive reset/update scripts copied unchanged |

## Model and backend matrix

| Kind | Accepted reference assets | Backends/platform paths | Constraint |
|---|---|---|---|
| Chat | GGUF; optional matching mmproj | llama.cpp CPU, Metal, CUDA, Vulkan, HIP/ROCm, SYCL | Require exact compatible engine/runtime manifest; CPU mandatory on Windows |
| Image | complete SD1.5/SDXL safetensors/ckpt; limited complete GGUF | stable-diffusion.cpp CPU/GPU, optional OpenVINO/CoreML workers | LoRA/ControlNet/VAE/text-encoder-only files are not standalone checkpoints; never load pickle checkpoint via Python |
| Speech | Whisper GGML .bin | whisper.cpp private CLI | Decode uploaded audio using bundled components; no host ffmpeg requirement |
| Voice | Kokoro JSON/ONNX/voice assets | private Node/ONNX worker in reference | Runtime and every lazy asset must be prepared in DIST; disable implicit remote fetch |
| NPU variants | converted model directories | OpenVINO Intel NPU, CoreML Apple Silicon | Distinct derived assets; driver/OS support required, no host setup |

The reference backend catalog names Windows CUDA/Vulkan/SYCL/HIP/CPU, Linux CUDA/ROCm/Vulkan/SYCL/CPU, and macOS arm64/x64. These are catalog choices, not compatibility evidence. NanoNexus initially prioritizes Apple Silicon and Windows x64. Unsupported platform paths remain visibly pending, not claimed as shipped. Single-file workflows only: Flux/HiDream/Hunyuan/Wan/Qwen Image/Z-Image multi-component workflows are explicitly unsupported by the reference README.

## License and supply-chain inventory

| Component | Local evidence / license handling |
|---|---|
| Reference implementation | Root LICENSE: MIT, retain copyright and permission notice for copied/adapted code |
| Frontend npm tree | package.json includes React, Tauri APIs, lucide-react, KaTeX, PDF.js; resolve exact lockfile versions and collect actual transitive notices before distribution (UP.053) |
| llama.cpp / whisper.cpp / stable-diffusion.cpp | Setup scripts fetch upstream engines; collect each pinned artifact's LICENSE and dependency notices; reference README does not substitute for artifact license audit |
| Kokoro / ONNX / private Node | setup-tts scripts install runtime packages; pin exact versions and include runtime/package/model/voice notices before shipping |
| OpenVINO / CoreML / Python stack | setup scripts install Python packages including torch/diffusers/transformers; optional runtime bundles require independent license and size audit |
| Models / voices | Model licenses are artifact-specific, potentially gated or restrictive; preserve repo/revision/license metadata and require user acknowledgement where necessary |
| WebView2 | Private fixed-runtime redistribution terms/notices must accompany Windows payload; no bootstrapper/host install |

Do not replace unknown artifact licenses with guessed SPDX strings. Missing license evidence blocks packaging that artifact, not unrelated source work. Existing NanoNexus runtime pin is distinct from the reference pin; no silent engine upgrade.

## Complete explicit HTTP dispatch surface

Each explicit reference dispatcher branch is assigned below. Generation proxy families remain families because the reference forwards their subpaths dynamically. Static UI and output-asset serving are covered by their owning workspace tasks.

| Reference route/prefix | Owning task |
|---|---|
| `/v1/` | UP.045 |
| `/api/health` | UP.070–071 / UP.055 |
| `/api/diagnostics` | UP.070–071 / UP.055 |
| `/api/cleanup-candidates` | UP.070–071 / UP.055 |
| `/api/cleanup` | UP.070–071 / UP.055 |
| `/api/backend-status` | UP.045 |
| `/api/llm/status` | UP.045 |
| `/api/llm/backends` | UP.045 |
| `/api/llm/stats` | UP.045 |
| `/api/llm/model-settings` | UP.040 |
| `/api/llm/conversations` | UP.040 |
| `/api/llm/save-conversation` | UP.040 |
| `/api/llm/delete-conversation` | UP.040 |
| `/api/llm/models` | UP.030–033 |
| `/api/huggingface/models` | UP.030–033 |
| `/api/llm/start` | UP.045 |
| `/api/llm/stop` | UP.045 |
| `/api/llm/benchmark` | UP.045 |
| `/api/speech/status` | UP.043 |
| `/api/speech/models` | UP.043 |
| `/api/speech/start` | UP.043 |
| `/api/speech/stop` | UP.043 |
| `/api/speech/transcribe` | UP.043 |
| `/api/speech/download-model` | UP.043 |
| `/api/speech/import-model` | UP.043 |
| `/api/speech/delete-model` | UP.043 |
| `/api/speech/transcriptions` | UP.043 |
| `/api/speech/delete-transcription` | UP.043 |
| `/api/tts/status` | UP.044 |
| `/api/tts/models` | UP.044 |
| `/api/tts/start` | UP.044 |
| `/api/tts/stop` | UP.044 |
| `/api/tts/speak` | UP.044 |
| `/api/tts/download-model` | UP.044 |
| `/api/tts/import-model` | UP.044 |
| `/api/tts/delete-model` | UP.044 |
| `/api/tts/outputs` | UP.044 |
| `/api/tts/delete-output` | UP.044 |
| `/api/web-search/config` | UP.047 |
| `/api/web-search` | UP.047 |
| `/api/llm/chat` | UP.040 |
| `/api/llm/recommend` | UP.045 |
| `/api/hardware-specs` | UP.045 |
| `/api/backend-options` | UP.045 |
| `/api/telemetry` | UP.046 |
| `/api/models` | UP.030–033 |
| `/api/openvino-models` | UP.030–033 |
| `/api/restart-backend` | UP.045 |
| `/api/stop-backend` | UP.045 |
| `/api/download-backend` | UP.045 |
| `/api/download-model` | UP.030–033 |
| `/api/llm/download-model` | UP.030–033 |
| `/api/download-openvino-model` | UP.045 |
| `/api/download-progress` | UP.030–033 |
| `/api/cancel-download` | UP.030–033 |
| `/api/generation-progress` | UP.041 |
| `/api/openvino-generate` | UP.041 |
| `/api/outputs` | UP.042 |
| `/api/output-file` | UP.042 |
| `/api/save-output` | UP.042 |
| `/api/delete-outputs` | UP.042 |
| `/api/import-model` | UP.030–033 |
| `/api/llm/import-model` | UP.030–033 |
| `/api/llm/delete-model` | UP.030–033 |
| `/api/move-model` | UP.030–033 |
| `/api/pick-model-file` | UP.030–033 |
| `/api/link-model` | UP.030–033 |
| `/api/delete-model` | UP.030–033 |
| `/api/` | UP.045 |

## Setup behavior to replace

Reference launchers bootstrap private Node and engines, may prompt for optional acceleration, and expose a localhost browser UI. Setup scripts include npm/pip installation, model downloads, runtime updates, CoreML/OpenVINO setup and optional source builds. Retain capabilities through prebuilt private bundles and explicit model/runtime download jobs to USB; prohibit target-side package installation, unpinned updates and automatic compilation. Cleanup must never remove models, outputs or unrelated user environments. No reference reset script is directly reusable.

## Acceptance mapping

Every workspace inherits validation, bounded jobs/uploads, cancellation, secret redaction, empty/loading/error states, compatibility messages and offline behavior from update-plan.md. New IPC must use the Transport facade and Rust services. Current audit completeness is source-surface coverage; native runtime correctness, exact dependency licenses and actual hardware support remain unverified and assigned to delivery tasks.
