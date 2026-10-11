//! Minimal Hugging Face client (MODEL-MANAGER §3): plain `reqwest`, no SDK. Redirects are never
//! followed automatically so the downloader can strip credentials when a download moves to a CDN.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub const DEFAULT_ENDPOINT: &str = "https://huggingface.co";
const SEARCH_TTL: Duration = Duration::from_secs(600);

#[derive(Debug, thiserror::Error)]
pub enum HfError {
    /// DNS, connection, or timeout failures: show the offline state.
    #[error("could not reach Hugging Face: {0}")]
    Offline(String),
    #[error("not found on Hugging Face")]
    NotFound,
    /// 401: the repo needs a token.
    #[error("this repository needs a Hugging Face token")]
    TokenRequired,
    /// 403: the repo is gated and the token has no access.
    #[error("this repository is gated: accept its terms on huggingface.co first")]
    Gated,
    #[error("Hugging Face answered with status {0}")]
    Status(u16),
    #[error("Hugging Face sent an unexpected answer: {0}")]
    Parse(String),
}

fn net(e: reqwest::Error) -> HfError {
    HfError::Offline(e.to_string())
}

/// A repo revision resolved to a commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct RevisionInfo {
    pub sha: String,
    pub license: Option<String>,
    pub gated: bool,
}

/// One file in a repo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TreeFile {
    pub path: String,
    pub size: u64,
    /// SHA-256 of the content (the LFS oid); `None` for small non-LFS files.
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct SearchHit {
    pub id: String,
    pub downloads: u64,
    pub likes: u64,
    pub last_modified: Option<String>,
    pub tags: Vec<String>,
}

/// A downloadable model: one GGUF, or all shards of a split one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct GgufFile {
    /// File name to show and to load (the first shard for split models).
    pub name: String,
    pub quant: Option<String>,
    pub bytes: u64,
    /// Every part in order (a single file has one).
    pub parts: Vec<TreeFile>,
}

/// Quant label from a file name (`Q4_K_M`, `Q8_0`, `IQ3_XS`, `F16`, `BF16`).
pub fn quant_from_name(name: &str) -> Option<String> {
    let upper = name.to_uppercase();
    let re = regex::Regex::new(r"(?:^|[^A-Z0-9])((?:IQ[1-4]_[A-Z]+)|(?:Q[2-8]_(?:K_[SML]|K|[01]))|(?:Q[2-8]_K_[SML])|BF16|F16|F32)(?:[^A-Z0-9]|$)").ok()?;
    re.captures(&upper)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
}

/// Group a repo's `.gguf` files: split shards (`-00001-of-00003`) become one item.
pub fn group_gguf(files: &[TreeFile]) -> Vec<GgufFile> {
    let shard = regex::Regex::new(r"^(.*)-(\d{5})-of-(\d{5})\.gguf$");
    let mut singles: Vec<GgufFile> = Vec::new();
    let mut groups: HashMap<String, Vec<(u32, TreeFile)>> = HashMap::new();
    for f in files.iter().filter(|f| f.path.ends_with(".gguf")) {
        let caps = shard.as_ref().ok().and_then(|re| re.captures(&f.path));
        if let Some(c) = caps {
            let n: u32 = c[2].parse().unwrap_or(0);
            groups.entry(c[1].to_owned()).or_default().push((n, f.clone()));
        } else {
            singles.push(GgufFile {
                name: f.path.clone(),
                quant: quant_from_name(&f.path),
                bytes: f.size,
                parts: vec![f.clone()],
            });
        }
    }
    for (stem, mut parts) in groups {
        parts.sort_by_key(|(n, _)| *n);
        let parts: Vec<TreeFile> = parts.into_iter().map(|(_, f)| f).collect();
        let first = parts.first().map(|p| p.path.clone()).unwrap_or_default();
        singles.push(GgufFile {
            name: first,
            quant: quant_from_name(&stem),
            bytes: parts.iter().map(|p| p.size).sum(),
            parts,
        });
    }
    singles.sort_by(|a, b| a.bytes.cmp(&b.bytes).then_with(|| a.name.cmp(&b.name)));
    singles
}

#[derive(Deserialize)]
struct RevisionRaw {
    sha: String,
    #[serde(default)]
    gated: serde_json::Value,
    #[serde(default, rename = "cardData")]
    card_data: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct TreeRaw {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    lfs: Option<LfsRaw>,
}

#[derive(Deserialize)]
struct LfsRaw {
    oid: String,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Deserialize)]
struct SearchRaw {
    #[serde(alias = "modelId")]
    id: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    likes: u64,
    #[serde(default, rename = "lastModified")]
    last_modified: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

/// Talks to the Hugging Face API. Cheap to clone-share behind an `Arc`.
pub struct HfClient {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
    cache: Mutex<HashMap<String, (Instant, Vec<SearchHit>)>>,
}

impl HfClient {
    /// `endpoint` defaults to `HF_ENDPOINT` or huggingface.co. The token is sent only to `endpoint`.
    pub fn new(endpoint: Option<String>, token: Option<String>) -> Result<Self, HfError> {
        let base = endpoint
            .or_else(|| std::env::var("HF_ENDPOINT").ok())
            .unwrap_or_else(|| DEFAULT_ENDPOINT.to_owned())
            .trim_end_matches('/')
            .to_owned();
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("NanoNexus/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(net)?;
        Ok(Self {
            http,
            base,
            token,
            cache: Mutex::new(HashMap::new()),
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.base
    }

    pub fn with_token(mut self, token: Option<String>) -> Self {
        self.token = token;
        self
    }

    fn get(&self, url: &str) -> reqwest::RequestBuilder {
        let r = self.http.get(url);
        match &self.token {
            Some(t) if url.starts_with(&self.base) => r.bearer_auth(t),
            _ => r,
        }
    }

    async fn json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T, HfError> {
        let resp = self.get(url).send().await.map_err(net)?;
        match resp.status().as_u16() {
            200..=299 => resp.json::<T>().await.map_err(|e| HfError::Parse(e.to_string())),
            401 => Err(HfError::TokenRequired),
            403 => Err(HfError::Gated),
            404 => Err(HfError::NotFound),
            s => Err(HfError::Status(s)),
        }
    }

    /// Resolve `revision` (a branch like `main`) to a commit.
    pub async fn resolve_revision(&self, repo: &str, revision: &str) -> Result<RevisionInfo, HfError> {
        let raw: RevisionRaw = self
            .json(&format!("{}/api/models/{repo}/revision/{revision}", self.base))
            .await?;
        let gated = !matches!(
            raw.gated,
            serde_json::Value::Null | serde_json::Value::Bool(false)
        );
        let license = raw
            .card_data
            .as_ref()
            .and_then(|c| c.get("license"))
            .and_then(|l| l.as_str())
            .map(str::to_owned);
        Ok(RevisionInfo {
            sha: raw.sha,
            license,
            gated,
        })
    }

    /// Every file of the repo at commit `sha` (recursive), with SHA-256 for LFS files.
    pub async fn tree(&self, repo: &str, sha: &str) -> Result<Vec<TreeFile>, HfError> {
        let raw: Vec<TreeRaw> = self
            .json(&format!(
                "{}/api/models/{repo}/tree/{sha}?recursive=true",
                self.base
            ))
            .await?;
        Ok(raw
            .into_iter()
            .filter(|t| t.kind == "file")
            .map(|t| TreeFile {
                size: t.lfs.as_ref().and_then(|l| l.size).unwrap_or(t.size),
                sha256: t.lfs.map(|l| l.oid),
                path: t.path,
            })
            .collect())
    }

    /// Search GGUF models by downloads. Results are cached for 10 minutes per query.
    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchHit>, HfError> {
        let key = format!("{}|{limit}", query.trim().to_lowercase());
        if let Ok(c) = self.cache.lock()
            && let Some((at, hits)) = c.get(&key)
            && at.elapsed() < SEARCH_TTL
        {
            return Ok(hits.clone());
        }
        let q: String = query
            .trim()
            .to_lowercase()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                    c.to_string()
                } else {
                    format!("%{:02X}", c as u32 & 0xff)
                }
            })
            .collect();
        let url = format!(
            "{}/api/models?search={q}&filter=gguf&sort=downloads&direction=-1&limit={limit}",
            self.base
        );
        let raw: Vec<SearchRaw> = self.json(&url).await?;
        let hits: Vec<SearchHit> = raw
            .into_iter()
            .map(|r| SearchHit {
                id: r.id,
                downloads: r.downloads,
                likes: r.likes,
                last_modified: r.last_modified,
                tags: r.tags,
            })
            .collect();
        if let Ok(mut c) = self.cache.lock() {
            c.insert(key, (Instant::now(), hits.clone()));
        }
        Ok(hits)
    }

    /// The URL a file downloads from (it redirects to a CDN; the downloader follows manually).
    pub fn resolve_url(&self, repo: &str, sha: &str, file: &str) -> String {
        format!("{}/{repo}/resolve/{sha}/{file}", self.base)
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn client(server: &MockServer, token: Option<&str>) -> HfClient {
        HfClient::new(Some(server.uri()), token.map(str::to_owned)).unwrap()
    }

    #[tokio::test]
    async fn resolves_a_revision_with_license_and_gating() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/models/o/r/revision/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"sha": "abc123", "gated": "auto", "cardData": {"license": "apache-2.0"}}),
            ))
            .mount(&s)
            .await;
        let r = client(&s, None)
            .await
            .resolve_revision("o/r", "main")
            .await
            .unwrap();
        assert_eq!(
            r,
            RevisionInfo {
                sha: "abc123".into(),
                license: Some("apache-2.0".into()),
                gated: true
            }
        );
    }

    #[tokio::test]
    async fn lists_files_with_lfs_hashes() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/models/o/r/tree/abc"))
            .and(query_param("recursive", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"type": "directory", "path": "docs", "size": 0},
                {"type": "file", "path": "README.md", "size": 120},
                {"type": "file", "path": "m-Q4_K_M.gguf", "size": 133, "lfs": {"oid": "deadbeef", "size": 657_000_000u64, "pointerSize": 133}}
            ])))
            .mount(&s)
            .await;
        let files = client(&s, None).await.tree("o/r", "abc").await.unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(
            files[1],
            TreeFile {
                path: "m-Q4_K_M.gguf".into(),
                size: 657_000_000,
                sha256: Some("deadbeef".into())
            }
        );
        assert_eq!(files[0].sha256, None);
    }

    #[tokio::test]
    async fn searches_and_caches() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/models"))
            .and(query_param("search", "minicpm"))
            .and(query_param("filter", "gguf"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"id": "a/b-GGUF", "downloads": 10, "likes": 2, "lastModified": "2026-01-01T00:00:00Z", "tags": ["gguf"]}])))
            .expect(1)
            .mount(&s)
            .await;
        let c = client(&s, None).await;
        let a = c.search("MiniCPM", 30).await.unwrap();
        assert_eq!(a[0].id, "a/b-GGUF");
        let b = c.search("minicpm", 30).await.unwrap();
        assert_eq!(
            a, b,
            "second call is served from the cache (the mock expects one request)"
        );
    }

    #[tokio::test]
    async fn status_codes_map_to_friendly_errors() {
        let s = MockServer::start().await;
        for (p, code) in [("a", 401), ("b", 403), ("c", 404), ("d", 500)] {
            Mock::given(method("GET"))
                .and(path(format!("/api/models/o/{p}/revision/main")))
                .respond_with(ResponseTemplate::new(code))
                .mount(&s)
                .await;
        }
        let c = client(&s, None).await;
        assert!(matches!(
            c.resolve_revision("o/a", "main").await,
            Err(HfError::TokenRequired)
        ));
        assert!(matches!(
            c.resolve_revision("o/b", "main").await,
            Err(HfError::Gated)
        ));
        assert!(matches!(
            c.resolve_revision("o/c", "main").await,
            Err(HfError::NotFound)
        ));
        assert!(matches!(
            c.resolve_revision("o/d", "main").await,
            Err(HfError::Status(500))
        ));
    }

    #[tokio::test]
    async fn the_token_is_sent_to_the_hub_only() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/models/o/r/revision/main"))
            .and(header("authorization", "Bearer hf_secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"sha": "x"})))
            .expect(1)
            .mount(&s)
            .await;
        let c = client(&s, Some("hf_secret")).await;
        assert!(c.resolve_revision("o/r", "main").await.is_ok());
        // `get` attaches the token only for URLs under the configured endpoint.
        let other = c.get("https://cdn.example.com/file").build().unwrap();
        assert!(other.headers().get("authorization").is_none());
    }

    #[tokio::test]
    async fn an_unreachable_server_is_offline() {
        let c = HfClient::new(Some("http://127.0.0.1:1".into()), None).unwrap();
        assert!(matches!(c.search("x", 5).await, Err(HfError::Offline(_))));
    }

    #[test]
    fn quants_and_shards() {
        assert_eq!(quant_from_name("Model-Q4_K_M.gguf").as_deref(), Some("Q4_K_M"));
        assert_eq!(quant_from_name("model.Q8_0.gguf").as_deref(), Some("Q8_0"));
        assert_eq!(quant_from_name("model-IQ3_XS.gguf").as_deref(), Some("IQ3_XS"));
        assert_eq!(quant_from_name("model-f16.gguf").as_deref(), Some("F16"));
        assert_eq!(quant_from_name("model.gguf"), None);
        let f = |p: &str, s: u64| TreeFile {
            path: p.into(),
            size: s,
            sha256: None,
        };
        let grouped = group_gguf(&[
            f("a-Q4_K_M.gguf", 5),
            f("big-Q8_0-00002-of-00002.gguf", 7),
            f("big-Q8_0-00001-of-00002.gguf", 8),
            f("README.md", 1),
        ]);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].name, "a-Q4_K_M.gguf");
        assert_eq!(
            (grouped[1].name.as_str(), grouped[1].bytes, grouped[1].parts.len()),
            ("big-Q8_0-00001-of-00002.gguf", 15, 2)
        );
        assert_eq!(grouped[1].quant.as_deref(), Some("Q8_0"));
    }
}
