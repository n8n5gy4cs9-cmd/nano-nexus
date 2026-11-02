/** Media IPC shapes; Rust (`service/media.rs`, `nexus-studio`) owns the schema. Bindings regenerate only in an approved build. */
export type OutputKind = "images" | "audio" | "transcripts";
export type ImageBackend = "cpu" | "vulkan" | "cuda" | "rocm" | "metal";
export type ImageFamily = "sd15" | "sdxl";
export const SAMPLERS = ["euler", "euler_a", "heun", "dpm2", "dpm++2m", "lcm"] as const;

export interface LoadState { active: boolean; phase: string; progress: number; current: number; total: number; speed: string }
export interface GenState { active: boolean; step: number; steps: number; speed: string; decoding: boolean }
export interface ImageStatus { running: boolean; ready: boolean; load: LoadState; generation: GenState; device: string; error: string | null }
export interface EngineInfo { name: string; installed: boolean; variants: string[] }
export interface MediaStatus {
  platform: string;
  engines: EngineInfo[];
  image: ImageStatus;
  busy: string | null;
  recommended_backend: string | null;
  notes: string[];
}
export interface ImageStartInput {
  backend: ImageBackend | null;
  family: ImageFamily;
  steps: number;
  cfg_scale: number;
  sampler: string;
  threads: number | null;
  flash_attn: boolean;
  vae_tiling: boolean;
  vae_on_cpu: boolean;
}
export interface ImageRequest {
  prompt: string;
  negative_prompt: string;
  width: number;
  height: number;
  steps: number;
  cfg_scale: number;
  seed: number | null;
  batch: number;
  sampler: string;
}
export interface ImageGenerateInput { request: ImageRequest; family: ImageFamily; init_image: string | null; denoising_strength: number | null }
export interface OutputRecord { file: string; metadata: string; created_at: string; details: Record<string, unknown> }
export interface TranscribeInput { path: string; language: string; translate_to_english: boolean; threads: number | null; save: boolean }
export interface TranscribeOutput { text: string; duration_ms: number; saved: OutputRecord | null }
export interface VoiceInfo { id: string; name: string }
export interface SpeakRequest { text: string; voice: string; speed: number }
export interface TtsProgress { phase: string; done: number; total: number }
