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

/// The `{# meta ... #}` header of a prompt file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PromptMeta {
    pub purpose: String,
    pub used_by: String,
    pub output: String,
    pub sampling: String,
    /// Names of the declared template variables (the identifier before any explanation).
    pub vars: Vec<String>,
}

/// Split `a, b (c, d), e` at top-level commas and keep each item's leading identifier.
fn var_names(list: &str) -> Vec<String> {
    let mut depth = 0i32;
    let mut item = String::new();
    let mut out = Vec::new();
    let flush = |item: &mut String, out: &mut Vec<String>| {
        let name: String = item.trim().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        if !name.is_empty() {
            out.push(name);
        }
        item.clear();
    };
    for c in list.chars() {
        match c {
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            ',' if depth == 0 => {
                flush(&mut item, &mut out);
                continue;
            }
            _ => {}
        }
        item.push(c);
    }
    flush(&mut item, &mut out);
    out
}

/// Parse the meta header (empty when absent).
pub fn parse_meta(src: &str) -> PromptMeta {
    let t = src.trim_start();
    let mut meta = PromptMeta::default();
    let Some(rest) = t.strip_prefix("{#") else { return meta };
    let Some(end) = rest.find("#}") else { return meta };
    for line in rest[..end].lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.trim();
        match k.trim() {
            "purpose" => meta.purpose = v.to_owned(),
            "used_by" => meta.used_by = v.to_owned(),
            "output" => meta.output = v.to_owned(),
            "sampling" => meta.sampling = v.to_owned(),
            "vars" => meta.vars = var_names(v),
            _ => {}
        }
    }
    meta
}

/// Sections in file order: lines that are exactly `=== name ===` start one.
fn sections(src: &str) -> Vec<(&str, &str)> {
    let body = strip_meta(src);
    let mut marks: Vec<(usize, usize, &str)> = Vec::new();
    let mut pos = 0;
    for line in body.split_inclusive('\n') {
        let t = line.trim();
        if let Some(name) = t.strip_prefix("=== ").and_then(|r| r.strip_suffix(" ===")) {
            marks.push((pos, pos + line.len(), name.trim()));
        }
        pos += line.len();
    }
    marks
        .iter()
        .enumerate()
        .map(|(i, &(_, text_start, name))| {
            let text_end = marks.get(i + 1).map_or(body.len(), |m| m.0);
            (name, body[text_start..text_end].trim())
        })
        .collect()
}

/// Split a prompt file into its (system, user) template text; example sections are ignored here.
pub fn split_sections(src: &str) -> Result<(&str, &str), InternalPromptError> {
    let secs = sections(src);
    let find = |n: &'static str| secs.iter().find(|(name, _)| *name == n).map(|(_, t)| *t).ok_or(InternalPromptError::MissingSection(n));
    Ok((find("system")?, find("user")?))
}

/// A fully rendered prompt: system, few-shot example turns, and the user message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedPrompt {
    pub system: String,
    /// `(user, assistant)` pairs sent as real prior turns.
    pub examples: Vec<(String, String)>,
    pub user: String,
}

/// Every embedded internal prompt, by file stem (`router`, `doc_qa`, ...).
static INTERNAL_DIR: include_dir::Dir<'static> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../prompts/internal");

/// The text of the embedded prompt `name` (without `.md`).
pub fn internal_source(name: &str) -> Option<&'static str> {
    INTERNAL_DIR.get_file(format!("{name}.md"))?.contents_utf8()
}

/// All embedded prompt names, sorted.
pub fn internal_names() -> Vec<String> {
    let mut v: Vec<String> = INTERNAL_DIR
        .files()
        .filter_map(|f| f.path().file_stem()?.to_str().map(str::to_owned))
        .collect();
    v.sort();
    v
}

/// Render the full prompt (system, examples, user). Declared variables that `vars` omits render as an
/// empty string, so optional ones (`user_question`) work with `{% if %}`; anything undeclared is an error.
pub fn render_full<V: Serialize>(src: &str, vars: &V) -> Result<RenderedPrompt, InternalPromptError> {
    let meta = parse_meta(src);
    let mut value = serde_json::to_value(vars).unwrap_or(serde_json::Value::Null);
    if let Some(obj) = value.as_object_mut() {
        for name in &meta.vars {
            obj.entry(name.clone()).or_insert_with(|| serde_json::Value::String(String::new()));
        }
    }
    let secs = sections(src);
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    for (i, (name, text)) in secs.iter().enumerate() {
        env.add_template_owned(format!("{i}:{name}"), (*text).to_owned())?;
    }
    let v = Value::from(Serde(&value));
    let mut system = None;
    let mut user = None;
    let mut examples = Vec::new();
    let mut pending: Option<String> = None;
    for (i, (name, _)) in secs.iter().enumerate() {
        let out = env.get_template(&format!("{i}:{name}"))?.render(&v)?.trim().to_owned();
        match *name {
            "system" => system = Some(out),
            "user" => user = Some(out),
            "example_user" => pending = Some(out),
            "example_assistant" => {
                if let Some(u) = pending.take() {
                    examples.push((u, out));
                }
            }
            _ => {}
        }
    }
    Ok(RenderedPrompt {
        // `reask.md` is a lone user message: no system section.
        system: system.unwrap_or_default(),
        examples,
        user: user.ok_or(InternalPromptError::MissingSection("user"))?,
    })
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
