//! LAN Share types shared by the store, the server, and the desktop UI (LAN-SHARE §5, §6).

use serde::{Deserialize, Serialize};

use crate::settings::SharePolicy;

/// A paired LAN device (the token itself is never stored or shown, only its hash is kept).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ShareDevice {
    pub id: String,
    pub name: String,
    pub policy: SharePolicy,
    pub user_agent: Option<String>,
    pub last_ip: Option<String>,
    pub created_at: i64,
    pub last_seen_at: Option<i64>,
    pub expires_at: i64,
}

/// A fresh pairing code: the QR secret (URL fragment) and the 6-digit PIN.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct PairingCode {
    pub secret: String,
    pub pin: String,
    /// Unix ms when this code stops working.
    pub expires_at: i64,
}

/// What the desktop shows about sharing (`share_status`, event `share:status`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ShareStatus {
    pub running: bool,
    pub port: u16,
    /// `http(s)://<ip>:<port>` for each LAN address (private ranges first), then the Bonjour name.
    pub urls: Vec<String>,
    pub devices: u32,
    pub https_fingerprint: Option<String>,
    /// Set when the last start failed (`port_in_use` and so on) with a human-readable reason.
    pub error: Option<String>,
    /// When the port is taken, the next free port to offer.
    pub suggested_port: Option<u16>,
}

/// Name of the cookie that carries the device token.
pub const SHARE_COOKIE: &str = "nexus_device";
/// Header (or `csrf` query parameter on the WebSocket upgrade) that must echo the CSRF token.
pub const SHARE_CSRF_HEADER: &str = "x-nexus-csrf";
