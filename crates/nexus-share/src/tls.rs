//! Optional HTTPS with a self-signed certificate (LAN-SHARE §3.6). The certificate is created once,
//! kept in the data folder, and re-made only when the list of names/addresses it must cover changes,
//! so its fingerprint stays stable for manual verification.

use std::path::Path;

use nexus_core::error::NexusError;
use sha2::{Digest, Sha256};

/// A certificate and key as PEM, plus the SHA-256 fingerprint shown to the user.
#[derive(Clone, Debug)]
pub struct TlsMaterial {
    pub cert_pem: String,
    pub key_pem: String,
    /// Uppercase hex pairs separated by `:`.
    pub fingerprint: String,
}

fn colon_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn io(e: std::io::Error) -> NexusError {
    NexusError::Io(e)
}

/// Load `share-cert.pem` / `share-key.pem` from `dir`, or create them for `names` (hosts and IPs).
pub fn load_or_create(dir: &Path, names: &[String]) -> Result<TlsMaterial, NexusError> {
    let (cert_f, key_f, names_f, fp_f) = (
        dir.join("share-cert.pem"),
        dir.join("share-key.pem"),
        dir.join("share-names.txt"),
        dir.join("share-cert.sha256"),
    );
    let mut sorted: Vec<String> = names.to_vec();
    sorted.sort();
    sorted.dedup();
    let wanted = sorted.join("\n");
    if std::fs::read_to_string(&names_f).ok().as_deref() == Some(wanted.as_str())
        && let (Ok(cert_pem), Ok(key_pem), Ok(fingerprint)) = (
            std::fs::read_to_string(&cert_f),
            std::fs::read_to_string(&key_f),
            std::fs::read_to_string(&fp_f),
        )
    {
        return Ok(TlsMaterial {
            cert_pem,
            key_pem,
            fingerprint: fingerprint.trim().to_owned(),
        });
    }
    let certified = rcgen::generate_simple_self_signed(sorted)
        .map_err(|e| NexusError::Internal(format!("could not create the HTTPS certificate: {e}")))?;
    let cert_pem = certified.cert.pem();
    let key_pem = certified.key_pair.serialize_pem();
    let fingerprint = colon_hex(&Sha256::digest(certified.cert.der().as_ref()));
    std::fs::create_dir_all(dir).map_err(io)?;
    std::fs::write(&cert_f, &cert_pem).map_err(io)?;
    std::fs::write(&key_f, &key_pem).map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&key_f, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::write(&names_f, &wanted).map_err(io)?;
    std::fs::write(&fp_f, &fingerprint).map_err(io)?;
    Ok(TlsMaterial {
        cert_pem,
        key_pem,
        fingerprint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificate_is_stable_until_the_names_change() {
        let dir = tempfile::tempdir().unwrap();
        let names = vec!["nanonexus.local".to_owned(), "192.168.1.5".to_owned()];
        let a = load_or_create(dir.path(), &names).unwrap();
        let b = load_or_create(dir.path(), &names).unwrap();
        assert_eq!(a.fingerprint, b.fingerprint);
        assert!(a.cert_pem.contains("BEGIN CERTIFICATE"));
        let c = load_or_create(dir.path(), &["other.local".to_owned()]).unwrap();
        assert_ne!(a.fingerprint, c.fingerprint);
        assert_eq!(a.fingerprint.split(':').count(), 32);
    }
}
