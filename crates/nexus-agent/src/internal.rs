//! Internal prompts (`prompts/internal/*.md`): small, single-purpose prompts for background jobs.
//!
//! File format: an optional `{# meta ... #}` header, then `=== system ===` and `=== user ===`
//! sections, both rendered with minijinja.

use minijinja::value::Serde;
use minijinja::{Environment, UndefinedBehavior, Value};
use serde::Serialize;

pub const TITLE_PROMPT: &str = include_str!("../../../prompts/internal/title.md");
pub const HISTORY_SUMMARY_PROMPT: &str = include_str!("../../../prompts/internal/history_summary.md");
pub const TITLE_SCHEMA: &str = include_str!("../../../schemas/llm/title.schema.json");

/// A rendered system + user prompt pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptPair {
    pub system: String,
    pub user: String,
}

/// Why an internal prompt could not be rendered.
#[derive(Debug, thiserror::Error)]
pub enum InternalPromptError {
    #[error("internal prompt is missing its `=== {0} ===` section")]
    MissingSection(&'static str),
    #[error("internal prompt template error: {0}")]
    Template(#[from] minijinja::Error),
}

fn strip_meta(src: &str) -> &str {
    let t = src.trim_start();
    match (t.starts_with("{#"), t.find("#}")) {
        (true, Some(end)) => &t[end + 2..],
        _ => t,
    }
}

/// Split a prompt file into its (system, user) template text.
pub fn split_sections(src: &str) -> Result<(&str, &str), InternalPromptError> {
    let body = strip_meta(src);
    let sys_at = body
        .find("=== system ===")
        .ok_or(InternalPromptError::MissingSection("system"))?;
    let user_at = body
        .find("=== user ===")
        .ok_or(InternalPromptError::MissingSection("user"))?;
    let system = body[sys_at + "=== system ===".len()..user_at.max(sys_at)].trim();
    let user = body[user_at + "=== user ===".len()..].trim();
    Ok((system, user))
}

/// Render both sections with `vars`. Undefined variables are errors.
pub fn render_pair<V: Serialize>(src: &str, vars: &V) -> Result<PromptPair, InternalPromptError> {
    let (system, user) = split_sections(src)?;
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.add_template("system", system)?;
    env.add_template("user", user)?;
    let v = Value::from(Serde(vars));
    Ok(PromptPair {
        system: env.get_template("system")?.render(&v)?.trim().to_owned(),
        user: env.get_template("user")?.render(&v)?.trim().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn title_prompt_renders() {
        let p = render_pair(
            TITLE_PROMPT,
            &json!({"user_text": "How do signals work?", "assistant_text": "They emit events."}),
        )
        .unwrap();
        assert!(p.system.starts_with("Write a short title"));
        assert_eq!(
            p.user,
            "<user>How do signals work?</user>\n<assistant>They emit events.</assistant>"
        );
    }

    #[test]
    fn summary_prompt_handles_an_empty_previous_summary() {
        let turns = json!([{"role": "user", "content": "hi"}, {"role": "assistant", "content": "hello"}]);
        let p = render_pair(
            HISTORY_SUMMARY_PROMPT,
            &json!({"previous_summary": "", "turns": turns.clone()}),
        )
        .unwrap();
        assert!(!p.user.contains("previous_summary"));
        assert!(p.user.contains("[user] hi") && p.user.contains("[assistant] hello"));
        let p = render_pair(
            HISTORY_SUMMARY_PROMPT,
            &json!({"previous_summary": "Earlier stuff", "turns": turns}),
        )
        .unwrap();
        assert!(
            p.user
                .contains("<previous_summary>Earlier stuff</previous_summary>")
        );
    }

    #[test]
    fn missing_sections_and_variables_are_errors() {
        assert!(matches!(
            split_sections("no sections"),
            Err(InternalPromptError::MissingSection(_))
        ));
        assert!(render_pair(TITLE_PROMPT, &json!({"user_text": "x"})).is_err());
    }

    #[test]
    fn every_internal_prompt_file_has_both_sections() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../prompts/internal");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            // reask.md is a single follow-up message template, not a system/user pair.
            if path.file_name().is_some_and(|n| n == "reask.md") {
                continue;
            }
            split_sections(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
    }
}
