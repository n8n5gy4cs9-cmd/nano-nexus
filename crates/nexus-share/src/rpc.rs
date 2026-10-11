//! WebSocket RPC (LAN-SHARE §4.2): one text frame per message, JSON.
//!
//! client → `{"id":7,"cmd":"chat_send","args":{..}}` · `{"id":7,"cancel":true}` · `{"sub":"runtime:status"}`
//! server → `{"id":7,"ok":..}` · `{"id":7,"err":{..}}` · `{"id":7,"event":..}` · `{"push":"..","data":..}`

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use nexus_core::error::{ErrorCode, IpcError};
use nexus_core::share::ShareDevice;
use nexus_core::sink::Sink;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::api::{PUSH_EVENTS, STREAMING, ShareApi, authorize};
use crate::auth::{Auth, AuthEvent};

/// Close the socket after this long without any client frame.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Largest accepted frame.
pub const MAX_FRAME: usize = 2 * 1024 * 1024;

#[derive(Deserialize)]
struct ClientFrame {
    id: Option<u64>,
    cmd: Option<String>,
    #[serde(default)]
    args: Value,
    #[serde(default)]
    cancel: bool,
    sub: Option<String>,
    unsub: Option<String>,
}

/// Streams events of one request into the socket's outgoing queue.
struct WsSink {
    id: u64,
    tx: mpsc::UnboundedSender<Message>,
}

impl Sink<Value> for WsSink {
    fn send(&self, event: Value) -> bool {
        self.tx
            .send(Message::Text(json!({ "id": self.id, "event": event }).to_string().into()))
            .is_ok()
    }
}

fn text(v: Value) -> Message {
    Message::Text(v.to_string().into())
}

fn close(code: u16, reason: &'static str) -> Message {
    Message::Close(Some(CloseFrame {
        code,
        reason: reason.into(),
    }))
}

fn err_frame(id: u64, e: &IpcError) -> Message {
    text(json!({ "id": id, "err": e }))
}

/// Serve one authenticated socket until it closes, goes idle, is revoked, or the server stops.
pub async fn serve_socket(
    socket: WebSocket,
    device: ShareDevice,
    api: Arc<dyn ShareApi>,
    auth: Arc<Auth>,
    shutdown: CancellationToken,
) {
    let (mut write, mut read) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            let last = matches!(m, Message::Close(_));
            if write.send(m).await.is_err() || last {
                break;
            }
        }
    });

    let conn_cancel = CancellationToken::new();
    let mut jobs: HashMap<u64, CancellationToken> = HashMap::new();
    let mut subs: HashSet<String> = HashSet::new();
    let mut push_rx = api.subscribe();
    let mut auth_rx = auth.subscribe();
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<u64>();

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                let _ = tx.send(close(1001, "server stopping"));
                break;
            }
            ev = auth_rx.recv() => {
                if let Ok(AuthEvent::Revoked(id)) = ev {
                    if id == device.id {
                        let _ = tx.send(text(json!({ "push": "share:session_ended", "data": null })));
                        let _ = tx.send(close(4401, "session ended"));
                        break;
                    }
                }
            }
            push = push_rx.recv() => {
                if let Ok(p) = push {
                    if subs.contains(&p.event) {
                        let _ = tx.send(text(json!({ "push": p.event, "data": p.data })));
                    }
                }
            }
            Some(id) = done_rx.recv() => { jobs.remove(&id); }
            frame = tokio::time::timeout(IDLE_TIMEOUT, read.next()) => {
                let msg = match frame {
                    Err(_) => { let _ = tx.send(close(1000, "idle")); break; }
                    Ok(None) | Ok(Some(Err(_))) => break,
                    Ok(Some(Ok(m))) => m,
                };
                match msg {
                    Message::Text(t) => {
                        if t.len() > MAX_FRAME {
                            let _ = tx.send(close(1009, "frame too large"));
                            break;
                        }
                        let Ok(frame) = serde_json::from_str::<ClientFrame>(t.as_str()) else {
                            let _ = tx.send(close(1003, "malformed frame"));
                            break;
                        };
                        if let Some(ev) = frame.sub {
                            if PUSH_EVENTS.contains(&ev.as_str()) { subs.insert(ev); }
                            continue;
                        }
                        if let Some(ev) = frame.unsub {
                            subs.remove(&ev);
                            continue;
                        }
                        let Some(id) = frame.id else {
                            let _ = tx.send(close(1003, "missing id"));
                            break;
                        };
                        if frame.cancel {
                            if let Some(job) = jobs.get(&id) { job.cancel(); }
                            continue;
                        }
                        let Some(cmd) = frame.cmd else {
                            let _ = tx.send(close(1003, "missing cmd"));
                            break;
                        };
                        // Re-read the device so a policy change or revoke applies to the next call.
                        let Some(current) = auth.device(&device.id) else {
                            let _ = tx.send(close(4401, "session ended"));
                            break;
                        };
                        if let Err(e) = authorize(current.policy, &cmd) {
                            let _ = tx.send(err_frame(id, &e));
                            continue;
                        }
                        if jobs.contains_key(&id) {
                            let e = IpcError { code: ErrorCode::Conflict, message: "request id already in use".into(), detail: None };
                            let _ = tx.send(err_frame(id, &e));
                            continue;
                        }
                        let cancel = conn_cancel.child_token();
                        jobs.insert(id, cancel.clone());
                        let api = api.clone();
                        let tx = tx.clone();
                        let done = done_tx.clone();
                        tokio::spawn(async move {
                            let sink: Arc<dyn Sink<Value>> = if STREAMING.contains(&cmd.as_str()) {
                                Arc::new(WsSink { id, tx: tx.clone() })
                            } else {
                                Arc::new(nexus_core::sink::NullSink)
                            };
                            let frame = match api.call(&current, &cmd, frame.args, sink, cancel).await {
                                Ok(v) => text(json!({ "id": id, "ok": v })),
                                Err(e) => err_frame(id, &e),
                            };
                            let _ = tx.send(frame);
                            let _ = done.send(id);
                        });
                    }
                    Message::Binary(_) => {
                        let _ = tx.send(close(1003, "text frames only"));
                        break;
                    }
                    Message::Close(_) => break,
                    Message::Ping(_) | Message::Pong(_) => {}
                }
            }
        }
    }
    // Stops every in-flight request of this connection (the turn is cancelled, the DB keeps what streamed).
    conn_cancel.cancel();
    drop(tx);
    let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
}
