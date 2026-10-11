//! Assistants: the registry (built-ins + user ones) behind `assistants_*` commands.

use nexus_agent::registry::{
    self, Entry, FieldError, Overrides, Registry, Source, ValidationReport, pipeline_ready, slug,
};
use nexus_agent::spec::AssistantSpec;
use nexus_core::error::NexusError;
use serde::Serialize;
use specta::Type;

use crate::state::AppCtx;

/// What the gallery and switcher show.
#[derive(Clone, Debug, PartialEq, Serialize, Type)]
pub struct AssistantCard {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub color: String,
    pub category: String,
    pub tagline: String,
    pub description: String,
    pub version: Option<String>,
    pub source: Source,
    pub enabled: bool,
    pub sort: i32,
    /// Access the assistant may ask for (`read`, `write`, ...), shown as chips.
    pub needs: Vec<String>,
    /// `chat | router | pipeline`
    pub workflow: String,
    pub pipeline: Option<String>,
    pub starters: Vec<String>,
    pub placeholder: Option<String>,
    /// `false` while its pipeline is not built yet (the card is disabled).
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

/// The full definition, for the builder.
#[derive(Clone, Debug, PartialEq, Serialize, Type)]
pub struct AssistantDetail {
    pub card: AssistantCard,
    pub spec: AssistantSpec,
    pub toml: String,
}

fn card(e: &Entry) -> AssistantCard {
    let s = &e.spec;
    let pipeline = s.workflow.pipeline.clone();
    let reason = (s.workflow.kind == "pipeline")
        .then(|| pipeline.as_deref().map(pipeline_ready))
        .flatten()
        .and_then(Result::err);
    AssistantCard {
        id: s.id.clone(),
        name: s.name.clone(),
        icon: s.icon.clone(),
        color: s.color.clone(),
        category: s.category.clone(),
        tagline: s.tagline.clone(),
        description: s.description.clone(),
        version: s.version.clone(),
        source: e.source,
        enabled: e.enabled,
        sort: e.sort,
        needs: s.permissions.needs.clone(),
        workflow: s.workflow.kind.clone(),
        pipeline,
        starters: s.ui.starters.clone(),
        placeholder: s.ui.placeholder.clone(),
        available: reason.is_none(),
        unavailable_reason: reason.map(str::to_owned),
    }
}

fn detail(e: &Entry) -> AssistantDetail {
    AssistantDetail {
        card: card(e),
        spec: e.spec.clone(),
        toml: e.toml.clone(),
    }
}

/// Built-ins plus the stored user assistants and overrides.
pub fn load_registry(store: &nexus_store::Store) -> Registry {
    let mut reg = Registry::builtin();
    for p in reg.problems() {
        tracing::error!(file = %p.origin, errors = ?p.errors, "a built-in assistant failed validation");
    }
    match store.list_assistant_rows() {
        Ok(rows) => {
            for r in rows {
                if r.source == "user" {
                    if let Err(e) = reg.add_user(&r.toml, r.enabled, r.sort) {
                        tracing::warn!(id = %r.id, errors = ?e, "skipping a user assistant that no longer validates");
                    }
                } else {
                    reg.apply_overrides(
                        &r.id,
                        Overrides {
                            enabled: r.enabled,
                            sort: r.sort,
                        },
                    );
                }
            }
        }
        Err(e) => tracing::warn!("could not load assistants: {e}"),
    }
    reg
}

fn registry_read<T>(ctx: &AppCtx, f: impl FnOnce(&Registry) -> T) -> Result<T, NexusError> {
    let g = ctx
        .registry
        .read()
        .map_err(|_| NexusError::Internal("assistant registry lock poisoned".into()))?;
    Ok(f(&g))
}

fn registry_write<T>(ctx: &AppCtx, f: impl FnOnce(&mut Registry) -> T) -> Result<T, NexusError> {
    let mut g = ctx
        .registry
        .write()
        .map_err(|_| NexusError::Internal("assistant registry lock poisoned".into()))?;
    Ok(f(&mut g))
}

fn field_error(errors: Vec<FieldError>) -> NexusError {
    let first = errors.first().map(|e| {
        if e.path.is_empty() {
            e.message.clone()
        } else {
            format!("{}: {}", e.path, e.message)
        }
    });
    NexusError::InvalidInput(first.unwrap_or_else(|| "the assistant is not valid".into()))
}

pub fn list(ctx: &AppCtx) -> Result<Vec<AssistantCard>, NexusError> {
    registry_read(ctx, |r| r.list().into_iter().map(card).collect())
}

pub fn get(ctx: &AppCtx, id: &str) -> Result<AssistantDetail, NexusError> {
    registry_read(ctx, |r| r.get(id).map(detail))?.ok_or_else(|| NexusError::NotFound("assistant".into()))
}

/// The spec for chat. `draft` (the builder's unsaved TOML) wins when given.
pub fn resolve(ctx: &AppCtx, id: &str, draft: Option<&str>) -> Result<AssistantSpec, NexusError> {
    if let Some(text) = draft {
        return registry::validate_toml(text).map_err(field_error);
    }
    registry_read(ctx, |r| {
        r.get(id)
            .filter(|e| e.enabled || e.source == Source::User || id == "nexus")
            .map(|e| e.spec.clone())
    })?
    .ok_or_else(|| NexusError::NotFound("assistant".into()))
}

/// Live builder validation: field errors plus the prompt's token count.
pub fn validate(text: &str) -> ValidationReport {
    registry::validate_report(text)
}

/// Create or update a user assistant. `previous_id` is the id being edited (so a rename replaces it).
pub fn save(ctx: &AppCtx, text: &str, previous_id: Option<&str>) -> Result<AssistantDetail, NexusError> {
    let spec = registry::validate_toml(text).map_err(field_error)?;
    let creating = previous_id != Some(spec.id.as_str());
    if creating && registry_read(ctx, |r| r.get(&spec.id).is_some())? {
        return Err(NexusError::Conflict(format!(
            "an assistant with the id `{}` already exists",
            spec.id
        )));
    }
    let (enabled, sort) = registry_read(ctx, |r| {
        r.get(previous_id.unwrap_or(&spec.id))
            .map(|e| (e.enabled, e.sort))
    })?
    .unwrap_or((true, 1000));
    if let Some(prev) = previous_id.filter(|p| *p != spec.id) {
        remove_user(ctx, prev)?;
    }
    registry_write(ctx, |r| r.add_user(text, enabled, sort))?.map_err(field_error)?;
    ctx.store.save_user_assistant(&spec.id, text, enabled, sort)?;
    get(ctx, &spec.id)
}

fn remove_user(ctx: &AppCtx, id: &str) -> Result<(), NexusError> {
    match registry_write(ctx, |r| r.remove(id))? {
        Some(_) => {
            ctx.store.delete_user_assistant(id)?;
            Ok(())
        }
        None if registry_read(ctx, |r| r.get(id).is_some())? => Err(NexusError::InvalidInput(
            "built-in assistants cannot be deleted; disable them instead".into(),
        )),
        None => Err(NexusError::NotFound("assistant".into())),
    }
}

pub fn delete(ctx: &AppCtx, id: &str) -> Result<(), NexusError> {
    remove_user(ctx, id)
}

/// Copy any assistant (built-in or user) into a new user assistant named `name`.
pub fn duplicate(ctx: &AppCtx, id: &str, name: Option<&str>) -> Result<AssistantDetail, NexusError> {
    let mut spec = registry_read(ctx, |r| r.get(id).map(|e| e.spec.clone()))?
        .ok_or_else(|| NexusError::NotFound("assistant".into()))?;
    let name = name
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{} copy", spec.name));
    spec.name = name.chars().take(32).collect();
    spec.id = registry_read(ctx, |r| r.unique_id(&slug(&spec.name)))?;
    spec.version = Some("1.0.0".into());
    let text = toml::to_string(&spec).map_err(|e| NexusError::Internal(e.to_string()))?;
    save(ctx, &text, None)
}

pub fn set_enabled(ctx: &AppCtx, id: &str, enabled: bool) -> Result<AssistantCard, NexusError> {
    if id == "nexus" && !enabled {
        return Err(NexusError::InvalidInput(
            "Nexus is the default assistant and cannot be disabled".into(),
        ));
    }
    let (source, sort) = registry_read(ctx, |r| r.get(id).map(|e| (e.source, e.sort)))?
        .ok_or_else(|| NexusError::NotFound("assistant".into()))?;
    match source {
        Source::Builtin => ctx.store.set_builtin_override(id, enabled, sort)?,
        Source::User => {
            ctx.store.set_user_assistant_state(id, enabled, sort)?;
        }
    }
    registry_write(ctx, |r| r.apply_overrides(id, Overrides { enabled, sort }))?;
    registry_read(ctx, |r| r.get(id).map(card))?.ok_or_else(|| NexusError::NotFound("assistant".into()))
}

/// The TOML text to write to a `.toml` file.
pub fn export(ctx: &AppCtx, id: &str) -> Result<String, NexusError> {
    registry_read(ctx, |r| r.get(id).map(|e| e.toml.clone()))?
        .ok_or_else(|| NexusError::NotFound("assistant".into()))
}

/// Import a `.toml` file. An id that is already taken gets a numeric suffix instead of overwriting.
pub fn import(ctx: &AppCtx, text: &str) -> Result<AssistantDetail, NexusError> {
    let mut spec = registry::validate_toml(text).map_err(field_error)?;
    let id = registry_read(ctx, |r| r.unique_id(&spec.id))?;
    if id == spec.id {
        return save(ctx, text, None);
    }
    spec.id = id;
    let text = toml::to_string(&spec).map_err(|e| NexusError::Internal(e.to_string()))?;
    save(ctx, &text, None)
}

/// Serialize a spec edited in the builder.
pub fn render_toml(spec: &AssistantSpec) -> Result<String, NexusError> {
    toml::to_string(spec).map_err(|e| NexusError::Internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    const MINE: &str = "schema_version = 1\nid = \"regex-smith\"\nname = \"Regex Smith\"\nicon = \"regex\"\ncolor = \"amber\"\ncategory = \"Custom\"\ntagline = \"Builds regexes.\"\n[workflow]\nkind = \"chat\"\n[prompt]\nsystem = '''\nYour job: write and explain regular expressions.\n'''\n";

    #[tokio::test]
    async fn gallery_lists_builtins_and_marks_unbuilt_pipelines() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        let cards = list(&ctx).unwrap();
        assert_eq!(cards.len(), 17);
        assert_eq!(cards[0].id, "nexus");
        assert!(cards[0].available);
        let scout = cards.iter().find(|c| c.id == "folder-scout").unwrap();
        assert!(!scout.available);
        assert_eq!(scout.unavailable_reason.as_deref(), Some("Available in M08"));
        assert_eq!(
            cards
                .iter()
                .find(|c| c.id == "rename-forge")
                .unwrap()
                .unavailable_reason
                .as_deref(),
            Some("Available in M09")
        );
    }

    #[tokio::test]
    async fn user_assistants_save_survive_restart_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        let saved = save(&ctx, MINE, None).unwrap();
        assert_eq!(saved.card.source, Source::User);
        // A second create with the same id is a conflict, an edit is not.
        assert!(matches!(save(&ctx, MINE, None), Err(NexusError::Conflict(_))));
        assert!(
            save(
                &ctx,
                &MINE.replace("Builds regexes.", "Builds better regexes."),
                Some("regex-smith")
            )
            .is_ok()
        );
        // "Restart": rebuild the registry from the store.
        let again = load_registry(&ctx.store);
        assert_eq!(
            again.get("regex-smith").map(|e| e.spec.tagline.as_str()),
            Some("Builds better regexes.")
        );
        delete(&ctx, "regex-smith").unwrap();
        assert!(get(&ctx, "regex-smith").is_err());
        assert!(load_registry(&ctx.store).get("regex-smith").is_none());
    }

    #[tokio::test]
    async fn built_ins_cannot_be_deleted_or_shadowed_but_can_be_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        assert!(matches!(delete(&ctx, "tldr"), Err(NexusError::InvalidInput(_))));
        let shadow = MINE.replace("regex-smith", "tldr");
        assert!(save(&ctx, &shadow, None).is_err());
        assert!(!set_enabled(&ctx, "tldr", false).unwrap().enabled);
        assert!(!load_registry(&ctx.store).get("tldr").unwrap().enabled);
        assert!(set_enabled(&ctx, "nexus", false).is_err());
    }

    #[tokio::test]
    async fn duplicate_and_import_pick_fresh_ids() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        let copy = duplicate(&ctx, "tldr", None).unwrap();
        assert_eq!(copy.card.id, "tl-dr-copy");
        assert_eq!(copy.card.source, Source::User);
        let again = duplicate(&ctx, "tldr", None).unwrap();
        assert_eq!(again.card.id, "tl-dr-copy-2");
        let a = import(&ctx, MINE).unwrap();
        let b = import(&ctx, MINE).unwrap();
        assert_eq!(
            (a.card.id.as_str(), b.card.id.as_str()),
            ("regex-smith", "regex-smith-2")
        );
        assert!(
            export(&ctx, "regex-smith")
                .unwrap()
                .contains("regular expressions")
        );
    }

    #[tokio::test]
    async fn broken_files_report_field_errors_and_drafts_win_in_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        let bad = MINE.replace("color = \"amber\"", "color = \"puce\"");
        assert!(!validate(&bad).ok);
        assert!(validate(&bad).errors.iter().any(|e| e.path == "/color"));
        assert!(resolve(&ctx, "nexus", Some(&bad)).is_err());
        assert_eq!(resolve(&ctx, "nexus", Some(MINE)).unwrap().id, "regex-smith");
        assert_eq!(resolve(&ctx, "nexus", None).unwrap().id, "nexus");
        assert!(resolve(&ctx, "nope", None).is_err());
    }
}
