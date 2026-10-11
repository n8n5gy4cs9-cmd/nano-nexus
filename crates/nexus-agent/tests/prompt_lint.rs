//! `prompt_lint` (ASSISTANTS §8.1): every built-in assistant follows the small-model prompt rules.

use nexus_agent::registry::{MAX_PROMPT_TOKENS, builtin_sources, validate_toml};
use nexus_agent::{PromptContext, estimate_tokens, render_system_prompt};

const ICONS: &str = include_str!("../../../schemas/assistant-icons.txt");

fn icons() -> Vec<&'static str> {
    ICONS
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

#[test]
fn there_are_eighteen_builtins() {
    assert_eq!(builtin_sources().len(), 18);
}

#[test]
fn every_builtin_passes_prompt_lint() {
    let icons = icons();
    let mut failures = Vec::new();
    for (file, text) in builtin_sources() {
        let spec = match validate_toml(text) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("{file}: schema: {e:?}"));
                continue;
            }
        };
        if format!("{}.toml", spec.id) != file {
            failures.push(format!("{file}: id `{}` must equal the file name", spec.id));
        }
        let tokens = estimate_tokens(&spec.prompt.system);
        if tokens > MAX_PROMPT_TOKENS {
            failures.push(format!(
                "{file}: system prompt is {tokens} tokens (max {MAX_PROMPT_TOKENS})"
            ));
        }
        if !(1..=4).contains(&spec.ui.starters.len()) {
            failures.push(format!(
                "{file}: needs 1-4 starters, has {}",
                spec.ui.starters.len()
            ));
        }
        if !icons.contains(&spec.icon.as_str()) {
            failures.push(format!(
                "{file}: icon `{}` is not in schemas/assistant-icons.txt",
                spec.icon
            ));
        }
        if spec.workflow.kind == "pipeline" && spec.workflow.pipeline.is_none() {
            failures.push(format!("{file}: pipeline workflow without a pipeline id"));
        }
        let mut ctx = PromptContext::new("Mac", "2026-10-11");
        ctx.granted_roots = vec!["/Users/me/Downloads".into()];
        if let Err(e) = render_system_prompt(&spec, &ctx) {
            failures.push(format!("{file}: template variables do not resolve: {e}"));
        }
    }
    assert!(
        failures.is_empty(),
        "prompt_lint failures:\n{}",
        failures.join("\n")
    );
}

#[test]
fn the_example_and_template_files_are_not_registered() {
    let names: Vec<String> = builtin_sources().into_iter().map(|(n, _)| n).collect();
    assert!(!names.iter().any(|n| n.starts_with('_') || n.contains("example")));
}
