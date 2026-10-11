//! Assistant validation and the registry of built-in and user assistants (ASSISTANTS §1).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use include_dir::{Dir, include_dir};
use serde::Serialize;

use crate::budget::estimate_tokens;
use crate::spec::AssistantSpec;

static BUILTIN_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../assistants");

/// The JSON Schema every assistant must match.
pub const ASSISTANT_SCHEMA: &str = include_str!("../../../schemas/assistant.schema.json");

/// Where an assistant came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Builtin,
    User,
}

/// One problem found in an assistant file, tied to the field it concerns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct FieldError {
    /// JSON-pointer-like path (`/model/temperature`); empty for the whole file.
    pub path: String,
    pub message: String,
}

impl FieldError {
    fn whole(message: impl Into<String>) -> Self {
        Self {
            path: String::new(),
            message: message.into(),
        }
    }
}

fn schema_validator() -> Option<&'static jsonschema::Validator> {
    static V: OnceLock<Option<jsonschema::Validator>> = OnceLock::new();
    V.get_or_init(|| {
        let schema: serde_json::Value = serde_json::from_str(ASSISTANT_SCHEMA).ok()?;
        jsonschema::validator_for(&schema).ok()
    })
    .as_ref()
}

/// Validate assistant TOML: syntax, the JSON Schema, then the typed mapping. Returns every problem found
/// (field errors), or the parsed spec.
pub fn validate_toml(text: &str) -> Result<AssistantSpec, Vec<FieldError>> {
    let value: serde_json::Value =
        toml::from_str(text).map_err(|e| vec![FieldError::whole(format!("not valid TOML: {e}"))])?;
    let Some(validator) = schema_validator() else {
        return Err(vec![FieldError::whole(
            "the assistant schema failed to load (this is a bug)",
        )]);
    };
    let errors: Vec<FieldError> = validator
        .iter_errors(&value)
        .map(|e| FieldError {
            path: e.instance_path.to_string(),
            message: e.to_string(),
        })
        .collect();
    if !errors.is_empty() {
        return Err(errors);
    }
    AssistantSpec::from_toml(text).map_err(|e| vec![FieldError::whole(e.to_string())])
}

/// Prompt size state for the builder's meter: green <= 350, amber <= 450, red above.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TokenLevel {
    Green,
    Amber,
    Red,
}

/// Longest system prompt (tokens) a built-in may have (ASSISTANTS §2, R1).
pub const MAX_PROMPT_TOKENS: u32 = 450;

pub fn token_level(tokens: u32) -> TokenLevel {
    match tokens {
        0..=350 => TokenLevel::Green,
        351..=MAX_PROMPT_TOKENS => TokenLevel::Amber,
        _ => TokenLevel::Red,
    }
}

/// Result of `assistant_validate`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct ValidationReport {
    pub ok: bool,
    pub errors: Vec<FieldError>,
    /// Estimated tokens of the system prompt (the runtime's tokenizer is used by the caller when up).
    pub prompt_tokens: u32,
    pub level: TokenLevel,
    pub id: Option<String>,
}

/// Validate and measure `text` with the character-based estimator.
pub fn validate_report(text: &str) -> ValidationReport {
    match validate_toml(text) {
        Ok(spec) => {
            let tokens = estimate_tokens(&spec.prompt.system);
            ValidationReport {
                ok: true,
                errors: vec![],
                prompt_tokens: tokens,
                level: token_level(tokens),
                id: Some(spec.id),
            }
        }
        Err(errors) => {
            // Still measure the prompt when the file parses as TOML, so the meter works while fixing errors.
            let tokens = toml::from_str::<toml::Value>(text)
                .ok()
                .and_then(|v| v.get("prompt")?.get("system")?.as_str().map(estimate_tokens))
                .unwrap_or(0);
            ValidationReport {
                ok: false,
                errors,
                prompt_tokens: tokens,
                level: token_level(tokens),
                id: None,
            }
        }
    }
}

/// `Err(milestone)` while a pipeline's code does not exist yet. The gallery shows such cards disabled.
pub fn pipeline_ready(pipeline: &str) -> Result<(), &'static str> {
    match pipeline {
        "folder_report" | "file_report" | "doc_qa" | "doc_summarize" | "tidy_plan" | "secrets_report"
        | "log_triage" | "data_profile" | "commit_message" => Ok(()),
        "rename_propose" | "strings_improve" => Err("Available in M09"),
        _ => Err("Not available yet"),
    }
}

/// Turn a display name into a kebab-case id.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_owned();
    if out.is_empty() || !out.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        format!("assistant-{out}").trim_end_matches('-').to_owned()
    } else {
        out
    }
}

/// An assistant plus where it came from and its list state.
#[derive(Clone, Debug)]
pub struct Entry {
    pub spec: AssistantSpec,
    pub source: Source,
    /// The exact TOML text (what export writes).
    pub toml: String,
    pub enabled: bool,
    pub sort: i32,
}

/// A problem loading one assistant file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadProblem {
    pub origin: String,
    pub errors: Vec<FieldError>,
}

/// Per-assistant overrides stored in the database (enabled and order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overrides {
    pub enabled: bool,
    pub sort: i32,
}

/// All known assistants, ordered by `sort` then name. Built-in ids cannot be shadowed by user ones.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    entries: BTreeMap<String, Entry>,
    problems: Vec<LoadProblem>,
}

/// File name -> TOML text of every embedded built-in (template and `*.example.toml` files excluded).
pub fn builtin_sources() -> Vec<(String, &'static str)> {
    let mut v: Vec<(String, &'static str)> = BUILTIN_DIR
        .files()
        .filter_map(|f| {
            let name = f.path().file_name()?.to_str()?;
            let hidden = name.starts_with('_') || name.ends_with(".example.toml") || !name.ends_with(".toml");
            (!hidden).then(|| Some((name.to_owned(), f.contents_utf8()?)))?
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

impl Registry {
    /// The built-ins only.
    pub fn builtin() -> Self {
        let mut r = Self::default();
        for (i, (name, text)) in builtin_sources().into_iter().enumerate() {
            match validate_toml(text) {
                Ok(spec) => {
                    r.entries.insert(
                        spec.id.clone(),
                        Entry {
                            spec,
                            source: Source::Builtin,
                            toml: text.to_owned(),
                            enabled: true,
                            sort: (i as i32) * 10,
                        },
                    );
                }
                Err(errors) => r.problems.push(LoadProblem { origin: name, errors }),
            }
        }
        // Nexus is always first.
        if let Some(n) = r.entries.get_mut("nexus") {
            n.sort = -1;
        }
        r
    }

    /// Add a user assistant (TOML text). A built-in id is refused: users duplicate with a new id instead.
    pub fn add_user(&mut self, toml_text: &str, enabled: bool, sort: i32) -> Result<(), Vec<FieldError>> {
        let spec = validate_toml(toml_text)?;
        if self
            .entries
            .get(&spec.id)
            .is_some_and(|e| e.source == Source::Builtin)
        {
            return Err(vec![FieldError {
                path: "/id".into(),
                message: format!("`{}` is a built-in assistant; choose another id", spec.id),
            }]);
        }
        self.entries.insert(
            spec.id.clone(),
            Entry {
                spec,
                source: Source::User,
                toml: toml_text.to_owned(),
                enabled,
                sort,
            },
        );
        Ok(())
    }

    /// Apply stored enabled/order overrides to a built-in.
    pub fn apply_overrides(&mut self, id: &str, o: Overrides) {
        if let Some(e) = self.entries.get_mut(id) {
            e.enabled = o.enabled;
            e.sort = o.sort;
        }
    }

    /// An id based on `base` that no assistant uses yet (`base`, `base-2`, `base-3`, ...).
    pub fn unique_id(&self, base: &str) -> String {
        let base: String = base.chars().take(36).collect();
        if !self.entries.contains_key(&base) {
            return base;
        }
        (2..)
            .map(|n| format!("{base}-{n}"))
            .find(|id| !self.entries.contains_key(id))
            .unwrap_or(base)
    }

    pub fn remove(&mut self, id: &str) -> Option<Entry> {
        if self.entries.get(id).is_some_and(|e| e.source == Source::Builtin) {
            return None;
        }
        self.entries.remove(id)
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.get(id)
    }

    /// Entries ordered by `sort`, then name.
    pub fn list(&self) -> Vec<&Entry> {
        let mut v: Vec<&Entry> = self.entries.values().collect();
        v.sort_by(|a, b| a.sort.cmp(&b.sort).then_with(|| a.spec.name.cmp(&b.spec.name)));
        v
    }

    /// Built-in files that failed validation (empty in a healthy build; `prompt_lint` asserts this).
    pub fn problems(&self) -> &[LoadProblem] {
        &self.problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtins_load() {
        let r = Registry::builtin();
        assert!(r.problems().is_empty(), "{:?}", r.problems());
        assert_eq!(r.list().len(), 19);
        assert_eq!(r.list()[0].spec.id, "nexus");
    }

    #[test]
    fn schema_errors_name_the_field() {
        let bad = crate::spec::NEXUS_TOML.replace("temperature = ", "temperature = 9 #");
        let errs = validate_toml(&bad).unwrap_err();
        assert!(errs.iter().any(|e| e.path == "/model/temperature"), "{errs:?}");
    }

    #[test]
    fn broken_toml_is_one_whole_file_error() {
        let errs = validate_toml("id = ").unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].path.is_empty());
    }

    #[test]
    fn user_assistants_cannot_shadow_built_ins() {
        let mut r = Registry::builtin();
        let err = r.add_user(crate::spec::NEXUS_TOML, true, 5).unwrap_err();
        assert_eq!(err[0].path, "/id");
        let copy = crate::spec::NEXUS_TOML.replace("id = \"nexus\"", "id = \"nexus-copy\"");
        r.add_user(&copy, true, 5).unwrap();
        assert_eq!(r.get("nexus-copy").map(|e| e.source), Some(Source::User));
        assert!(r.remove("nexus").is_none());
        assert!(r.remove("nexus-copy").is_some());
    }

    #[test]
    fn slugs_and_unique_ids() {
        assert_eq!(slug("Regex Smith!"), "regex-smith");
        assert_eq!(slug("  --  "), "assistant");
        assert_eq!(slug("3D Helper"), "assistant-3d-helper");
        let r = Registry::builtin();
        assert_eq!(r.unique_id("tldr"), "tldr-2");
        assert_eq!(r.unique_id("fresh"), "fresh");
    }

    #[test]
    fn serialized_specs_validate_again() {
        let spec = validate_toml(crate::spec::NEXUS_TOML).unwrap();
        let text = toml::to_string(&spec).unwrap();
        assert_eq!(validate_toml(&text).unwrap(), spec);
        for (_, t) in builtin_sources() {
            let s = validate_toml(t).unwrap();
            assert_eq!(validate_toml(&toml::to_string(&s).unwrap()).unwrap(), s);
        }
    }

    #[test]
    fn token_levels() {
        assert_eq!(token_level(350), TokenLevel::Green);
        assert_eq!(token_level(351), TokenLevel::Amber);
        assert_eq!(token_level(451), TokenLevel::Red);
    }

    #[test]
    fn report_measures_even_invalid_files() {
        let bad = crate::spec::NEXUS_TOML.replace("icon = ", "bogus = 1\nicon = ");
        let r = validate_report(&bad);
        assert!(!r.ok && r.prompt_tokens > 0);
        assert!(validate_report(crate::spec::NEXUS_TOML).ok);
    }
}
