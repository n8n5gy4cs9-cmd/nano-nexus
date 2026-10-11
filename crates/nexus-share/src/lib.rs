//! LAN Share server (LAN-SHARE.md): pairing auth, WebSocket RPC, embedded web UI, graceful stop.
//! It never depends on Tauri; the app supplies a [`ShareApi`].
//!
//! Not built yet: Bonjour advertising (T14.08) and optional self-signed HTTPS (T14.09).

pub mod api;
pub mod assets;
pub mod auth;
pub mod rpc;
pub mod server;
pub mod urls;

pub use api::{Push, ShareApi, authorize};
pub use auth::{Auth, AuthConfig, AuthEvent};
pub use server::{ServerConfig, ShareServer, next_free_port};
