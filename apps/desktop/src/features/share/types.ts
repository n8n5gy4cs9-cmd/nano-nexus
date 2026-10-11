// Hand-written mirrors of the Rust share types (kept out of the generated bindings.ts).
export type SharePolicy = "chat_only" | "chat_read" | "full";

export interface ShareDevice {
  id: string;
  name: string;
  policy: SharePolicy;
  user_agent: string | null;
  last_ip: string | null;
  created_at: number;
  last_seen_at: number | null;
  expires_at: number;
}

export interface ShareStatus {
  running: boolean;
  port: number;
  urls: string[];
  devices: number;
  https_fingerprint: string | null;
  error: string | null;
  suggested_port: number | null;
}

export interface PairingCode {
  secret: string;
  pin: string;
  expires_at: number;
}

export interface PairingView {
  code: PairingCode;
  url: string;
  qr_svg: string;
}
