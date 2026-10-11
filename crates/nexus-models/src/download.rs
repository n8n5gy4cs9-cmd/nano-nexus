//! The resumable, verified downloader (MODEL-MANAGER §4).
//!
//! A download lives in `<dest>.part` plus `<dest>.part.json`. It resumes with `Range`, hashes while it
//! streams, retries network errors with backoff, follows redirects by hand (so `Range` survives and the
//! Hugging Face token never reaches a CDN host), verifies size and SHA-256, validates the GGUF header,
//! and only then renames the file into place.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::gguf;

/// Free space required beyond the file itself.
pub const DISK_HEADROOM: u64 = 500 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Preparing,
    Downloading,
    Verifying,
    Finalizing,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DownloadProgress {
    pub bytes: u64,
    pub total: u64,
    pub bytes_per_sec: f64,
    pub eta_secs: Option<u64>,
    pub phase: Phase,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    /// Paused or cancelled: the caller decides whether to keep the part file.
    #[error("cancelled")]
    Cancelled,
    #[error("not enough free disk space: need {need} bytes, have {have}")]
    InsufficientDisk { need: u64, have: u64 },
    #[error("the downloaded file does not match its checksum")]
    ChecksumMismatch,
    #[error("the downloaded file has the wrong size ({got} bytes, expected {expected})")]
    SizeMismatch { got: u64, expected: u64 },
    #[error("network error: {0}")]
    Network(String),
    #[error("the server answered with status {0}")]
    Http(u16),
    #[error("the downloaded file is not a valid GGUF model: {0}")]
    NotGguf(String),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
}

/// What to download and where it goes.
#[derive(Clone, Debug)]
pub struct DownloadSpec {
    pub url: String,
    /// Sent only to the host of `url`, never to a redirect target on another host.
    pub token: Option<String>,
    pub expected_sha256: String,
    pub expected_bytes: u64,
    pub dest: PathBuf,
    /// Recorded in the part's meta so a changed revision restarts the download.
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PartMeta {
    url: String,
    sha256: String,
    bytes: u64,
    revision: String,
}

/// The finished file.
#[derive(Clone, Debug, PartialEq)]
pub struct Downloaded {
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}

pub fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

fn meta_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part.json");
    PathBuf::from(s)
}

/// Delete a partial download (Cancel; Pause keeps it).
pub fn discard_part(dest: &Path) {
    let _ = std::fs::remove_file(part_path(dest));
    let _ = std::fs::remove_file(meta_path(dest));
}

/// Bytes already on disk for a resumable download of `dest`.
pub fn part_bytes(dest: &Path) -> u64 {
    std::fs::metadata(part_path(dest)).map(|m| m.len()).unwrap_or(0)
}

/// Free bytes on the volume holding `path` (`None` when it cannot be determined).
pub fn available_space(path: &Path) -> Option<u64> {
    let canon = std::fs::canonicalize(path).ok()?;
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| canon.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

fn authority(url: &str) -> &str {
    url.split("://")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .unwrap_or("")
}

fn absolute(base: &str, location: &str) -> String {
    if location.contains("://") {
        location.to_owned()
    } else if let Some(rest) = location.strip_prefix('/') {
        let scheme = base.split("://").next().unwrap_or("https");
        format!("{scheme}://{}/{rest}", authority(base))
    } else {
        let dir = base.rsplit_once('/').map_or(base, |(d, _)| d);
        format!("{dir}/{location}")
    }
}

pub struct Downloader {
    http: reqwest::Client,
    retry_delays: Vec<Duration>,
    headroom: u64,
    progress_every: Duration,
}

impl Downloader {
    pub fn new() -> Result<Self, DownloadError> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .user_agent(concat!("NanoNexus/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| DownloadError::Network(e.to_string()))?;
        Ok(Self {
            http,
            retry_delays: [1, 2, 4, 8, 16].map(Duration::from_secs).to_vec(),
            headroom: DISK_HEADROOM,
            progress_every: PROGRESS_EVERY,
        })
    }

    /// Test knobs: retry delays, the disk headroom, and the progress interval.
    pub fn with_tuning(mut self, retry_delays: Vec<Duration>, headroom: u64) -> Self {
        self.retry_delays = retry_delays;
        self.headroom = headroom;
        self
    }

    /// Emit progress at most this often (default 4 Hz).
    pub fn with_progress_interval(mut self, every: Duration) -> Self {
        self.progress_every = every;
        self
    }

    /// Open `url` with `Range: bytes=from-`, following redirects by hand.
    async fn open(&self, spec: &DownloadSpec, from: u64) -> Result<reqwest::Response, DownloadError> {
        let origin = authority(&spec.url).to_owned();
        let mut url = spec.url.clone();
        for _ in 0..=MAX_REDIRECTS {
            let mut req = self.http.get(&url);
            if from > 0 {
                req = req.header(reqwest::header::RANGE, format!("bytes={from}-"));
            }
            if let Some(t) = &spec.token
                && authority(&url) == origin
            {
                req = req.bearer_auth(t);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| DownloadError::Network(e.to_string()))?;
            let status = resp.status();
            if status.is_redirection() {
                let loc = resp
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or(DownloadError::Http(status.as_u16()))?;
                url = absolute(&url, loc);
                continue;
            }
            return Ok(resp);
        }
        Err(DownloadError::Network("too many redirects".into()))
    }

    /// Download and verify. On `Cancelled` the part file is kept (Pause); call [`discard_part`] to cancel for good.
    pub async fn download(
        &self,
        spec: &DownloadSpec,
        progress: &(dyn Fn(DownloadProgress) + Send + Sync),
        cancel: &CancellationToken,
    ) -> Result<Downloaded, DownloadError> {
        if let Some(dir) = spec.dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let part = part_path(&spec.dest);
        let meta = PartMeta {
            url: spec.url.clone(),
            sha256: spec.expected_sha256.to_lowercase(),
            bytes: spec.expected_bytes,
            revision: spec.revision.clone(),
        };
        let have_part = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let meta_ok = std::fs::read_to_string(meta_path(&spec.dest))
            .ok()
            .and_then(|t| serde_json::from_str::<PartMeta>(&t).ok())
            .is_some_and(|m| m == meta);
        if !meta_ok {
            discard_part(&spec.dest);
        }
        let resume_from = if meta_ok { have_part } else { 0 };

        // Pre-flight disk check (only the bytes still to come, plus headroom).
        let need = spec
            .expected_bytes
            .saturating_sub(resume_from)
            .saturating_mul(105)
            / 100
            + self.headroom;
        if let Some(have) = spec.dest.parent().and_then(available_space)
            && have < need
        {
            return Err(DownloadError::InsufficientDisk { need, have });
        }
        std::fs::write(
            meta_path(&spec.dest),
            serde_json::to_vec(&meta).map_err(|e| std::io::Error::other(e.to_string()))?,
        )?;

        let mut hasher = Sha256::new();
        let mut done: u64 = 0;
        let started = Instant::now();
        let mut emitter = Emitter::new(spec.expected_bytes, started, self.progress_every);
        if resume_from > 0 {
            done = hash_existing(&part, &mut hasher, resume_from, cancel, |n| {
                progress(emitter.frame(n, Phase::Preparing, true))
            })?;
        }

        let mut attempt = 0usize;
        loop {
            if cancel.is_cancelled() {
                return Err(DownloadError::Cancelled);
            }
            match self
                .fetch_into(
                    spec,
                    &part,
                    &mut hasher,
                    &mut done,
                    &mut emitter,
                    progress,
                    cancel,
                )
                .await
            {
                Ok(()) => break,
                Err(DownloadError::Cancelled) => return Err(DownloadError::Cancelled),
                Err(e @ (DownloadError::Network(_) | DownloadError::Http(500..=599))) => {
                    let Some(delay) = self.retry_delays.get(attempt).copied() else {
                        return Err(e);
                    };
                    attempt += 1;
                    tracing::warn!(error = %e, attempt, "download interrupted; retrying");
                    tokio::select! {
                        () = tokio::time::sleep(delay) => {}
                        () = cancel.cancelled() => return Err(DownloadError::Cancelled),
                    }
                }
                Err(e) => return Err(e),
            }
        }

        // Verify.
        progress(emitter.frame(done, Phase::Verifying, true));
        if done != spec.expected_bytes {
            discard_part(&spec.dest);
            return Err(DownloadError::SizeMismatch {
                got: done,
                expected: spec.expected_bytes,
            });
        }
        let digest = hex(&hasher.finalize());
        if !digest.eq_ignore_ascii_case(&spec.expected_sha256) {
            discard_part(&spec.dest);
            return Err(DownloadError::ChecksumMismatch);
        }
        if let Err(e) = gguf::parse_file(&part) {
            discard_part(&spec.dest);
            return Err(DownloadError::NotGguf(e.to_string()));
        }
        progress(emitter.frame(done, Phase::Finalizing, true));
        std::fs::rename(&part, &spec.dest)?;
        let _ = std::fs::remove_file(meta_path(&spec.dest));
        Ok(Downloaded {
            path: spec.dest.clone(),
            bytes: done,
            sha256: digest,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn fetch_into(
        &self,
        spec: &DownloadSpec,
        part: &Path,
        hasher: &mut Sha256,
        done: &mut u64,
        emitter: &mut Emitter,
        progress: &(dyn Fn(DownloadProgress) + Send + Sync),
        cancel: &CancellationToken,
    ) -> Result<(), DownloadError> {
        if *done >= spec.expected_bytes && spec.expected_bytes > 0 {
            return Ok(());
        }
        let resp = tokio::select! {
            r = self.open(spec, *done) => r?,
            () = cancel.cancelled() => return Err(DownloadError::Cancelled),
        };
        let status = resp.status().as_u16();
        match status {
            206 => {
                let start = resp
                    .headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("bytes "))
                    .and_then(|v| v.split(['-', '/']).next())
                    .and_then(|v| v.parse::<u64>().ok());
                if start != Some(*done) {
                    return Err(DownloadError::Network(
                        "the server resumed at the wrong offset".into(),
                    ));
                }
            }
            200 => {
                if *done > 0 {
                    // The server ignored Range: start over.
                    tracing::info!("server ignored Range; restarting the download");
                    *done = 0;
                    *hasher = Sha256::new();
                }
            }
            416 => {
                // Our offset is past the end. Either the part is already whole, or it is junk.
                if *done != spec.expected_bytes {
                    *done = 0;
                    *hasher = Sha256::new();
                    std::fs::File::create(part)?;
                    return Err(DownloadError::Network(
                        "the server rejected the resume offset; restarting".into(),
                    ));
                }
                return Ok(());
            }
            s => return Err(DownloadError::Http(s)),
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(part)?;
        file.set_len(*done)?;
        file.seek(SeekFrom::Start(*done))?;
        let mut out = std::io::BufWriter::with_capacity(1 << 20, file);
        let mut stream = resp.bytes_stream();
        loop {
            let next = tokio::select! {
                n = stream.next() => n,
                () = cancel.cancelled() => {
                    out.flush()?;
                    return Err(DownloadError::Cancelled);
                }
            };
            match next {
                Some(Ok(chunk)) => {
                    if *done + chunk.len() as u64 > spec.expected_bytes {
                        out.flush()?;
                        discard_part(&spec.dest);
                        return Err(DownloadError::SizeMismatch {
                            got: *done + chunk.len() as u64,
                            expected: spec.expected_bytes,
                        });
                    }
                    hasher.update(&chunk);
                    out.write_all(&chunk)?;
                    *done += chunk.len() as u64;
                    if let Some(p) = emitter.tick(*done) {
                        progress(p);
                    }
                }
                Some(Err(e)) => {
                    out.flush()?;
                    return Err(DownloadError::Network(e.to_string()));
                }
                None => break,
            }
        }
        out.flush()?;
        if *done < spec.expected_bytes {
            return Err(DownloadError::Network("the connection closed early".into()));
        }
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Hash the first `len` bytes of an existing part so a resumed download verifies as a whole.
fn hash_existing(
    path: &Path,
    hasher: &mut Sha256,
    len: u64,
    cancel: &CancellationToken,
    mut on_bytes: impl FnMut(u64),
) -> Result<u64, DownloadError> {
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    while total < len {
        if cancel.is_cancelled() {
            return Err(DownloadError::Cancelled);
        }
        let want = usize::try_from((len - total).min(buf.len() as u64)).unwrap_or(buf.len());
        let n = f.read(&mut buf[..want])?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
        on_bytes(total);
    }
    Ok(total)
}

/// Throttles progress to 4 Hz and smooths speed with an EMA over ~3 s.
struct Emitter {
    every: Duration,
    total: u64,
    last_at: Instant,
    last_bytes: u64,
    ema: f64,
    started: Instant,
}

impl Emitter {
    fn new(total: u64, now: Instant, every: Duration) -> Self {
        Self {
            every,
            total,
            last_at: now,
            last_bytes: 0,
            ema: 0.0,
            started: now,
        }
    }

    fn frame(&self, bytes: u64, phase: Phase, _force: bool) -> DownloadProgress {
        let eta = (self.ema > 1.0 && phase == Phase::Downloading)
            .then(|| (self.total.saturating_sub(bytes) as f64 / self.ema) as u64);
        DownloadProgress {
            bytes,
            total: self.total,
            bytes_per_sec: self.ema,
            eta_secs: eta,
            phase,
        }
    }

    fn tick(&mut self, bytes: u64) -> Option<DownloadProgress> {
        let now = Instant::now();
        let dt = now.duration_since(self.last_at);
        if dt < self.every && bytes < self.total {
            return None;
        }
        let secs = dt.as_secs_f64().max(1e-3);
        let inst = bytes.saturating_sub(self.last_bytes) as f64 / secs;
        let alpha = 1.0 - (-secs / 3.0).exp();
        self.ema = if self.ema == 0.0 {
            inst
        } else {
            alpha * inst + (1.0 - alpha) * self.ema
        };
        self.last_at = now;
        self.last_bytes = bytes;
        let _ = self.started;
        Some(self.frame(bytes, Phase::Downloading, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode, header};
    use axum::response::Response;
    use axum::routing::get;
    use bytes::Bytes;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Log {
        ranges: Vec<Option<String>>,
        auth: Vec<Option<String>>,
    }

    #[derive(Clone)]
    struct Srv {
        data: Arc<Vec<u8>>,
        honor_range: bool,
        /// Cut the connection after this many bytes on the first request.
        drop_after: Arc<Mutex<Option<usize>>>,
        log: Arc<Mutex<Log>>,
        redirect_to: Option<String>,
    }

    async fn handler(State(s): State<Srv>, headers: HeaderMap) -> Response {
        let range = headers
            .get(header::RANGE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        if let Ok(mut l) = s.log.lock() {
            l.ranges.push(range.clone());
            l.auth.push(
                headers
                    .get(header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned),
            );
        }
        if let Some(to) = &s.redirect_to {
            return Response::builder()
                .status(302)
                .header(header::LOCATION, to.as_str())
                .body(Body::empty())
                .unwrap();
        }
        let total = s.data.len();
        let start = match (&range, s.honor_range) {
            (Some(r), true) => r
                .strip_prefix("bytes=")
                .and_then(|v| v.strip_suffix('-'))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0),
            _ => 0,
        };
        if start >= total && start > 0 {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .body(Body::empty())
                .unwrap();
        }
        let slice: Vec<u8> = s.data[start..].to_vec();
        let cut = s.drop_after.lock().ok().and_then(|mut d| d.take());
        let mut b = Response::builder().header(header::CONTENT_LENGTH, slice.len());
        b = if start > 0 {
            b.status(StatusCode::PARTIAL_CONTENT).header(
                header::CONTENT_RANGE,
                format!("bytes {start}-{}/{total}", total - 1),
            )
        } else {
            b.status(StatusCode::OK)
        };
        let mut chunks: Vec<Result<Bytes, std::io::Error>> = match cut {
            Some(n) => slice[..n.min(slice.len())]
                .chunks(32 * 1024)
                .map(|c| Ok(Bytes::copy_from_slice(c)))
                .collect(),
            None => slice
                .chunks(64 * 1024)
                .map(|c| Ok(Bytes::copy_from_slice(c)))
                .collect(),
        };
        if cut.is_some() {
            chunks.push(Err(std::io::Error::other("connection reset")));
        }
        // A short pause between chunks lets each one reach the client before a later error.
        let paced = futures_util::stream::unfold(chunks.into_iter(), |mut it| async move {
            tokio::time::sleep(Duration::from_millis(2)).await;
            it.next().map(|c| (c, it))
        });
        b.body(Body::from_stream(paced)).unwrap()
    }

    async fn serve(srv: Srv) -> String {
        let app = axum::Router::new().route("/f.gguf", get(handler)).with_state(srv);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://{addr}/f.gguf")
    }

    fn model_bytes(total: usize) -> Vec<u8> {
        let mut v = gguf::fixture::sample();
        v.resize(total, 7);
        v
    }

    fn sha(data: &[u8]) -> String {
        hex(&Sha256::digest(data))
    }

    fn srv(data: Vec<u8>) -> Srv {
        Srv {
            data: Arc::new(data),
            honor_range: true,
            drop_after: Arc::default(),
            log: Arc::default(),
            redirect_to: None,
        }
    }

    fn spec(url: String, data: &[u8], dir: &Path) -> DownloadSpec {
        DownloadSpec {
            url,
            token: None,
            expected_sha256: sha(data),
            expected_bytes: data.len() as u64,
            dest: dir.join("m").join("model.gguf"),
            revision: "r1".into(),
        }
    }

    fn dl() -> Downloader {
        Downloader::new()
            .unwrap()
            .with_tuning(vec![Duration::from_millis(10); 5], 0)
            .with_progress_interval(Duration::ZERO)
    }

    fn nop(_: DownloadProgress) {}

    #[tokio::test]
    async fn downloads_verifies_and_moves_into_place() {
        let data = model_bytes(1_500_000);
        let url = serve(srv(data.clone())).await;
        let dir = tempfile::tempdir().unwrap();
        let s = spec(url, &data, dir.path());
        let phases = Mutex::new(Vec::new());
        let out = dl()
            .download(
                &s,
                &|p| phases.lock().unwrap().push(p.phase),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(out.bytes, data.len() as u64);
        assert_eq!(std::fs::read(&s.dest).unwrap(), data);
        assert!(!part_path(&s.dest).exists() && !meta_path(&s.dest).exists());
        let p = phases.lock().unwrap().clone();
        assert!(
            p.contains(&Phase::Downloading)
                && p.contains(&Phase::Verifying)
                && p.last() == Some(&Phase::Finalizing)
        );
    }

    #[tokio::test]
    async fn pause_keeps_the_part_and_resume_uses_range() {
        let data = model_bytes(2_000_000);
        let server = srv(data.clone());
        let log = server.log.clone();
        let url = serve(server).await;
        let dir = tempfile::tempdir().unwrap();
        let s = spec(url, &data, dir.path());
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        let r = dl()
            .download(
                &s,
                &move |p| {
                    if p.bytes > 600_000 {
                        c2.cancel()
                    }
                },
                &cancel,
            )
            .await;
        assert!(matches!(r, Err(DownloadError::Cancelled)));
        let partial = part_bytes(&s.dest);
        assert!(partial > 0 && partial < data.len() as u64);
        let out = dl().download(&s, &nop, &CancellationToken::new()).await.unwrap();
        assert_eq!(std::fs::read(&out.path).unwrap(), data);
        let l = log.lock().unwrap();
        assert_eq!(l.ranges[0], None);
        assert_eq!(l.ranges[1].as_deref(), Some(format!("bytes={partial}-").as_str()));
    }

    #[tokio::test]
    async fn a_server_that_ignores_range_restarts_from_zero() {
        let data = model_bytes(800_000);
        let mut server = srv(data.clone());
        server.honor_range = false;
        let url = serve(server).await;
        let dir = tempfile::tempdir().unwrap();
        let s = spec(url, &data, dir.path());
        // Leave a plausible part behind.
        std::fs::create_dir_all(s.dest.parent().unwrap()).unwrap();
        std::fs::write(part_path(&s.dest), &data[..300_000]).unwrap();
        std::fs::write(
            meta_path(&s.dest),
            serde_json::to_vec(&PartMeta {
                url: s.url.clone(),
                sha256: sha(&data),
                bytes: data.len() as u64,
                revision: "r1".into(),
            })
            .unwrap(),
        )
        .unwrap();
        let out = dl().download(&s, &nop, &CancellationToken::new()).await.unwrap();
        assert_eq!(std::fs::read(&out.path).unwrap(), data);
    }

    #[tokio::test]
    async fn a_corrupted_part_fails_the_checksum_and_is_deleted() {
        let data = model_bytes(500_000);
        let url = serve(srv(data.clone())).await;
        let dir = tempfile::tempdir().unwrap();
        let s = spec(url, &data, dir.path());
        // A matching meta, but one flipped byte in the part (the debug command's scenario).
        std::fs::create_dir_all(s.dest.parent().unwrap()).unwrap();
        let mut bad = data[..200_000].to_vec();
        bad[150_000] ^= 0xff;
        std::fs::write(part_path(&s.dest), &bad).unwrap();
        std::fs::write(
            meta_path(&s.dest),
            serde_json::to_vec(&PartMeta {
                url: s.url.clone(),
                sha256: sha(&data),
                bytes: data.len() as u64,
                revision: "r1".into(),
            })
            .unwrap(),
        )
        .unwrap();
        let r = dl().download(&s, &nop, &CancellationToken::new()).await;
        assert!(matches!(r, Err(DownloadError::ChecksumMismatch)), "{r:?}");
        assert!(!part_path(&s.dest).exists() && !s.dest.exists());
    }

    #[tokio::test]
    async fn a_dropped_connection_is_retried_and_resumed() {
        let data = model_bytes(900_000);
        let server = srv(data.clone());
        *server.drop_after.lock().unwrap() = Some(300_000);
        let log = server.log.clone();
        let url = serve(server).await;
        let dir = tempfile::tempdir().unwrap();
        let s = spec(url, &data, dir.path());
        let out = dl().download(&s, &nop, &CancellationToken::new()).await.unwrap();
        assert_eq!(std::fs::read(&out.path).unwrap(), data);
        let l = log.lock().unwrap();
        assert_eq!(l.ranges.len(), 2);
        assert!(
            l.ranges[1]
                .as_deref()
                .is_some_and(|r| r.starts_with("bytes=") && r != "bytes=0-")
        );
    }

    #[tokio::test]
    async fn the_token_does_not_follow_a_redirect_to_another_host() {
        let data = model_bytes(300_000);
        let cdn = srv(data.clone());
        let cdn_log = cdn.log.clone();
        let cdn_url = serve(cdn).await;
        let mut hub = srv(vec![]);
        hub.redirect_to = Some(cdn_url.replace("127.0.0.1", "localhost"));
        let hub_log = hub.log.clone();
        let hub_url = serve(hub).await;
        let dir = tempfile::tempdir().unwrap();
        let mut s = spec(hub_url, &data, dir.path());
        s.token = Some("hf_secret".into());
        dl().download(&s, &nop, &CancellationToken::new()).await.unwrap();
        assert_eq!(
            hub_log.lock().unwrap().auth[0].as_deref(),
            Some("Bearer hf_secret")
        );
        assert_eq!(
            cdn_log.lock().unwrap().auth[0],
            None,
            "the CDN must not see the token"
        );
    }

    #[tokio::test]
    async fn not_enough_disk_fails_before_any_request() {
        let data = model_bytes(1000);
        let server = srv(data.clone());
        let log = server.log.clone();
        let url = serve(server).await;
        let dir = tempfile::tempdir().unwrap();
        let mut s = spec(url, &data, dir.path());
        s.expected_bytes = u64::MAX / 4;
        let r = dl().download(&s, &nop, &CancellationToken::new()).await;
        if available_space(dir.path()).is_some() {
            assert!(matches!(r, Err(DownloadError::InsufficientDisk { .. })), "{r:?}");
            assert!(log.lock().unwrap().ranges.is_empty());
        }
    }

    #[tokio::test]
    async fn a_file_that_is_not_gguf_is_rejected() {
        let data = vec![1u8; 100_000];
        let url = serve(srv(data.clone())).await;
        let dir = tempfile::tempdir().unwrap();
        let s = spec(url, &data, dir.path());
        let r = dl().download(&s, &nop, &CancellationToken::new()).await;
        assert!(matches!(r, Err(DownloadError::NotGguf(_))), "{r:?}");
        assert!(!s.dest.exists() && !part_path(&s.dest).exists());
    }

    #[tokio::test]
    async fn a_changed_revision_starts_over() {
        let data = model_bytes(400_000);
        let url = serve(srv(data.clone())).await;
        let dir = tempfile::tempdir().unwrap();
        let s = spec(url, &data, dir.path());
        std::fs::create_dir_all(s.dest.parent().unwrap()).unwrap();
        std::fs::write(part_path(&s.dest), vec![9u8; 1000]).unwrap();
        std::fs::write(
            meta_path(&s.dest),
            serde_json::to_vec(&PartMeta {
                url: s.url.clone(),
                sha256: sha(&data),
                bytes: data.len() as u64,
                revision: "old".into(),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(dl().download(&s, &nop, &CancellationToken::new()).await.is_ok());
    }

    #[tokio::test]
    async fn http_errors_surface() {
        let dir = tempfile::tempdir().unwrap();
        let data = model_bytes(1000);
        let s = spec("http://127.0.0.1:1/nope.gguf".into(), &data, dir.path());
        let r = Downloader::new()
            .unwrap()
            .with_tuning(vec![], 0)
            .download(&s, &nop, &CancellationToken::new())
            .await;
        assert!(matches!(r, Err(DownloadError::Network(_))), "{r:?}");
    }

    #[test]
    fn urls_resolve() {
        assert_eq!(absolute("http://a:1/x/y", "/z"), "http://a:1/z");
        assert_eq!(absolute("http://a/x/y", "https://b/c"), "https://b/c");
        assert_eq!(absolute("http://a/x/y", "w"), "http://a/x/w");
        assert_eq!(
            authority("https://cdn.example.com:8443/p"),
            "cdn.example.com:8443"
        );
    }
}
