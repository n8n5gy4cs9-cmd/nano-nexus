//! End-to-end checks of the HTTP side: auth, CSRF, headers, port handling (LAN-SHARE §7).
#![allow(clippy::unwrap_used)]

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use async_trait::async_trait;
use nexus_core::error::{ErrorCode, IpcError, NexusError};
use nexus_core::share::ShareDevice;
use nexus_core::sink::Sink;
use nexus_share::{Auth, AuthConfig, Push, ServerConfig, ShareApi, ShareServer};
use nexus_store::Store;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

struct Echo(broadcast::Sender<Push>);

#[async_trait]
impl ShareApi for Echo {
    async fn call(
        &self,
        _device: &ShareDevice,
        cmd: &str,
        _args: Value,
        _sink: Arc<dyn Sink<Value>>,
        _cancel: CancellationToken,
    ) -> Result<Value, IpcError> {
        if cmd == "boom" {
            return Err(IpcError { code: ErrorCode::Internal, message: "boom".into(), detail: None });
        }
        Ok(json!({ "echo": cmd }))
    }

    fn subscribe(&self) -> broadcast::Receiver<Push> {
        self.0.subscribe()
    }
}

fn config(port: u16) -> ServerConfig {
    ServerConfig {
        bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port,
        secure_cookie: false,
        session_days: 30,
        tls: None,
        mdns_name: None,
    }
}

async fn start(port: u16) -> (ShareServer, Arc<Auth>) {
    let auth = Auth::new(Arc::new(Store::open_in_memory().unwrap()), AuthConfig::default());
    let api = Arc::new(Echo(broadcast::channel(4).0));
    let server = ShareServer::start(config(port), auth.clone(), api).await.unwrap();
    (server, auth)
}

fn url(server: &ShareServer, path: &str) -> String {
    format!("http://{}{path}", server.local_addr())
}

async fn pair(client: &reqwest::Client, server: &ShareServer, code: &str) -> reqwest::Response {
    client
        .post(url(server, "/api/pair"))
        .header("x-nexus-csrf", "pair")
        .json(&json!({ "code": code, "name": "Kim's iPhone" }))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn health_is_public_and_carries_security_headers() {
    let (server, _auth) = start(0).await;
    let client = reqwest::Client::new();
    let res = client.get(url(&server, "/api/health")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let h = res.headers();
    assert_eq!(h["x-frame-options"], "DENY");
    assert_eq!(h["referrer-policy"], "no-referrer");
    assert_eq!(h["cache-control"], "no-store");
    assert!(h["content-security-policy"].to_str().unwrap().contains("default-src 'self'"));
    server.stop().await;
}

#[tokio::test]
async fn nothing_is_served_without_a_session() {
    let (server, _auth) = start(0).await;
    let client = reqwest::Client::new();
    assert_eq!(client.get(url(&server, "/api/session")).send().await.unwrap().status(), 401);
    // The WebSocket endpoint never upgrades without a session (a plain GET is rejected).
    assert!(client.get(url(&server, "/api/ws")).send().await.unwrap().status().is_client_error());
    // Unknown API paths never fall through to the web app.
    assert_eq!(client.get(url(&server, "/api/nope")).send().await.unwrap().status(), 404);
    // The web app itself is public (it only contains the pairing screen and code).
    assert_eq!(client.get(url(&server, "/")).send().await.unwrap().status(), 200);
    server.stop().await;
}

#[tokio::test]
async fn pairing_sets_a_locked_down_cookie_and_the_code_is_single_use() {
    let (server, auth) = start(0).await;
    let client = reqwest::Client::new();
    let code = auth.new_pairing_code().unwrap();

    // A custom header is required, which forces a CORS preflight for other sites.
    let no_header = client
        .post(url(&server, "/api/pair"))
        .json(&json!({ "code": code.pin, "name": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(no_header.status(), 403);

    let ok = pair(&client, &server, &code.pin).await;
    assert_eq!(ok.status(), 200);
    let cookie = ok.headers()["set-cookie"].to_str().unwrap().to_owned();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict") && cookie.contains("Path=/"));
    assert!(!cookie.contains("Secure"), "plain HTTP mode has no Secure flag");
    let body: Value = ok.json().await.unwrap();
    assert!(body["csrf"].as_str().is_some_and(|c| c.len() == 64));

    let session = client
        .get(url(&server, "/api/session"))
        .header("cookie", cookie.split(';').next().unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(session.status(), 200);

    // The same code again fails.
    assert_eq!(pair(&client, &server, &code.pin).await.status(), 401);
    server.stop().await;
}

#[tokio::test]
async fn logout_needs_csrf_and_revokes_the_device() {
    let (server, auth) = start(0).await;
    let client = reqwest::Client::new();
    let code = auth.new_pairing_code().unwrap();
    let res = pair(&client, &server, &code.secret).await;
    let cookie = res.headers()["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_owned();
    let csrf = res.json::<Value>().await.unwrap()["csrf"].as_str().unwrap().to_owned();

    let without = client.post(url(&server, "/api/logout")).header("cookie", &cookie).send().await.unwrap();
    assert_eq!(without.status(), 403);
    let wrong = client
        .post(url(&server, "/api/logout"))
        .header("cookie", &cookie)
        .header("x-nexus-csrf", "0".repeat(64))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 403);
    let ok = client
        .post(url(&server, "/api/logout"))
        .header("cookie", &cookie)
        .header("x-nexus-csrf", &csrf)
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);
    assert_eq!(client.get(url(&server, "/api/session")).header("cookie", &cookie).send().await.unwrap().status(), 401);
    server.stop().await;
}

#[tokio::test]
async fn revoking_a_device_ends_its_session() {
    let (server, auth) = start(0).await;
    let client = reqwest::Client::new();
    let code = auth.new_pairing_code().unwrap();
    let res = pair(&client, &server, &code.pin).await;
    let cookie = res.headers()["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_owned();
    let id = res.json::<Value>().await.unwrap()["device"]["id"].as_str().unwrap().to_owned();
    assert!(auth.revoke(&id).unwrap());
    assert_eq!(client.get(url(&server, "/api/session")).header("cookie", &cookie).send().await.unwrap().status(), 401);
    server.stop().await;
}

#[tokio::test]
async fn a_taken_port_is_reported_and_stop_releases_it() {
    let (first, auth) = start(0).await;
    let port = first.local_addr().port();
    let api = Arc::new(Echo(broadcast::channel(4).0));
    let err = ShareServer::start(config(port), auth.clone(), api.clone()).await.err().unwrap();
    assert!(matches!(err, NexusError::PortInUse(p) if p == port));
    first.stop().await;
    let again = ShareServer::start(config(port), auth, api).await.unwrap();
    again.stop().await;
}
