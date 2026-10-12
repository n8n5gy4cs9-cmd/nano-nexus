//! Share URLs, LAN address discovery, and the pairing QR (LAN-SHARE §2).

use std::net::{IpAddr, Ipv4Addr};

use qrcode::QrCode;
use qrcode::render::svg;

/// Private-range rank: 192.168/16 first, then 10/8, then 172.16/12, then anything else.
fn rank(ip: Ipv4Addr) -> u8 {
    let o = ip.octets();
    match (o[0], o[1]) {
        (192, 168) => 0,
        (10, _) => 1,
        (172, 16..=31) => 2,
        _ => 3,
    }
}

/// Non-loopback IPv4 addresses of interfaces that are up, best first.
pub fn lan_addresses() -> Vec<Ipv4Addr> {
    let mut found: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            IpAddr::V4(v4) if !v4.is_link_local() => Some(v4),
            _ => None,
        })
        .collect();
    found.sort_by_key(|ip| rank(*ip));
    found.dedup();
    found
}

/// This computer's own `.local` name (what macOS and Windows already answer for on the network),
/// e.g. `harris-macbook-pro`. `None` if it cannot be determined.
pub fn host_local_name() -> Option<String> {
    static NAME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        let run = |cmd: &str, args: &[&str]| {
            std::process::Command::new(cmd)
                .args(args)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
        };
        let raw = if cfg!(target_os = "macos") {
            run("scutil", &["--get", "LocalHostName"]).or_else(|| run("hostname", &["-s"]))
        } else {
            run("hostname", &[])
        }?;
        let name = crate::mdns::sanitize_name(raw.trim().trim_end_matches(".local"));
        (!name.is_empty()).then_some(name)
    })
    .clone()
}

/// Browser URLs for a server on `bind:port`. A specific bind shows only that address.
pub fn share_urls(bind: IpAddr, port: u16, https: bool, mdns_name: Option<&str>) -> Vec<String> {
    let scheme = if https { "https" } else { "http" };
    let hosts: Vec<String> = if bind.is_unspecified() {
        lan_addresses().into_iter().map(|i| i.to_string()).collect()
    } else {
        vec![bind.to_string()]
    };
    let mut urls: Vec<String> = hosts
        .into_iter()
        .map(|h| format!("{scheme}://{h}:{port}"))
        .collect();
    if bind.is_unspecified() {
        // The computer's own .local name works without any extra advertising, so list it first.
        if let Some(host) = host_local_name() {
            urls.push(format!("{scheme}://{host}.local:{port}"));
        }
        if let Some(name) = mdns_name
            && host_local_name().as_deref() != Some(name)
        {
            urls.push(format!("{scheme}://{name}.local:{port}"));
        }
    }
    urls
}

/// The URL the QR encodes: the secret rides in the fragment so it never reaches a server log.
pub fn pairing_url(base: &str, secret: &str) -> String {
    format!("{base}/pair#{secret}")
}

/// A QR code as an SVG string.
pub fn qr_svg(data: &str) -> Option<String> {
    let code = QrCode::new(data.as_bytes()).ok()?;
    Some(
        code.render::<svg::Color<'_>>()
            .min_dimensions(200, 200)
            .quiet_zone(true)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ranges_sort_first() {
        let mut v = [
            Ipv4Addr::new(8, 8, 8, 8),
            Ipv4Addr::new(10, 0, 0, 2),
            Ipv4Addr::new(192, 168, 1, 2),
        ];
        v.sort_by_key(|i| rank(*i));
        assert_eq!(v[0], Ipv4Addr::new(192, 168, 1, 2));
        assert_eq!(v[2], Ipv4Addr::new(8, 8, 8, 8));
    }

    #[test]
    fn specific_bind_shows_only_that_address() {
        let urls = share_urls(IpAddr::from([127, 0, 0, 1]), 9889, false, Some("nanonexus"));
        assert_eq!(urls, vec!["http://127.0.0.1:9889".to_owned()]);
    }

    #[test]
    fn pairing_url_uses_a_fragment() {
        assert_eq!(
            pairing_url("http://1.2.3.4:9889", "abc"),
            "http://1.2.3.4:9889/pair#abc"
        );
    }

    #[test]
    fn qr_is_an_svg() {
        assert!(
            qr_svg("http://192.168.1.20:9889/pair#abc")
                .unwrap()
                .contains("<svg")
        );
    }
}
