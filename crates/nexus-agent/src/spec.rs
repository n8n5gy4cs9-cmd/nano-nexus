//! Assistant definitions (ADR-011): TOML data matching `schemas/assistant.schema.json`.
//!
//! M04 only needs to load and render the built-in Nexus assistant. T06.01 adds schema validation, the
//! full registry, and user assistants on top of this type.

use serde::{Deserialize, Serialize};

/// Sampling for the final (streamed) answer. Values are relative intents; the effective temperature
/// is also capped by the model's recommendation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields, default)]
pub struct ModelParams {
    pub thinking: bool,
    pub temperature: f32,
    pub top_p: f32,
    pub min_p: f32,
    pub top_k: u32,
    pub repeat_penalty: f32,
    pub max_tokens: u32,
}

impl Default for ModelParams {
    fn default() -> Self {
        Self {
            thinking: false,
            temperature: 0.7,
            top_p: 0.95,
            min_p: 0.0,
            top_k: 40,
            repeat_penalty: 1.0,
            max_tokens: 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields, default)]
pub struct ContextSpec {
    /// `text | file | folder | selection | none`
    pub attach: Vec<String>,
    pub max_attach_tokens: u32,
    pub history_turns: u32,
}

impl Default for ContextSpec {
    fn default() -> Self {
        Self {
            attach: vec!["text".into()],
            max_attach_tokens: 3000,
            history_turns: 8,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields, default)]
pub struct PermissionsSpec {
    /// `read | write | create | move | trash`
    pub needs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct WorkflowSpec {
    /// `chat | router | pipeline`
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_hops: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_route: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields, default)]
pub struct ToolsSpec {
    pub allowed: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields, default)]
pub struct UiSpec {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub starters: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct PromptSpec {
    pub system: String,
}

/// One assistant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct AssistantSpec {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub icon: String,
    pub color: String,
    pub category: String,
    pub tagline: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub model: ModelParams,
    #[serde(default)]
    pub context: ContextSpec,
    #[serde(default)]
    pub permissions: PermissionsSpec,
    pub workflow: WorkflowSpec,
    #[serde(default)]
    pub tools: ToolsSpec,
    #[serde(default)]
    pub ui: UiSpec,
    pub prompt: PromptSpec,
}

/// Why an assistant could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum SpecError {
    #[error("invalid assistant TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("unsupported schema_version {0} (this build reads 1)")]
    SchemaVersion(u32),
}

impl AssistantSpec {
    /// Parse an assistant from its TOML text.
    pub fn from_toml(text: &str) -> Result<Self, SpecError> {
        let spec: AssistantSpec = toml::from_str(text)?;
        if spec.schema_version != 1 {
            return Err(SpecError::SchemaVersion(spec.schema_version));
        }
        Ok(spec)
    }
}

/// The built-in default assistant (full registry: M06).
pub const NEXUS_TOML: &str = include_str!("../../../assistants/nexus.toml");

/// Parse the built-in Nexus assistant.
pub fn builtin_nexus() -> Result<AssistantSpec, SpecError> {
    AssistantSpec::from_toml(NEXUS_TOML)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nexus_parses() {
        let n = builtin_nexus().unwrap();
        assert_eq!(n.id, "nexus");
        assert_eq!(n.workflow.kind, "router");
        assert_eq!(n.model.max_tokens, 1024);
        assert_eq!(n.context.history_turns, 8);
        assert_eq!(n.ui.starters.len(), 4);
        assert!(n.prompt.system.contains("{{ granted_roots"));
    }

    #[test]
    fn unknown_keys_and_bad_versions_are_rejected() {
        let base = NEXUS_TOML.replace("schema_version = 1", "schema_version = 2");
        assert!(matches!(
            AssistantSpec::from_toml(&base),
            Err(SpecError::SchemaVersion(2))
        ));
        let extra = NEXUS_TOML.replace("[model]", "bogus = 1\n[model]");
        assert!(AssistantSpec::from_toml(&extra).is_err());
    }
}
