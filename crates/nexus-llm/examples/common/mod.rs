//! Shared setup for the examples: locate the runtime and model, start a supervisor.
#![allow(dead_code)]

use std::path::PathBuf;

use nexus_llm::{RuntimeConfig, Supervisor};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `NEXUS_LLAMA_BIN`, else the macOS arm64 binary from `scripts/setup-runtime.sh`.
pub fn server_bin() -> PathBuf {
    std::env::var_os("NEXUS_LLAMA_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("vendor/runtime/macos-arm64/llama-server-aarch64-apple-darwin"))
}

/// `NEXUS_MODEL`, else the first `.gguf` under `vendor/models`.
pub fn model_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(p) = std::env::var_os("NEXUS_MODEL") {
        return Ok(PathBuf::from(p));
    }
    let mut stack = vec![repo_root().join("vendor/models")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "gguf") {
                return Ok(p);
            }
        }
    }
    Err("no model found: run `node scripts/fetch-model.mjs` or set NEXUS_MODEL".into())
}

/// Start the server and wait until it is ready.
pub async fn start() -> Result<Supervisor, Box<dyn std::error::Error>> {
    let sup = Supervisor::new(None);
    sup.start(RuntimeConfig::new(server_bin(), model_path()?)).await?;
    Ok(sup)
}
