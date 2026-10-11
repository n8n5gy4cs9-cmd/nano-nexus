//! Test helpers shared by the pipeline tests.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use nexus_core::fs::{GrantLevel, GrantScope};
use nexus_fs::{DenyList, MemoryAudit, NewGrant, PermissionEngine};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::{JsonCheck, JsonLlm};
use crate::internal::{PromptSampling, RenderedPrompt};
use crate::tools::EngineGatekeeper;

/// Returns scripted JSON values in order (`None` = the model failed) and records every prompt.
pub struct Scripted {
    pub replies: Mutex<VecDeque<Option<Value>>>,
    pub prompts: Mutex<Vec<RenderedPrompt>>,
}

impl Scripted {
    pub fn new(replies: Vec<Option<Value>>) -> Self {
        Self {
            replies: Mutex::new(replies.into()),
            prompts: Mutex::new(Vec::new()),
        }
    }
    pub fn calls(&self) -> usize {
        self.prompts.lock().map(|p| p.len()).unwrap_or(0)
    }
}

#[async_trait]
impl JsonLlm for Scripted {
    async fn json(
        &self,
        prompt: RenderedPrompt,
        _schema: &Value,
        _s: PromptSampling,
        _check: &JsonCheck,
        _cancel: &CancellationToken,
    ) -> Option<Value> {
        if let Ok(mut p) = self.prompts.lock() {
            p.push(prompt);
        }
        self.replies.lock().ok().and_then(|mut r| r.pop_front()).flatten()
    }
}

/// A gatekeeper with a recursive read grant on `root`.
pub fn gate_for(root: &Path) -> EngineGatekeeper {
    let engine = Arc::new(PermissionEngine::new(
        DenyList::default(),
        Arc::new(MemoryAudit::default()),
    ));
    engine
        .add_grant(NewGrant {
            root: root.to_owned(),
            level: GrantLevel::Read,
            recursive: true,
            scope: GrantScope::Session,
            expires_at: None,
            label: None,
        })
        .unwrap();
    EngineGatekeeper::new(engine, "test", None)
}

/// A gatekeeper with no grants.
pub fn no_grants() -> EngineGatekeeper {
    EngineGatekeeper::new(
        Arc::new(PermissionEngine::new(
            DenyList::default(),
            Arc::new(MemoryAudit::default()),
        )),
        "test",
        None,
    )
}

pub fn write(root: &Path, rel: &str, data: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, data).unwrap();
}
