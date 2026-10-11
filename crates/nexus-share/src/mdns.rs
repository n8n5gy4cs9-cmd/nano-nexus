//! Bonjour advertising (`nanonexus.local`) so phones can use a name instead of an IP (LAN-SHARE §2).

use std::collections::HashMap;

use mdns_sd::{ServiceDaemon, ServiceInfo};

/// A running advertisement. Dropping it (or [`Advertiser::stop`]) withdraws the name.
pub struct Advertiser {
    daemon: ServiceDaemon,
}

/// Host labels allowed in a `.local` name: lowercase letters, digits and `-`, at most 63 characters.
pub fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    let short: String = trimmed.chars().take(63).collect();
    if short.is_empty() { "nanonexus".into() } else { short }
}

impl Advertiser {
    /// Advertise `_http._tcp` on all interfaces as `<name>.local`.
    pub fn start(name: &str, port: u16) -> Result<Self, String> {
        let name = sanitize_name(name);
        let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
        let host = format!("{name}.local.");
        let info = ServiceInfo::new("_http._tcp.local.", &name, &host, "", port, None::<HashMap<String, String>>)
            .map_err(|e| e.to_string())?
            .enable_addr_auto();
        daemon.register(info).map_err(|e| e.to_string())?;
        Ok(Self { daemon })
    }

    /// Withdraw the advertisement.
    pub fn stop(self) {
        let _ = self.daemon.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_dns_safe() {
        assert_eq!(sanitize_name("NanoNexus"), "nanonexus");
        assert_eq!(sanitize_name("  Kim's Mac! "), "kim-s-mac");
        assert_eq!(sanitize_name("***"), "nanonexus");
        assert_eq!(sanitize_name(&"a".repeat(100)).len(), 63);
    }
}
