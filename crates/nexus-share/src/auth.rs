//! Pairing, PIN rate limiting, device sessions, and CSRF tokens (LAN-SHARE §3).
//!
//! Tokens are random and only their SHA-256 is stored. Pairing codes are single-use and expire
//! after five minutes. Wrong guesses are limited per client IP.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use nexus_core::error::NexusError;
use nexus_core::settings::SharePolicy;
use nexus_core::share::{PairingCode, ShareDevice};
use nexus_store::Store;
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

/// How long a pairing code works.
pub const PAIRING_TTL: Duration = Duration::from_secs(5 * 60);
/// Wrong guesses allowed per window before the lockout.
pub const MAX_ATTEMPTS: u32 = 5;
/// Length of the attempt window.
pub const ATTEMPT_WINDOW: Duration = Duration::from_secs(60);
/// How long an IP is locked out after too many wrong guesses.
pub const LOCKOUT: Duration = Duration::from_secs(10 * 60);

/// Settings the auth layer follows (updated when the user changes them).
#[derive(Clone, Debug)]
pub struct AuthConfig {
    pub default_policy: SharePolicy,
    pub session_days: u32,
    pub max_devices: u32,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            default_policy: SharePolicy::ChatRead,
            session_days: 30,
            max_devices: 5,
        }
    }
}

/// Things the desktop wants to hear about.
#[derive(Clone, Debug)]
pub enum AuthEvent {
    /// A device paired.
    Paired(ShareDevice),
    /// An IP was locked out after too many wrong codes.
    LockedOut(IpAddr),
    /// A device was revoked (its sockets must close).
    Revoked(String),
}

struct Pairing {
    secret: String,
    pin: String,
    expires: Instant,
}

#[derive(Default)]
struct Attempts {
    failures: u32,
    window_start: Option<Instant>,
    locked_until: Option<Instant>,
}

#[derive(Default)]
struct Inner {
    pairing: Option<Pairing>,
    attempts: HashMap<IpAddr, Attempts>,
}

/// The auth state of one share server.
pub struct Auth {
    store: Arc<Store>,
    inner: Mutex<Inner>,
    config: RwLock<AuthConfig>,
    events: broadcast::Sender<AuthEvent>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Base64url without padding.
pub fn base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(B64[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(B64[n as usize & 63] as char);
        }
    }
    out
}

/// Lowercase hex SHA-256.
pub fn sha256_hex(input: &str) -> String {
    Sha256::digest(input.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Compare without leaking where the first difference is.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

fn random_bytes<const N: usize>() -> Result<[u8; N], NexusError> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|e| NexusError::Internal(format!("no randomness: {e}")))?;
    Ok(bytes)
}

/// The CSRF token that belongs to a device token (double-submit; recomputed, never stored).
pub fn csrf_for(token: &str) -> String {
    sha256_hex(&format!("nexus-csrf:{token}"))
}

impl Auth {
    pub fn new(store: Arc<Store>, config: AuthConfig) -> Arc<Self> {
        Arc::new(Self {
            store,
            inner: Mutex::new(Inner::default()),
            config: RwLock::new(config),
            events: broadcast::channel(32).0,
        })
    }

    /// Follow changed settings.
    pub fn set_config(&self, config: AuthConfig) {
        if let Ok(mut c) = self.config.write() {
            *c = config;
        }
    }

    pub fn config(&self) -> AuthConfig {
        self.config.read().map(|c| c.clone()).unwrap_or_default()
    }

    /// Subscribe to pairing, lockout, and revoke events.
    pub fn subscribe(&self) -> broadcast::Receiver<AuthEvent> {
        self.events.subscribe()
    }

    /// Create (or rotate) the pairing secret and PIN. The previous code stops working.
    pub fn new_pairing_code(&self) -> Result<PairingCode, NexusError> {
        let secret = base64url(&random_bytes::<32>()?);
        let n = u32::from_le_bytes(random_bytes::<4>()?) % 1_000_000;
        let pin = format!("{n:06}");
        let expires_ms = now_ms() + PAIRING_TTL.as_millis() as i64;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| NexusError::Internal("auth lock".into()))?;
        inner.pairing = Some(Pairing {
            secret: secret.clone(),
            pin: pin.clone(),
            expires: Instant::now() + PAIRING_TTL,
        });
        Ok(PairingCode {
            secret,
            pin,
            expires_at: expires_ms,
        })
    }

    /// Forget the pairing code (sharing stopped).
    pub fn clear_pairing(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.pairing = None;
        }
    }

    /// Pair a device with the QR secret or the PIN. Returns the device and its **token** (shown once).
    pub fn pair(
        &self,
        code: &str,
        name: &str,
        ip: IpAddr,
        user_agent: Option<&str>,
    ) -> Result<(ShareDevice, String), NexusError> {
        self.pair_at(code, name, ip, user_agent, Instant::now())
    }

    pub(crate) fn pair_at(
        &self,
        code: &str,
        name: &str,
        ip: IpAddr,
        user_agent: Option<&str>,
        now: Instant,
    ) -> Result<(ShareDevice, String), NexusError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 60 {
            return Err(NexusError::InvalidInput(
                "name a device with 1-60 characters".into(),
            ));
        }
        let code = code.trim();
        {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| NexusError::Internal("auth lock".into()))?;
            let attempts = inner.attempts.entry(ip).or_default();
            if let Some(until) = attempts.locked_until {
                if now < until {
                    return Err(NexusError::Busy(
                        "too many wrong codes; try again in a few minutes".into(),
                    ));
                }
                *attempts = Attempts::default();
            }
            let valid = match &inner.pairing {
                Some(p) if now < p.expires => {
                    // Evaluate both so timing does not reveal which one matched.
                    let by_secret = constant_time_eq(code, &p.secret);
                    let by_pin = constant_time_eq(code, &p.pin);
                    by_secret || by_pin
                }
                _ => false,
            };
            if !valid {
                let attempts = inner.attempts.entry(ip).or_default();
                if attempts
                    .window_start
                    .is_none_or(|s| now.duration_since(s) > ATTEMPT_WINDOW)
                {
                    attempts.window_start = Some(now);
                    attempts.failures = 0;
                }
                attempts.failures += 1;
                if attempts.failures >= MAX_ATTEMPTS {
                    attempts.locked_until = Some(now + LOCKOUT);
                    let _ = self.events.send(AuthEvent::LockedOut(ip));
                }
                return Err(NexusError::Unauthorized);
            }
            // Single use: consume the code before anything can fail.
            inner.pairing = None;
            inner.attempts.remove(&ip);
        }
        let config = self.config();
        if self.store.count_share_devices()? >= config.max_devices {
            return Err(NexusError::Conflict(format!(
                "the limit of {} paired devices is reached; revoke one on the desktop",
                config.max_devices
            )));
        }
        let token = base64url(&random_bytes::<32>()?);
        let created = now_ms();
        let device = ShareDevice {
            id: uuid::Uuid::now_v7().to_string(),
            name: name.to_owned(),
            policy: config.default_policy,
            user_agent: user_agent.map(|u| u.chars().take(200).collect()),
            last_ip: Some(ip.to_string()),
            created_at: created,
            last_seen_at: Some(created),
            expires_at: created + i64::from(config.session_days) * 86_400_000,
        };
        self.store.insert_share_device(&device, &sha256_hex(&token))?;
        let _ = self.events.send(AuthEvent::Paired(device.clone()));
        Ok((device, token))
    }

    /// The device this token belongs to, if the session is still valid.
    pub fn authenticate(&self, token: &str, ip: Option<IpAddr>) -> Option<ShareDevice> {
        if token.is_empty() || token.len() > 128 {
            return None;
        }
        let device = self
            .store
            .share_device_by_hash(&sha256_hex(token))
            .ok()
            .flatten()?;
        if let Err(e) = self
            .store
            .touch_share_device(&device.id, ip.map(|i| i.to_string()).as_deref())
        {
            tracing::warn!("could not record device activity: {e}");
        }
        Some(device)
    }

    /// Is this device still paired? (Checked per RPC so a revoke takes effect immediately.)
    pub fn is_alive(&self, device_id: &str) -> bool {
        matches!(self.store.share_device(device_id), Ok(Some(_)))
    }

    /// The current record of a device (policy changes show up here).
    pub fn device(&self, device_id: &str) -> Option<ShareDevice> {
        self.store.share_device(device_id).ok().flatten()
    }

    /// Revoke a device. Open sockets of that device are told to close.
    pub fn revoke(&self, device_id: &str) -> Result<bool, NexusError> {
        let removed = self.store.revoke_share_device(device_id)?;
        if removed {
            let _ = self.events.send(AuthEvent::Revoked(device_id.to_owned()));
        }
        Ok(removed)
    }

    /// The device list for the desktop UI.
    pub fn devices(&self) -> Result<Vec<ShareDevice>, NexusError> {
        Ok(self.store.list_share_devices()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth() -> Arc<Auth> {
        Auth::new(Arc::new(Store::open_in_memory().unwrap()), AuthConfig::default())
    }

    fn ip(n: u8) -> IpAddr {
        IpAddr::from([192, 168, 1, n])
    }

    #[test]
    fn base64url_matches_known_vectors() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn secret_pairs_once() {
        let a = auth();
        let code = a.new_pairing_code().unwrap();
        let (device, token) = a.pair(&code.secret, "Kim's iPhone", ip(5), None).unwrap();
        assert_eq!(a.authenticate(&token, Some(ip(5))).unwrap().id, device.id);
        // Second use fails: the code was consumed.
        assert!(matches!(
            a.pair(&code.secret, "Other", ip(6), None),
            Err(NexusError::Unauthorized)
        ));
    }

    #[test]
    fn pin_pairs_and_only_hashes_are_stored() {
        let a = auth();
        let code = a.new_pairing_code().unwrap();
        assert_eq!(code.pin.len(), 6);
        let (_, token) = a.pair(&code.pin, "iPad", ip(7), None).unwrap();
        assert!(
            a.store.share_device_by_hash(&token).unwrap().is_none(),
            "plain token must not be stored"
        );
        assert!(
            a.store
                .share_device_by_hash(&sha256_hex(&token))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn expired_code_is_refused() {
        let a = auth();
        let code = a.new_pairing_code().unwrap();
        let later = Instant::now() + PAIRING_TTL + Duration::from_secs(1);
        assert!(matches!(
            a.pair_at(&code.secret, "x", ip(5), None, later),
            Err(NexusError::Unauthorized)
        ));
    }

    #[test]
    fn rotating_invalidates_the_old_code() {
        let a = auth();
        let first = a.new_pairing_code().unwrap();
        let _second = a.new_pairing_code().unwrap();
        assert!(a.pair(&first.secret, "x", ip(5), None).is_err());
    }

    #[test]
    fn wrong_codes_lock_the_ip_out() {
        let a = auth();
        let mut rx = a.subscribe();
        let code = a.new_pairing_code().unwrap();
        let t0 = Instant::now();
        for _ in 0..MAX_ATTEMPTS {
            assert!(matches!(
                a.pair_at("000000x", "x", ip(9), None, t0),
                Err(NexusError::Unauthorized)
            ));
        }
        // Even the right code is refused while locked out.
        assert!(matches!(
            a.pair_at(&code.pin, "x", ip(9), None, t0 + Duration::from_secs(5)),
            Err(NexusError::Busy(_))
        ));
        assert!(matches!(rx.try_recv(), Ok(AuthEvent::LockedOut(_))));
        // Another IP is unaffected, and the lockout ends.
        assert!(a.pair_at(&code.pin, "x", ip(10), None, t0).is_ok());
        let b = auth();
        let c2 = b.new_pairing_code().unwrap();
        for _ in 0..MAX_ATTEMPTS {
            let _ = b.pair_at("nope", "x", ip(9), None, t0);
        }
        // Still locked out a second later ...
        assert!(matches!(
            b.pair_at(&c2.pin, "x", ip(9), None, t0 + Duration::from_secs(1)),
            Err(NexusError::Busy(_))
        ));
        // ... and free after LOCKOUT. The pairing code itself has expired by then (PAIRING_TTL < LOCKOUT),
        // so the answer is Unauthorized again, not Busy.
        assert!(matches!(
            b.pair_at(&c2.pin, "x", ip(9), None, t0 + LOCKOUT + Duration::from_secs(1)),
            Err(NexusError::Unauthorized)
        ));
    }

    #[test]
    fn revoked_device_stops_authenticating() {
        let a = auth();
        let mut rx = a.subscribe();
        let code = a.new_pairing_code().unwrap();
        let (device, token) = a.pair(&code.secret, "Phone", ip(5), None).unwrap();
        assert!(a.revoke(&device.id).unwrap());
        assert!(a.authenticate(&token, None).is_none());
        assert!(!a.is_alive(&device.id));
        assert!(matches!(rx.try_recv(), Ok(AuthEvent::Paired(_))));
        assert!(matches!(rx.try_recv(), Ok(AuthEvent::Revoked(_))));
    }

    #[test]
    fn device_limit_is_enforced() {
        let a = auth();
        a.set_config(AuthConfig {
            max_devices: 1,
            ..AuthConfig::default()
        });
        let c = a.new_pairing_code().unwrap();
        a.pair(&c.secret, "one", ip(5), None).unwrap();
        let c = a.new_pairing_code().unwrap();
        assert!(matches!(
            a.pair(&c.secret, "two", ip(6), None),
            Err(NexusError::Conflict(_))
        ));
    }

    #[test]
    fn csrf_is_derived_from_the_token() {
        assert_eq!(csrf_for("abc"), csrf_for("abc"));
        assert_ne!(csrf_for("abc"), csrf_for("abd"));
        assert!(constant_time_eq("a", "a") && !constant_time_eq("a", "ab"));
    }
}
