//! The curated model catalog (`catalog/models.json`, embedded) and its schema validation.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

pub const CATALOG_JSON: &str = include_str!("../../../catalog/models.json");
pub const CATALOG_SCHEMA: &str = include_str!("../../../schemas/model-catalog.schema.json");

/// Sampling for one mode (thinking on or off). Unset values use the server defaults.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct Sampling {
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u32>,
    pub min_p: Option<f64>,
    pub repeat_penalty: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct SamplingPresets {
    pub no_think: Sampling,
    #[serde(default)]
    pub think: Option<Sampling>,
}

/// One catalog entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub family: String,
    pub params_b: f64,
    pub quant: String,
    pub repo: String,
    pub revision: String,
    #[serde(default)]
    pub file: Option<String>,
    pub file_pattern: String,
    pub approx_bytes: u64,
    pub license: String,
    #[serde(default)]
    pub context_max: Option<u64>,
    pub context_default: u32,
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub supports_thinking_toggle: bool,
    pub sampling: SamplingPresets,
    #[serde(default)]
    pub min_ram_gb: Option<f64>,
    pub tier: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, rename = "isDefault")]
    pub is_default: bool,
    pub verified: bool,
    #[serde(default)]
    pub why: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// A few plain sentences appended to the system prompt while this model runs (model quirks that a
    /// small model benefits from hearing). Optional; users can turn the hints off in Settings.
    #[serde(default)]
    pub prompt_hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: u32,
    pub updated: String,
    #[serde(default)]
    pub notes: Option<String>,
    pub models: Vec<CatalogEntry>,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("the catalog is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("the catalog does not match its schema: {0}")]
    Schema(String),
    #[error("the catalog must have exactly one default model, found {0}")]
    Defaults(usize),
    #[error("duplicate catalog id `{0}`")]
    DuplicateId(String),
    #[error("catalog entry `{id}` has an invalid file_pattern: {reason}")]
    Pattern { id: String, reason: String },
}

impl Catalog {
    /// Parse and validate catalog JSON: the schema, unique ids, valid regexes, exactly one default.
    pub fn parse(text: &str) -> Result<Self, CatalogError> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        let schema: serde_json::Value = serde_json::from_str(CATALOG_SCHEMA)?;
        let validator =
            jsonschema::validator_for(&schema).map_err(|e| CatalogError::Schema(e.to_string()))?;
        let errors: Vec<String> = validator
            .iter_errors(&value)
            .map(|e| format!("{}: {e}", e.instance_path))
            .collect();
        if !errors.is_empty() {
            return Err(CatalogError::Schema(errors.join("; ")));
        }
        let cat: Catalog = serde_json::from_value(value)?;
        let mut seen = std::collections::BTreeSet::new();
        for m in &cat.models {
            if !seen.insert(m.id.clone()) {
                return Err(CatalogError::DuplicateId(m.id.clone()));
            }
            regex::Regex::new(&m.file_pattern).map_err(|e| CatalogError::Pattern {
                id: m.id.clone(),
                reason: e.to_string(),
            })?;
        }
        let defaults = cat.models.iter().filter(|m| m.is_default).count();
        if defaults != 1 {
            return Err(CatalogError::Defaults(defaults));
        }
        Ok(cat)
    }

    /// The embedded catalog.
    pub fn embedded() -> &'static Catalog {
        static C: OnceLock<Catalog> = OnceLock::new();
        C.get_or_init(|| match Catalog::parse(CATALOG_JSON) {
            Ok(c) => c,
            Err(e) => {
                // The embedded catalog is validated by a test; this keeps the app usable if that ever slips.
                tracing::error!("the embedded model catalog is invalid: {e}");
                Catalog {
                    schema: 1,
                    updated: String::new(),
                    notes: None,
                    models: Vec::new(),
                }
            }
        })
    }

    pub fn get(&self, id: &str) -> Option<&CatalogEntry> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn default_entry(&self) -> Option<&CatalogEntry> {
        self.models.iter().find(|m| m.is_default)
    }

    /// Entries grouped by tier (`tiny`, `small`, `medium`) in catalog order.
    pub fn by_tier(&self) -> BTreeMap<&str, Vec<&CatalogEntry>> {
        let mut m: BTreeMap<&str, Vec<&CatalogEntry>> = BTreeMap::new();
        for e in &self.models {
            m.entry(e.tier.as_str()).or_default().push(e);
        }
        m
    }
}

impl CatalogEntry {
    /// Pick this entry's file from a repo's file names: the exact `file` if present, else the first
    /// name matching `file_pattern` (preferring the first shard of a split model).
    pub fn pick_file<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> Option<String> {
        let names: Vec<&str> = names.into_iter().collect();
        if let Some(f) = &self.file
            && names.contains(&f.as_str())
        {
            return Some(f.clone());
        }
        let re = regex::Regex::new(&self.file_pattern).ok()?;
        let shard = regex::Regex::new(r"-\d{5}-of-\d{5}\.gguf$").ok()?;
        names
            .iter()
            .filter(|n| n.ends_with(".gguf") && re.is_match(&shard.replace(n, ".gguf")))
            .min_by_key(|n| (!n.contains("-00001-of-"), n.len()))
            .map(|n| (*n).to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_catalog_is_valid() {
        let c = Catalog::parse(CATALOG_JSON).unwrap();
        assert_eq!(c.models.len(), 21);
        assert_eq!(
            c.default_entry().map(|d| d.id.as_str()),
            Some("minicpm5-1b-fable5-v2-q8")
        );
        assert!(!Catalog::embedded().models.is_empty());
        assert!(c.by_tier().contains_key("tiny"));
    }

    #[test]
    fn schema_defaults_and_duplicates_are_enforced() {
        let mut v: serde_json::Value = serde_json::from_str(CATALOG_JSON).unwrap();
        v["models"][0]["bogus"] = serde_json::json!(1);
        assert!(matches!(
            Catalog::parse(&v.to_string()),
            Err(CatalogError::Schema(_))
        ));

        let mut v: serde_json::Value = serde_json::from_str(CATALOG_JSON).unwrap();
        v["models"][1]["isDefault"] = serde_json::json!(true);
        assert!(matches!(
            Catalog::parse(&v.to_string()),
            Err(CatalogError::Defaults(2))
        ));

        let mut v: serde_json::Value = serde_json::from_str(CATALOG_JSON).unwrap();
        v["models"][0]["isDefault"] = serde_json::json!(false);
        assert!(matches!(
            Catalog::parse(&v.to_string()),
            Err(CatalogError::Defaults(0))
        ));

        let mut v: serde_json::Value = serde_json::from_str(CATALOG_JSON).unwrap();
        let first = v["models"][0].clone();
        v["models"][1] = first;
        v["models"][1]["isDefault"] = serde_json::json!(false);
        assert!(matches!(
            Catalog::parse(&v.to_string()),
            Err(CatalogError::DuplicateId(_))
        ));

        let mut v: serde_json::Value = serde_json::from_str(CATALOG_JSON).unwrap();
        v["models"][0]["file_pattern"] = serde_json::json!("(unclosed");
        assert!(matches!(
            Catalog::parse(&v.to_string()),
            Err(CatalogError::Pattern { .. })
        ));
    }

    #[test]
    fn file_picking_prefers_the_exact_name_then_the_pattern() {
        let c = Catalog::embedded();
        let e = c.get("minicpm5-1b-fable5-v2-q8").unwrap();
        let names = [
            "README.md",
            "MiniCPM5-1B-Claude-Opus-Fable5-V2-Thinking-F16.gguf",
            "MiniCPM5-1B-Claude-Opus-Fable5-V2-Thinking-Q8_0.gguf",
        ];
        assert_eq!(
            e.pick_file(names).as_deref(),
            Some("MiniCPM5-1B-Claude-Opus-Fable5-V2-Thinking-Q8_0.gguf")
        );
        let renamed = ["model-q8_0.gguf", "model-f16.gguf"];
        assert_eq!(e.pick_file(renamed).as_deref(), Some("model-q8_0.gguf"));
        assert_eq!(e.pick_file(["readme.md"]), None);
        let split = ["m-Q8_0-00002-of-00002.gguf", "m-Q8_0-00001-of-00002.gguf"];
        assert_eq!(e.pick_file(split).as_deref(), Some("m-Q8_0-00001-of-00002.gguf"));
    }
}
