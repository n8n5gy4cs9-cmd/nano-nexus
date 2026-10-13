//! Web access for the `search_web` and `fetch_url` tools (ADR-016).
//!
//! Off unless the user turned on Settings → Tools → "Allow web access". The fetcher refuses anything
//! that is not plain `http(s)` to a public address (so a page can never make the app talk to
//! `llama-server` on 127.0.0.1 or to the user's LAN), re-checks every redirect, pins the DNS answer it
//! checked, and caps time and size. Pages come back as plain text, never as markup.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use regex::Regex;
use reqwest::Url;

const USER_AGENT: &str = "Mozilla/5.0 (compatible; NanoNexus/1.0; +local assistant)";
const MAX_BODY_BYTES: usize = 1_500_000;
const MAX_REDIRECTS: usize = 4;
const TIMEOUT: Duration = Duration::from_secs(12);

/// One search result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// A page reduced to text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub url: String,
    pub title: String,
    pub text: String,
}

/// Why a web call failed (messages are shown to the model, so they say what to do).
#[derive(Debug, thiserror::Error)]
pub enum WebError {
    #[error("Only public http(s) addresses can be opened.")]
    Blocked,
    #[error("The address is not valid.")]
    BadUrl,
    #[error("The site did not answer in time.")]
    Timeout,
    #[error("The page is not text (it is {0}).")]
    NotText(String),
    #[error("The site answered with status {0}.")]
    Status(u16),
    #[error("Network error: {0}")]
    Network(String),
}

/// Is this a public, routable address?
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_broadcast()
                || v.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (64..128).contains(&o[1])) // CGNAT
                || o[0] >= 224) // multicast + reserved
        }
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            let s = v.segments();
            !(v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80) // link local
        }
    }
}

/// Web search and page fetching.
#[derive(Clone, Debug, Default)]
pub struct WebClient;

impl WebClient {
    pub fn new() -> Self {
        Self
    }

    /// Search with DuckDuckGo's HTML endpoint (no account or key).
    pub async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchHit>, WebError> {
        let mut url = Url::parse("https://html.duckduckgo.com/html/").map_err(|_| WebError::BadUrl)?;
        url.query_pairs_mut().append_pair("q", query);
        let (_, body) = self.get(url, "text/html").await?;
        Ok(parse_search(&body, max_results))
    }

    /// Fetch `raw` as plain text, at most `max_chars` characters.
    pub async fn fetch(&self, raw: &str, max_chars: usize) -> Result<Page, WebError> {
        let url = Url::parse(raw.trim()).map_err(|_| WebError::BadUrl)?;
        let (final_url, body) = self.get(url, "text/html,text/plain,application/json").await?;
        let (title, text) = html_to_text(&body);
        let text: String = text.chars().take(max_chars).collect();
        Ok(Page {
            url: final_url.to_string(),
            title,
            text,
        })
    }

    /// GET with the public-address guard on the first request and on every redirect.
    async fn get(&self, mut url: Url, accept: &str) -> Result<(Url, String), WebError> {
        for _ in 0..=MAX_REDIRECTS {
            let addr = guard(&url).await?;
            let host = url.host_str().ok_or(WebError::BadUrl)?.to_owned();
            let client = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .timeout(TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .resolve(&host, addr)
                .build()
                .map_err(|e| WebError::Network(e.to_string()))?;
            let resp = client
                .get(url.clone())
                .header("Accept", accept)
                .send()
                .await
                .map_err(|e| {
                    if e.is_timeout() {
                        WebError::Timeout
                    } else {
                        WebError::Network(e.without_url().to_string())
                    }
                })?;
            let status = resp.status();
            if status.is_redirection() {
                let next = resp
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or(WebError::Status(status.as_u16()))?;
                url = url.join(next).map_err(|_| WebError::BadUrl)?;
                continue;
            }
            if !status.is_success() {
                return Err(WebError::Status(status.as_u16()));
            }
            let ctype = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("text/plain")
                .to_ascii_lowercase();
            if !(ctype.starts_with("text/") || ctype.contains("json") || ctype.contains("xml")) {
                return Err(WebError::NotText(ctype));
            }
            let mut resp = resp;
            let mut bytes = Vec::new();
            while let Some(chunk) = resp
                .chunk()
                .await
                .map_err(|e| WebError::Network(e.without_url().to_string()))?
            {
                bytes.extend_from_slice(&chunk);
                if bytes.len() >= MAX_BODY_BYTES {
                    bytes.truncate(MAX_BODY_BYTES);
                    break;
                }
            }
            return Ok((url, String::from_utf8_lossy(&bytes).into_owned()));
        }
        Err(WebError::Network("too many redirects".into()))
    }
}

/// Reject non-http(s) URLs and non-public hosts; returns the checked address to connect to.
async fn guard(url: &Url) -> Result<SocketAddr, WebError> {
    if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() {
        return Err(WebError::Blocked);
    }
    let port = url.port_or_known_default().ok_or(WebError::BadUrl)?;
    let host = url.host_str().ok_or(WebError::BadUrl)?;
    let addrs: Vec<SocketAddr> = match host.trim_matches(['[', ']']).parse::<IpAddr>() {
        Ok(ip) => vec![SocketAddr::new(ip, port)],
        Err(_) => tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| WebError::Network(e.to_string()))?
            .collect(),
    };
    if addrs.is_empty() || addrs.iter().any(|a| !is_public_ip(a.ip())) {
        return Err(WebError::Blocked);
    }
    addrs.first().copied().ok_or(WebError::Blocked)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(if b[i] == b'+' { b' ' } else { b[i] });
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let text = decode_entities(&out);
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Pull results out of DuckDuckGo's HTML page.
pub fn parse_search(html: &str, max: usize) -> Vec<SearchHit> {
    let (Ok(link), Ok(snip)) = (
        Regex::new(r#"(?s)<a[^>]*class="result__a"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#),
        Regex::new(r#"(?s)class="result__snippet"[^>]*>(.*?)</a>"#),
    ) else {
        return Vec::new();
    };
    let snippets: Vec<String> = snip.captures_iter(html).map(|c| strip_tags(&c[1])).collect();
    link.captures_iter(html)
        .enumerate()
        .filter_map(|(i, c)| {
            let href = decode_entities(&c[1]);
            let target = match href.split_once("uddg=") {
                Some((_, rest)) => percent_decode(rest.split('&').next().unwrap_or("")),
                None => href.clone(),
            };
            let target = if target.starts_with("//") {
                format!("https:{target}")
            } else {
                target
            };
            if !(target.starts_with("http://") || target.starts_with("https://")) {
                return None;
            }
            Some(SearchHit {
                title: strip_tags(&c[2]),
                url: target,
                snippet: snippets.get(i).cloned().unwrap_or_default(),
            })
        })
        .take(max)
        .collect()
}

/// `(title, text)` of an HTML (or plain text) body. Scripts, styles and page chrome are dropped.
pub fn html_to_text(body: &str) -> (String, String) {
    if !body.contains('<') {
        return (String::new(), body.trim().to_owned());
    }
    let title = Regex::new(r"(?is)<title[^>]*>(.*?)</title>")
        .ok()
        .and_then(|re| re.captures(body).map(|c| strip_tags(&c[1])))
        .unwrap_or_default();
    let mut cleaned = body.to_owned();
    for tag in [
        "head", "script", "style", "noscript", "nav", "footer", "header", "svg", "form",
    ] {
        if let Ok(re) = Regex::new(&format!(r"(?is)<{tag}\b.*?</{tag}>")) {
            cleaned = re.replace_all(&cleaned, " ").into_owned();
        }
    }
    if let Ok(re) = Regex::new(r"(?is)<!--.*?-->") {
        cleaned = re.replace_all(&cleaned, " ").into_owned();
    }
    // Keep paragraph structure: block-level tags become line breaks before the tags are stripped.
    if let Ok(re) = Regex::new(r"(?i)</?(p|div|br|li|h[1-6]|tr|section|article|pre)\b[^>]*>") {
        cleaned = re.replace_all(&cleaned, "\n").into_owned();
    }
    let mut out = String::new();
    for line in strip_tags_keep_lines(&cleaned).lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !line.is_empty() {
            out.push_str(&line);
            out.push('\n');
        }
    }
    (title, out.trim_end().to_owned())
}

fn strip_tags_keep_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_entities(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_and_special_addresses_are_not_public() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.5",
            "172.16.0.1",
            "169.254.1.1",
            "0.0.0.0",
            "100.64.0.1",
            "224.0.0.1",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "93.184.216.34", "2606:4700:4700::1111"] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn loopback_and_odd_schemes_are_refused() {
        let c = WebClient::new();
        for u in [
            "http://127.0.0.1:8080/",
            "http://localhost/",
            "http://[::1]/",
            "file:///etc/passwd",
            "ftp://example.org/",
            "http://user:pw@example.org/",
            "http://192.168.1.1/",
        ] {
            let e = c.fetch(u, 100).await.unwrap_err();
            assert!(matches!(e, WebError::Blocked | WebError::BadUrl), "{u}: {e}");
        }
    }

    #[test]
    fn search_results_are_parsed_and_redirects_unwrapped() {
        let html = r#"
<div class="result"><h2><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdoc.rust-lang.org%2Fbook%2F&amp;rut=abc">The <b>Rust</b> Book</a></h2>
<a class="result__snippet" href="x">Learn &amp; build <b>Rust</b>.</a></div>
<div class="result"><a class="result__a" href="https://example.org/two">Two</a>
<a class="result__snippet" href="x">Second</a></div>"#;
        let hits = parse_search(html, 5);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://doc.rust-lang.org/book/");
        assert_eq!(hits[0].title, "The Rust Book");
        assert_eq!(hits[0].snippet, "Learn & build Rust.");
        assert_eq!(parse_search(html, 1).len(), 1);
    }

    #[test]
    fn pages_become_plain_text_without_scripts() {
        let html = "<html><head><title>Hi &amp; bye</title><style>p{}</style></head><body><nav>menu</nav>\
<script>alert(1)</script><h1>Head</h1><p>One <b>two</b></p><p>Three</p></body></html>";
        let (title, text) = html_to_text(html);
        assert_eq!(title, "Hi & bye");
        assert_eq!(text, "Head\nOne two\nThree");
        assert!(!text.contains("alert") && !text.contains("menu"));
    }
}
