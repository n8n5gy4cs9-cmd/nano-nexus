//! Prints a built-in assistant as JSON for the evaluation script (`scripts/eval-assistants.mjs`):
//! the rendered system prompt (preamble included) and the model settings from its TOML.
//!   cargo run -q -p nexus-agent --example render_prompt -- nexus
#![allow(clippy::unwrap_used, clippy::expect_used)]

use nexus_agent::registry::{builtin_sources, validate_toml};
use nexus_agent::{PromptContext, render_system_prompt};
use serde_json::json;

fn main() {
    let id = std::env::args()
        .nth(1)
        .expect("usage: render_prompt <assistant id>");
    let (_, text) = builtin_sources()
        .into_iter()
        .find(|(file, _)| file.trim_end_matches(".toml") == id)
        .unwrap_or_else(|| panic!("no built-in assistant `{id}`"));
    let spec = validate_toml(text).expect("the built-in assistants are valid");
    let ctx = PromptContext::new("Mac", "2026-10-12");
    let system = render_system_prompt(&spec, &ctx).expect("the prompt renders");
    println!(
        "{}",
        json!({
            "id": spec.id,
            "name": spec.name,
            "workflow": spec.workflow.kind,
            "system": system,
            "model": {
                "thinking": spec.model.thinking,
                "temperature": spec.model.temperature,
                "top_p": spec.model.top_p,
                "min_p": spec.model.min_p,
                "top_k": spec.model.top_k,
                "repeat_penalty": spec.model.repeat_penalty,
                "max_tokens": spec.model.max_tokens,
            }
        })
    );
}
