//! Free-port discovery and bind-failure detection (INFERENCE-CONTRACT §1).

use std::net::TcpListener;

/// Ask the OS for a free loopback port. The listener is dropped before the child binds, so a race
/// is possible; the supervisor retries (max 16 attempts).
pub fn find_free_port() -> std::io::Result<u16> {
    let l = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(l.local_addr()?.port())
}

/// Does this log line say the server could not bind its port?
pub fn is_bind_error(line: &str) -> bool {
    let l = line.to_lowercase();
    l.contains("address already in use")
        || l.contains("couldn't bind")
        || l.contains("could not bind")
        || l.contains("failed to bind")
        || l.contains("bind failed")
        || (l.contains("bind") && l.contains("error"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_port_is_bindable() {
        let p = find_free_port().unwrap();
        assert!(p > 0);
        assert!(TcpListener::bind(("127.0.0.1", p)).is_ok());
    }

    #[test]
    fn detects_bind_errors() {
        assert!(is_bind_error(
            "main: couldn't bind HTTP server socket, hostname: 127.0.0.1, port: 8080"
        ));
        assert!(is_bind_error("Error: Address already in use (os error 48)"));
        assert!(!is_bind_error(
            "llama_model_loader: loaded meta data with 25 key-value pairs"
        ));
    }
}
