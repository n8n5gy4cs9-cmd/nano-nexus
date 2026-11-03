/** Studio IPC shapes; Rust owns the schema. Bindings regenerate only in an approved build. */
export const MODEL_KINDS = ["chat", "image", "speech", "tts"] as const;
export type ModelKind = (typeof MODEL_KINDS)[number];
export function isModelKind(value: string): value is ModelKind {
  return MODEL_KINDS.some((kind) => kind === value);
}
export interface StudioAsset { sha256: string; path: string; bytes: number }
export interface StudioModel {
  id: string;
  name: string;
  kind: ModelKind;
  variant: string;
  metadata: string;
  assets: { role: string; asset: StudioAsset }[];
}
export interface StudioModelView { model: StudioModel; available: boolean; selected: boolean }
export interface StudioModelInput { id: string; name: string; kind: ModelKind; role: string; variant: string }
export interface StudioProgress {
  bytes: number;
  total: number;
  bytes_per_sec: number;
  eta_secs: number | null;
  phase: "preparing" | "downloading" | "verifying" | "finalizing";
}

export interface StudioMigrationResult { imported: string[]; skipped: string[]; failed: string[] }
export interface StudioOrphan { sha256: string; path: string; bytes: number; present: boolean }
export interface StudioOrphanPreview { assets: StudioOrphan[]; total_bytes: number }
export interface StudioReclaimResult { reclaimed: string[]; bytes: number; skipped: string[]; failed: string[] }
