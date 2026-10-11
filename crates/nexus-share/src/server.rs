//! The axum server: routes, security headers, graceful stop (LAN-SHARE §3, §4).

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use nexus_core::error::{ErrorCode, IpcError, NexusError};
use nexus_core::share::{SHARE_COOKIE, SHARE_CSRF_HEADER, ShareDevice};
use serde::Deserialize;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::api::ShareApi;
use crate::assets;
use crate::auth::{Auth, constant_time_eq, csrf_for};

/// Request bodies larger than this are refused.
pub const MAX_BODY: usize = 2 * 1024 * 1024;
/// Open sockets allowed per device.
pub const MAX_SOCKETS_PER_DEVICE: usize = 4;

const CSP: &str = "default-src 'self'; connect-src 'self' ws: wss:; style-src 'self' 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'";

/// How to run the server.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub bind: IpAddr,
    pub port: u16,
    /// Add `Secure` to the cookie (HTTPS mode).
    pub secure_cookie: bool,
    /// Cookie lifetime.
    pub session_days: u32,
    /// Serve HTTPS with this certificate (plain HTTP when `None`).
    pub tls: Option<crate::tls::TlsMaterial>,
    /// Advertise `<name>.local` over Bonjour when set.
    pub mdns_name: Option<String>,
}

struct Shared {
    auth: Arc<Auth>,
    api: Arc<dyn ShareApi>,
    config: ServerConfig,
    shutdown: CancellationToken,
    sockets: Mutex<HashMap<String, usize>>,
}

/// A running share server.
pub struct ShareServer {
    addr: SocketAddr,
    shutdown: CancellationToken,
    task: JoinHandle<()>,
    advertiser: Option<crate::mdns::Advertiser>,
}

fn status_of(code: ErrorCode) -> StatusCode {
    match code {
        ErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
        ErrorCode::ForbiddenRemote | ErrorCode::PermissionDenied | ErrorCode::PathDenied => StatusCode::FORBIDDEN,
        ErrorCode::NotFound => StatusCode::NOT_FOUND,
        ErrorCode::InvalidInput => StatusCode::BAD_REQUEST,
        ErrorCode::Conflict => StatusCode::CONFLICT,
        ErrorCode::Busy => StatusCode::TOO_MANY_REQUESTS,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn error_response(e: NexusError) -> Response {
    // Pairing failures say nothing beyond "no": no hint which part was wrong.
    let ipc = if matches!(e, NexusError::Unauthorized) {
        IpcError { code: ErrorCode::Unauthorized, message: "unauthorized".into(), detail: None }
    } else {
        IpcError { code: e.code(), message: e.to_string(), detail: None }
    };
    (status_of(ipc.code), Json(json!({ "err": ipc }))).into_response()
}

fn unauthorized() -> Response {
    error_response(NexusError::Unauthorized)
}

/// The device token from the `Cookie` header.
fn cookie_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|kv| {
        let (k, v) = kv.trim().split_once('=')?;
        (k == SHARE_COOKIE).then(|| v.to_owned())
    })
}

/// Same-origin check: when the browser sends `Origin`, its host must be the one it connected to.
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else { return true };
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok());
    let origin_host = origin
        .to_str()
        .ok()
        .and_then(|o| o.split_once("://").map(|(_, rest)| rest));
    matches!((host, origin_host), (Some(h), Some(o)) if h.eq_ignore_ascii_case(o))
}

fn csrf_ok(token: &str, presented: Option<&str>) -> bool {
    presented.is_some_and(|p| constant_time_eq(p, &csrf_for(token)))
}

/// Authenticate a request: cookie → device; mutating requests also need the CSRF header.
fn authenticate(
    shared: &Shared,
    headers: &HeaderMap,
    ip: IpAddr,
    require_csrf: bool,
) -> Result<(ShareDevice, String), Response> {
    let token = cookie_token(headers).ok_or_else(unauthorized)?;
    let device = shared.auth.authenticate(&token, Some(ip)).ok_or_else(unauthorized)?;
    if require_csrf {
        let presented = headers.get(SHARE_CSRF_HEADER).and_then(|h| h.to_str().ok());
        if !csrf_ok(&token, presented) || !same_origin(headers) {
            return Err((StatusCode::FORBIDDEN, Json(json!({ "err": { "code": "forbidden_remote", "message": "bad CSRF token", "detail": null } }))).into_response());
        }
    }
    Ok((device, token))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true }))
}

#[derive(Deserialize)]
struct PairBody {
    code: String,
    name: String,
}

async fn pair(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<PairBody>,
) -> Response {
    // A custom header forces a CORS preflight, so other sites cannot post here.
    if headers.get(SHARE_CSRF_HEADER).is_none() || !same_origin(&headers) {
        return (StatusCode::FORBIDDEN, Json(json!({ "err": { "code": "forbidden_remote", "message": "bad request", "detail": null } }))).into_response();
    }
    let ua = headers.get(header::USER_AGENT).and_then(|h| h.to_str().ok());
    match shared.auth.pair(&body.code, &body.name, peer.ip(), ua) {
        Ok((device, token)) => {
            let max_age = i64::from(shared.config.session_days) * 86_400;
            let secure = if shared.config.secure_cookie { "; Secure" } else { "" };
            let cookie = format!("{SHARE_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}{secure}");
            let mut res = Json(json!({ "device": device, "csrf": csrf_for(&token) })).into_response();
            if let Ok(v) = HeaderValue::from_str(&cookie) {
                res.headers_mut().insert(header::SET_COOKIE, v);
            }
            res
        }
        Err(e) => error_response(e),
    }
}

async fn session(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    match authenticate(&shared, &headers, peer.ip(), false) {
        Ok((device, token)) => Json(json!({ "device": device, "csrf": csrf_for(&token) })).into_response(),
        Err(r) => r,
    }
}

async fn logout(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let (device, _) = match authenticate(&shared, &headers, peer.ip(), true) {
        Ok(ok) => ok,
        Err(r) => return r,
    };
    if let Err(e) = shared.auth.revoke(&device.id) {
        return error_response(e);
    }
    let mut res = Json(json!({ "ok": true })).into_response();
    let clear = format!("{SHARE_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0");
    if let Ok(v) = HeaderValue::from_str(&clear) {
        res.headers_mut().insert(header::SET_COOKIE, v);
    }
    res
}

/// Releases a device's socket slot when the connection ends.
struct SlotGuard {
    shared: Arc<Shared>,
    device_id: String,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        if let Ok(mut m) = self.shared.sockets.lock() {
            if let Some(n) = m.get_mut(&self.device_id) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    m.remove(&self.device_id);
                }
            }
        }
    }
}

async fn ws(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    upgrade: WebSocketUpgrade,
) -> Response {
    // Browsers cannot set headers on a WebSocket, so the CSRF token may come as `?csrf=`.
    let mut headers = headers;
    if let Some(c) = q.get("csrf").and_then(|c| HeaderValue::from_str(c).ok()) {
        headers.insert(SHARE_CSRF_HEADER, c);
    }
    let (device, _) = match authenticate(&shared, &headers, peer.ip(), true) {
        Ok(ok) => ok,
        Err(r) => return r,
    };
    {
        let Ok(mut m) = shared.sockets.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        let n = m.entry(device.id.clone()).or_insert(0);
        if *n >= MAX_SOCKETS_PER_DEVICE {
            return (StatusCode::TOO_MANY_REQUESTS, "too many connections").into_response();
        }
        *n += 1;
    }
    let guard = SlotGuard { shared: shared.clone(), device_id: device.id.clone() };
    upgrade
        .max_message_size(crate::rpc::MAX_FRAME)
        .on_upgrade(move |socket| async move {
            let _guard = guard;
            crate::rpc::serve_socket(socket, device, shared.api.clone(), shared.auth.clone(), shared.shutdown.clone()).await;
        })
}

async fn fallback(method: Method, uri: Uri) -> Response {
    if uri.path().starts_with("/api/") {
        return (StatusCode::NOT_FOUND, Json(json!({ "err": { "code": "not_found", "message": "not found", "detail": null } }))).into_response();
    }
    assets::serve(&method, &uri)
}

/// Security headers on every response; API responses are never cached.
async fn headers_layer(req: axum::extract::Request, next: Next) -> Response {
    let api = req.uri().path().starts_with("/api/");
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if api {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    res
}

fn router(shared: Arc<Shared>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/pair", post(pair))
        .route("/api/session", get(session))
        .route("/api/logout", post(logout))
        .route("/api/ws", get(ws))
        .fallback(fallback)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(middleware::from_fn(headers_layer))
        .with_state(shared)
}

impl ShareServer {
    /// Bind and serve. `port_in_use` if the port is taken.
    pub async fn start(config: ServerConfig, auth: Arc<Auth>, api: Arc<dyn ShareApi>) -> Result<Self, NexusError> {
        let addr = SocketAddr::new(config.bind, config.port);
        let shutdown = CancellationToken::new();
        let tls = config.tls.clone();
        let mdns_name = config.mdns_name.clone();
        let shared = Arc::new(Shared {
            auth,
            api,
            config,
            shutdown: shutdown.clone(),
            sockets: Mutex::new(HashMap::new()),
        });
        let app = router(shared).into_make_service_with_connect_info::<SocketAddr>();
        let stop = shutdown.clone();
        let (bound, task) = match tls {
            None => {
                let listener = TcpListener::bind(addr).await.map_err(|e| bind_error(e, addr.port()))?;
                let bound = listener.local_addr()?;
                let task = tokio::spawn(async move {
                    let served = axum::serve(listener, app)
                        .with_graceful_shutdown(async move { stop.cancelled().await })
                        .await;
                    if let Err(e) = served {
                        tracing::warn!("share server ended with an error: {e}");
                    }
                });
                (bound, task)
            }
            Some(material) => {
                // Probe the port first so a conflict is reported as `port_in_use`, not as a late serve error.
                let probe = std::net::TcpListener::bind(addr).map_err(|e| bind_error(e, addr.port()))?;
                let bound = probe.local_addr()?;
                drop(probe);
                // The `ring` provider is installed once per process; a second install is harmless.
                let _ = rustls::crypto::ring::default_provider().install_default();
                let rustls_config = axum_server::tls_rustls::RustlsConfig::from_pem(
                    material.cert_pem.into_bytes(),
                    material.key_pem.into_bytes(),
                )
                .await
                .map_err(|e| NexusError::Internal(format!("invalid HTTPS certificate: {e}")))?;
                let handle = axum_server::Handle::new();
                let watcher = handle.clone();
                tokio::spawn(async move {
                    stop.cancelled().await;
                    watcher.graceful_shutdown(Some(std::time::Duration::from_secs(3)));
                });
                let task = tokio::spawn(async move {
                    let served = axum_server::bind_rustls(bound, rustls_config).handle(handle).serve(app).await;
                    if let Err(e) = served {
                        tracing::warn!("share server ended with an error: {e}");
                    }
                });
                (bound, task)
            }
        };
        let advertiser = mdns_name.and_then(|name| match crate::mdns::Advertiser::start(&name, bound.port()) {
            Ok(a) => Some(a),
            Err(e) => {
                tracing::warn!("could not advertise over Bonjour: {e}");
                None
            }
        });
        tracing::info!(%bound, "LAN share started");
        Ok(Self { addr: bound, shutdown, task, advertiser })
    }

    /// The bound address (useful when the port was 0 in tests).
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Close every socket and release the port.
    pub async fn stop(self) {
        if let Some(a) = self.advertiser {
            a.stop();
        }
        self.shutdown.cancel();
        if tokio::time::timeout(std::time::Duration::from_secs(5), self.task).await.is_err() {
            tracing::warn!("share server did not stop in time");
        }
        tracing::info!("LAN share stopped");
    }
}

fn bind_error(e: std::io::Error, port: u16) -> NexusError {
    match e.kind() {
        std::io::ErrorKind::AddrInUse => NexusError::PortInUse(port),
        _ => NexusError::Io(e),
    }
}

/// First free port at or above `from` (at most 20 probes), for the "Use 9890" suggestion.
pub async fn next_free_port(bind: IpAddr, from: u16) -> Option<u16> {
    for port in from.saturating_add(1)..=from.saturating_add(20) {
        if TcpListener::bind(SocketAddr::new(bind, port)).await.is_ok() {
            return Some(port);
        }
    }
    None
}
