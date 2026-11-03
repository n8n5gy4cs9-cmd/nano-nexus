//! Provider/model profiles for coding sessions (UP.080).
//!
//! The local llama.cpp engine is the default and needs no consent. A cloud profile never sends anything
//! until the user has consented to that profile, and its secret lives in memory only (never serialized,
//! never in the USB database, never in Debug output). This module is pure policy: it makes no requests.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Id of the built-in local profile.
pub const LOCAL_PROFILE_ID: &str = "local";

/// Where the model runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// The pinned local llama.cpp engine on 127.0.0.1.
    Local,
    /// A remote HTTPS endpoint the user configured.
    Cloud,
}

/// What the model/endpoint has been declared to support. Declared by the user, not probed, so the UI
/// must say "declared" rather than "verified".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Capabilities {
    pub streaming: bool,
    /// Structured tool calls. Without this a coding session cannot run read/search/propose steps.
    pub tools: bool,
    pub vision: bool,
}

/// One saved profile. Contains no secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ProviderProfile {
    pub id: String,
    pub label: String,
    pub kind: ProviderKind,
    /// Empty for the local profile.
    pub base_url: String,
    pub model: String,
    pub capabilities: Capabilities,
    /// The user agreed that prompts and file excerpts of this profile's sessions leave this computer.
    pub network_consent: bool,
}

impl ProviderProfile {
    /// The always-present local default.
    pub fn local() -> Self {
        Self {
            id: LOCAL_PROFILE_ID.into(),
            label: "This computer (offline)".into(),
            kind: ProviderKind::Local,
            base_url: String::new(),
            model: String::new(),
            capabilities: Capabilities { streaming: true, tools: true, vision: false },
            network_consent: false,
        }
    }
}

/// Why a profile or a send was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProfileError {
    #[error("profile id must be 1-40 lowercase letters, digits or dashes")]
    BadId,
    #[error("profile name must not be empty")]
    EmptyLabel,
    #[error("a cloud profile needs an https:// address")]
    NotHttps,
    #[error("the address must not contain a user name, password, query or fragment; the key is entered separately")]
    CredentialsInUrl,
    #[error("a cloud profile needs a model name")]
    EmptyModel,
    #[error("the built-in local profile cannot be changed or removed")]
    LocalIsFixed,
    #[error("network use for this profile has not been allowed")]
    NoConsent,
    #[error("this profile is not declared to support tool calls, which coding sessions need")]
    NoTools,
    #[error("no key was entered for this profile in this session")]
    MissingSecret,
    #[error("unknown profile")]
    Unknown,
}

/// Check a profile before it is saved.
pub fn validate(p: &ProviderProfile) -> Result<(), ProfileError> {
    let id_ok = !p.id.is_empty()
        && p.id.len() <= 40
        && p.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !id_ok {
        return Err(ProfileError::BadId);
    }
    if p.label.trim().is_empty() {
        return Err(ProfileError::EmptyLabel);
    }
    match p.kind {
        ProviderKind::Local => {
            if p.id != LOCAL_PROFILE_ID {
                return Err(ProfileError::LocalIsFixed);
            }
        }
        ProviderKind::Cloud => {
            if p.id == LOCAL_PROFILE_ID {
                return Err(ProfileError::LocalIsFixed);
            }
            let rest = p.base_url.strip_prefix("https://").ok_or(ProfileError::NotHttps)?;
            let host_part = rest.split('/').next().unwrap_or("");
            if host_part.is_empty() || host_part.contains(char::is_whitespace) {
                return Err(ProfileError::NotHttps);
            }
            if host_part.contains('@') || rest.contains('?') || rest.contains('#') {
                return Err(ProfileError::CredentialsInUrl);
            }
            if p.model.trim().is_empty() {
                return Err(ProfileError::EmptyModel);
            }
        }
    }
    Ok(())
}

/// May a coding session send a request through this profile right now?
pub fn check_send(p: &ProviderProfile, has_secret: bool) -> Result<(), ProfileError> {
    if !p.capabilities.tools {
        return Err(ProfileError::NoTools);
    }
    if p.kind == ProviderKind::Cloud {
        if !p.network_consent {
            return Err(ProfileError::NoConsent);
        }
        if !has_secret {
            return Err(ProfileError::MissingSecret);
        }
    }
    Ok(())
}

/// In-memory secrets, keyed by profile id. Dropped when the app exits; never written anywhere.
#[derive(Default)]
pub struct SecretStore(Mutex<HashMap<String, String>>);

impl std::fmt::Debug for SecretStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretStore(<redacted>)")
    }
}

impl SecretStore {
    pub fn set(&self, id: &str, secret: String) {
        if let Ok(mut m) = self.0.lock() {
            m.insert(id.to_owned(), secret);
        }
    }

    pub fn has(&self, id: &str) -> bool {
        self.0.lock().is_ok_and(|m| m.get(id).is_some_and(|s| !s.is_empty()))
    }

    pub fn get(&self, id: &str) -> Option<String> {
        self.0.lock().ok().and_then(|m| m.get(id).cloned())
    }

    pub fn clear(&self, id: &str) {
        if let Ok(mut m) = self.0.lock() {
            m.remove(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud(url: &str) -> ProviderProfile {
        ProviderProfile {
            id: "acme".into(),
            label: "Acme".into(),
            kind: ProviderKind::Cloud,
            base_url: url.into(),
            model: "m1".into(),
            capabilities: Capabilities { streaming: true, tools: true, vision: false },
            network_consent: false,
        }
    }

    #[test]
    fn local_default_is_valid_and_needs_no_consent() {
        let p = ProviderProfile::local();
        assert_eq!(validate(&p), Ok(()));
        assert_eq!(check_send(&p, false), Ok(()));
    }

    #[test]
    fn cloud_addresses_must_be_plain_https() {
        assert_eq!(validate(&cloud("https://api.example.com/v1")), Ok(()));
        assert_eq!(validate(&cloud("http://api.example.com")), Err(ProfileError::NotHttps));
        assert_eq!(validate(&cloud("https://u:p@api.example.com")), Err(ProfileError::CredentialsInUrl));
        assert_eq!(validate(&cloud("https://api.example.com/v1?key=x")), Err(ProfileError::CredentialsInUrl));
    }

    #[test]
    fn cloud_send_needs_consent_key_and_tools() {
        let mut p = cloud("https://api.example.com");
        assert_eq!(check_send(&p, true), Err(ProfileError::NoConsent));
        p.network_consent = true;
        assert_eq!(check_send(&p, false), Err(ProfileError::MissingSecret));
        assert_eq!(check_send(&p, true), Ok(()));
        p.capabilities.tools = false;
        assert_eq!(check_send(&p, true), Err(ProfileError::NoTools));
    }

    #[test]
    fn secrets_stay_in_memory_and_out_of_debug() {
        let s = SecretStore::default();
        s.set("acme", "sk-123".into());
        assert!(s.has("acme"));
        assert!(!format!("{s:?}").contains("sk-123"));
        s.clear("acme");
        assert!(!s.has("acme"));
    }
}
