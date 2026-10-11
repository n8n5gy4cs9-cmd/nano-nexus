//! Per-launch API token.

use secrecy::SecretString;

use crate::error::LlmError;

/// 24 random bytes, hex-encoded (48 chars). Never log it or send it to the webview or LAN clients.
pub fn new_token() -> Result<SecretString, LlmError> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|e| LlmError::Transport(format!("no randomness: {e}")))?;
    let mut s = String::with_capacity(48);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    Ok(SecretString::from(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn tokens_are_48_hex_and_unique() {
        let a = new_token().unwrap();
        let b = new_token().unwrap();
        assert_eq!(a.expose_secret().len(), 48);
        assert!(a.expose_secret().chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a.expose_secret(), b.expose_secret());
        assert!(!format!("{a:?}").contains(a.expose_secret()));
    }
}
