//! The real LAN Share server (HTTP, pairing, cookies, WebSocket, the embedded web UI) in front of a scripted
//! fake backend, for the Playwright end-to-end test (`scripts/e2e/share-e2e.mjs`, T14.15).
//!
//! Prints one JSON line `{"url": "...", "pin": "123456"}` and then reads commands from stdin:
//! `pin` (print a new pairing code), `revoke` (revoke every paired device), `quit`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::io::BufRead;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use nexus_core::error::{ErrorCode, IpcError};
use nexus_core::share::ShareDevice;
use nexus_core::sink::Sink;
use nexus_share::{Auth, AuthConfig, Push, ServerConfig, ShareApi, ShareServer};
use nexus_store::Store;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct World {
    next: u32,
    /// (conversation id, title, messages as JSON)
    conversations: Vec<(String, String, Vec<Value>)>,
    running: HashMap<String, CancellationToken>,
}

struct Fake {
    push: broadcast::Sender<Push>,
    world: Arc<Mutex<World>>,
}

fn err(code: ErrorCode, message: &str) -> IpcError {
    IpcError {
        code,
        message: message.into(),
        detail: None,
    }
}

fn message(id: &str, conv: &str, role: &str, content: &str, status: &str) -> Value {
    json!({
        "id": id, "conversation_id": conv, "role": role, "content": content, "reasoning": null,
        "assistant_id": if role == "assistant" { json!("nexus") } else { Value::Null },
        "model_id": null, "tool_trace": null, "status": status, "error": null,
        "tokens_prompt": null, "tokens_out": null, "tps": null, "ttft_ms": null,
        "created_at": 1, "attachments": []
    })
}

fn card() -> Value {
    json!({
        "id": "nexus", "name": "Nexus", "icon": "hexagon", "color": "violet", "category": "General",
        "tagline": "Your everyday local copilot.", "description": "", "version": null, "source": "builtin",
        "enabled": true, "sort": 0, "needs": [], "workflow": "chat", "pipeline": null,
        "starters": ["Say hello"], "placeholder": null, "available": true, "unavailable_reason": null
    })
}

#[async_trait]
impl ShareApi for Fake {
    async fn call(
        &self,
        _device: &ShareDevice,
        cmd: &str,
        args: Value,
        sink: Arc<dyn Sink<Value>>,
        _cancel: CancellationToken,
    ) -> Result<Value, IpcError> {
        match cmd {
            "app_info" => Ok(
                json!({"version": "0.1.0", "os": "linux", "arch": "x86_64", "data_dir": "", "log_dir": "", "build": null}),
            ),
            "runtime_status" => Ok(
                json!({"state": "ready", "model_id": "fake", "model_path": "", "ctx_size": 8192, "gpu_layers": "all"}),
            ),
            "assistants_list" => Ok(json!([card()])),
            "assistant_get" => Ok(card()),
            "notifications_list" => Ok(json!([])),
            "notifications_unread_count" => Ok(json!(0)),
            "conversations_list" => {
                let w = self.world.lock().unwrap();
                let q = args["query"].as_str().unwrap_or("").to_lowercase();
                let rows: Vec<Value> = w
                    .conversations
                    .iter()
                    .rev()
                    .filter(|(_, title, _)| q.is_empty() || title.to_lowercase().contains(&q))
                    .map(|(id, title, _)| json!({"id": id, "title": title, "assistant_id": "nexus", "pinned": false, "updated_at": 1, "snippet": null}))
                    .collect();
                Ok(json!(rows))
            }
            "conversation_get" => {
                let w = self.world.lock().unwrap();
                let id = args["id"].as_str().unwrap_or("");
                let (cid, title, msgs) = w
                    .conversations
                    .iter()
                    .find(|(c, _, _)| c == id)
                    .ok_or_else(|| err(ErrorCode::NotFound, "no such conversation"))?;
                Ok(json!({
                    "conversation": {"id": cid, "title": title, "assistant_id": "nexus", "pinned": false, "summary": null, "summary_upto": null, "created_at": 1, "updated_at": 1, "archived": false},
                    "messages": msgs
                }))
            }
            "chat_stop" => {
                let id = args["assistantMessageId"].as_str().unwrap_or("");
                let token = self.world.lock().unwrap().running.get(id).cloned();
                if let Some(t) = &token {
                    t.cancel();
                }
                Ok(json!(token.is_some()))
            }
            "chat_send" => {
                let send = &args["args"];
                let text = send["text"].as_str().unwrap_or("").to_owned();
                let (conv, user_id, asst_id, cancel) = {
                    let mut w = self.world.lock().unwrap();
                    w.next += 1;
                    let n = w.next;
                    let conv = send["conversation_id"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            let id = format!("c{n}");
                            let title: String = text.chars().take(40).collect();
                            w.conversations.push((id.clone(), title, Vec::new()));
                            id
                        });
                    let (user_id, asst_id) = (format!("u{n}"), format!("a{n}"));
                    let cancel = CancellationToken::new();
                    w.running.insert(asst_id.clone(), cancel.clone());
                    if let Some((_, _, msgs)) = w.conversations.iter_mut().find(|(c, _, _)| *c == conv) {
                        msgs.push(message(&user_id, &conv, "user", &text, "done"));
                        msgs.push(message(&asst_id, &conv, "assistant", "", "streaming"));
                    }
                    (conv, user_id, asst_id, cancel)
                };
                let world = self.world.clone();
                let (conv2, asst2) = (conv.clone(), asst_id.clone());
                tokio::spawn(async move {
                    sink.send(json!({"type": "started", "assistant_message_id": asst2}));
                    let mut shown = String::new();
                    let words = [
                        "Hello", "from", "the", "fake", "model.", "This", "answer", "streams", "slowly",
                        "so", "the", "test", "can", "press", "Stop", "in", "the", "middle", "of", "it.",
                    ];
                    let mut stopped = false;
                    for w in words {
                        tokio::select! {
                            () = cancel.cancelled() => { stopped = true; break; }
                            () = tokio::time::sleep(Duration::from_millis(250)) => {}
                        }
                        let delta = format!("{w} ");
                        shown.push_str(&delta);
                        sink.send(json!({"type": "content", "delta": delta}));
                    }
                    let status = if stopped { "stopped" } else { "done" };
                    {
                        let mut guard = world.lock().unwrap();
                        guard.running.remove(&asst2);
                        if let Some((_, _, msgs)) =
                            guard.conversations.iter_mut().find(|(c, _, _)| *c == conv2)
                            && let Some(m) = msgs.iter_mut().find(|m| m["id"] == asst2.as_str())
                        {
                            m["content"] = json!(shown.trim_end());
                            m["status"] = json!(status);
                        }
                    }
                    sink.send(json!({"type": "done", "finish_reason": if stopped { "stop_requested" } else { "stop" }, "tokens_out": 5, "tps": 4.0, "ttft_ms": 250}));
                });
                Ok(
                    json!({"conversation_id": conv, "user_message_id": user_id, "assistant_message_id": asst_id}),
                )
            }
            other => Err(err(
                ErrorCode::NotFound,
                &format!("the fake backend has no `{other}`"),
            )),
        }
    }

    fn subscribe(&self) -> broadcast::Receiver<Push> {
        self.push.subscribe()
    }
}

#[tokio::main]
async fn main() {
    let auth = Auth::new(Arc::new(Store::open_in_memory().unwrap()), AuthConfig::default());
    let api = Arc::new(Fake {
        push: broadcast::channel(8).0,
        world: Arc::new(Mutex::new(World::default())),
    });
    let config = ServerConfig {
        bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port: 0,
        secure_cookie: false,
        session_days: 1,
        tls: None,
        mdns_name: None,
    };
    let server = ShareServer::start(config, auth.clone(), api).await.unwrap();
    let print = |auth: &Auth| {
        let code = auth.new_pairing_code().unwrap();
        println!(
            "{}",
            json!({"url": format!("http://{}", server.local_addr()), "pin": code.pin, "secret": code.secret})
        );
    };
    print(&auth);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    while let Some(line) = rx.recv().await {
        match line.trim() {
            "pin" => print(&auth),
            "revoke" => {
                for d in auth.devices().unwrap() {
                    auth.revoke(&d.id).unwrap();
                }
                println!("{}", json!({"revoked": true}));
            }
            "quit" => break,
            _ => {}
        }
    }
    server.stop().await;
}
