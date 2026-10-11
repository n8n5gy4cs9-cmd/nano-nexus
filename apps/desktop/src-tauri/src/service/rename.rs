//! Rename Forge service (M09): index a project, list naming issues, ask the model for names, keep the
//! user's decisions, plan the accepted renames as a change set, and verify with the project's checker.
//! Nothing here writes user files; a plan is only a change set the user reviews on the Changes page.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nexus_agent::pipelines::rename::{ProposeOptions, propose};
use nexus_agent::tools::Gatekeeper;
use nexus_agent::pipelines::{PipelineCx, SchedulerJson};
use nexus_analyze::code::index::{Flags, IndexOptions, build_index};
use nexus_analyze::code::plan::{PlanOptions, RenameItem, plan_renames};
use nexus_analyze::code::propose::{ProposalStatus, Validator};
use nexus_analyze::code::rules::{RuleOptions, detect};
use nexus_analyze::code::verify::{Checker, VerifyResult};
use nexus_analyze::code::{CodeIndex, SymbolId};
use nexus_core::error::NexusError;
use nexus_core::events::{NotificationAction, NotificationLevel};
use nexus_core::fs::Access;
use nexus_core::ids::MessageId;
use nexus_store::{NewNotification, RenameItemPatch, RenameItemRow, RenameSessionRow};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

use super::pipelines::{gatekeeper, home_dir, pipeline_err, tool_err};
use crate::state::{AppCtx, AppEvent};

const STATUSES: &[&str] = &["candidate", "proposed", "accepted", "rejected", "needs_manual", "applied"];

/// Loaded indexes and running jobs.
#[derive(Default)]
pub struct RenameRuntime {
    indexes: Mutex<HashMap<String, Arc<CodeIndex>>>,
    jobs: Mutex<HashMap<String, CancellationToken>>,
}

impl RenameRuntime {
    fn add_job(&self, id: &str, t: CancellationToken) {
        if let Ok(mut m) = self.jobs.lock() {
            m.insert(id.to_owned(), t);
        }
    }
    fn end_job(&self, id: &str) {
        if let Ok(mut m) = self.jobs.lock() {
            m.remove(id);
        }
    }
    /// Cancel a running index, proposal, or verify job. `false` when there is none with that id.
    pub fn cancel(&self, id: &str) -> bool {
        match self.jobs.lock().ok().and_then(|m| m.get(id).cloned()) {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }
}

/// Progress of an index or proposal job.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct RenameProgress {
    /// `indexing | detecting | proposing | done`
    pub phase: String,
    pub done: u32,
    pub total: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct RenameSessionView {
    pub id: String,
    pub root: String,
    pub status: String,
    pub langs: Vec<String>,
    pub files: u32,
    pub symbols: u32,
    pub issues: u32,
    pub created_at: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct RenameItemView {
    pub symbol_id: String,
    pub old_name: String,
    pub kind: String,
    pub path: String,
    pub line: u32,
    pub proposed: Option<String>,
    pub final_name: Option<String>,
    pub confidence: Option<f32>,
    pub reason: Option<String>,
    pub issues: Vec<String>,
    pub flags: Vec<String>,
    pub status: String,
    pub include_strings: bool,
    /// Occurrences besides the definition.
    pub usages: u32,
    /// Why the last edit was refused (the stored values are unchanged).
    pub error: Option<String>,
}

/// What the planner produced.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct RenamePlanView {
    pub changeset_id: String,
    pub edits: u32,
    pub skipped: Vec<String>,
    /// Project-relative files with uncommitted git changes that the plan touches.
    pub dirty: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Type)]
pub struct RenamePlanOptions {
    pub docs: bool,
    pub comments: bool,
    pub ambiguous: bool,
    pub public_api: bool,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn flag_names(f: Flags) -> Vec<String> {
    [
        (Flags::PUBLIC_API, "public_api"),
        (Flags::AMBIGUOUS, "ambiguous"),
        (Flags::EXPORTED, "exported"),
        (Flags::DYNAMIC_REFS, "dynamic_refs"),
        (Flags::TEST, "test"),
        (Flags::OVERRIDE, "override"),
        (Flags::ENTRY_POINT, "entry_point"),
    ]
    .into_iter()
    .filter(|(bit, _)| f.has(*bit))
    .map(|(_, n)| n.to_owned())
    .collect()
}

fn json_list(items: &[String]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".into())
}

fn parse_list(s: Option<&str>) -> Vec<String> {
    s.and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default()
}

fn session_row(ctx: &AppCtx, id: &str) -> Result<RenameSessionRow, NexusError> {
    ctx.store
        .get_rename_session(id)?
        .ok_or_else(|| NexusError::NotFound(format!("rename session {id}")))
}

fn load_index(ctx: &AppCtx, s: &RenameSessionRow) -> Result<Arc<CodeIndex>, NexusError> {
    if let Some(i) = ctx.renames.indexes.lock().ok().and_then(|m| m.get(&s.id).cloned()) {
        return Ok(i);
    }
    let index = Arc::new(
        CodeIndex::load(Path::new(&s.index_path))
            .map_err(|e| NexusError::InvalidInput(format!("The project index is gone ({e}); index the project again.")))?,
    );
    if let Ok(mut m) = ctx.renames.indexes.lock() {
        m.insert(s.id.clone(), index.clone());
    }
    Ok(index)
}

fn view_of(index: &CodeIndex, r: RenameItemRow, error: Option<String>) -> RenameItemView {
    let sym = r.symbol_id.parse::<u32>().ok().and_then(|n| index.symbol(SymbolId(n)));
    let (path, line, usages) = match sym {
        Some(s) => {
            let path = match s.kind {
                nexus_analyze::code::SymbolKind::File | nexus_analyze::code::SymbolKind::Folder => s.signature.clone(),
                _ => index.path_of(s.def.file).unwrap_or_default().to_owned(),
            };
            (path, s.def.line, s.refs.iter().filter(|x| !x.is_def).count() as u32)
        }
        None => (String::new(), 0, 0),
    };
    RenameItemView {
        symbol_id: r.symbol_id,
        old_name: r.old_name,
        kind: r.kind,
        path,
        line,
        proposed: r.proposed,
        final_name: r.final_name,
        confidence: r.confidence.map(|c| c as f32),
        reason: r.reason,
        issues: parse_list(r.issues.as_deref()),
        flags: parse_list(r.flags.as_deref()),
        status: r.status,
        include_strings: r.include_strings,
        usages,
        error,
    }
}

/// Index `root` (Read access required; asks when missing), find naming issues, and store the session.
pub async fn session_create(
    ctx: &Arc<AppCtx>,
    job_id: &str,
    root: &str,
    progress: Arc<dyn Fn(RenameProgress) + Send + Sync>,
) -> Result<RenameSessionView, NexusError> {
    let gate = gatekeeper(ctx, "user");
    let canon = gate
        .authorize(Path::new(root), Access::Read, "to index this project")
        .await
        .map_err(tool_err)?;
    if !canon.is_dir() {
        return Err(NexusError::InvalidInput(format!("{} is not a folder", canon.display())));
    }
    let id = MessageId::new().0;
    let dir = ctx.paths.cache.join("rename").join(&id);
    std::fs::create_dir_all(&dir)?;
    let index_path = dir.join("index.bin");
    let now = now_ms();
    ctx.store.insert_rename_session(&RenameSessionRow {
        id: id.clone(),
        root: canon.display().to_string(),
        langs: "[]".into(),
        index_path: index_path.display().to_string(),
        status: "indexing".into(),
        created_at: now,
        updated_at: now,
    })?;

    let cancel = CancellationToken::new();
    ctx.renames.add_job(job_id, cancel.clone());
    progress(RenameProgress { phase: "indexing".into(), done: 0, total: 0 });
    let (root_path, save_to, token) = (canon.clone(), index_path.clone(), cancel.clone());
    let built = tokio::task::spawn_blocking(move || {
        let index = build_index(&root_path, &IndexOptions::default(), None, &token)?;
        index.save(&save_to)?;
        Ok::<_, nexus_analyze::code::IndexError>(index)
    })
    .await;
    ctx.renames.end_job(job_id);
    let index = match built.map_err(|e| NexusError::Internal(e.to_string()))? {
        Ok(i) => i,
        Err(e) => {
            let _ = ctx.store.set_rename_session_status(&id, "failed", None);
            return Err(if cancel.is_cancelled() { NexusError::Cancelled } else { NexusError::InvalidInput(e.to_string()) });
        }
    };

    progress(RenameProgress { phase: "detecting".into(), done: 0, total: 0 });
    let cands = detect(&index, &RuleOptions::default());
    let rows: Vec<RenameItemRow> = cands
        .iter()
        .filter_map(|c| {
            let s = index.symbol(c.symbol)?;
            Some(RenameItemRow {
                session_id: id.clone(),
                symbol_id: c.symbol.0.to_string(),
                old_name: s.name.clone(),
                kind: s.kind.label().to_owned(),
                proposed: c.suggested.clone(),
                final_name: None,
                confidence: None,
                reason: None,
                issues: Some(json_list(&c.issues.iter().map(|i| i.rule.clone()).collect::<Vec<_>>())),
                flags: Some(json_list(&flag_names(s.flags))),
                status: "candidate".into(),
                include_strings: false,
            })
        })
        .collect();
    ctx.store.upsert_rename_items(&rows)?;
    let mut langs: Vec<String> = index.files.iter().map(|f| f.lang.name().to_owned()).collect();
    langs.sort();
    langs.dedup();
    ctx.store.set_rename_session_status(&id, "ready", Some(&json_list(&langs)))?;
    let view = RenameSessionView {
        id: id.clone(),
        root: canon.display().to_string(),
        status: "ready".into(),
        langs,
        files: index.files.len() as u32,
        symbols: index.symbols.len() as u32,
        issues: rows.len() as u32,
        created_at: now as f64,
    };
    if let Ok(mut m) = ctx.renames.indexes.lock() {
        m.insert(id, Arc::new(index));
    }
    progress(RenameProgress { phase: "done".into(), done: view.issues, total: view.issues });
    Ok(view)
}

pub fn sessions_list(ctx: &AppCtx, limit: u32) -> Result<Vec<RenameSessionView>, NexusError> {
    Ok(ctx
        .store
        .list_rename_sessions(limit.clamp(1, 100))?
        .into_iter()
        .map(|s| RenameSessionView {
            issues: ctx.store.list_rename_items(&s.id, None).map(|v| v.len() as u32).unwrap_or(0),
            langs: parse_list(Some(&s.langs)),
            id: s.id,
            root: s.root,
            status: s.status,
            files: 0,
            symbols: 0,
            created_at: s.created_at as f64,
        })
        .collect())
}

pub fn session_delete(ctx: &AppCtx, id: &str) -> Result<bool, NexusError> {
    if let Some(s) = ctx.store.get_rename_session(id)? {
        // Only ever remove a folder inside our own cache.
        if let Some(dir) = Path::new(&s.index_path).parent()
            && dir.starts_with(&ctx.paths.cache)
        {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
    if let Ok(mut m) = ctx.renames.indexes.lock() {
        m.remove(id);
    }
    Ok(ctx.store.delete_rename_session(id)?)
}

/// Items of a session, optionally one status (`candidate | proposed | accepted | …`).
pub fn items(ctx: &AppCtx, session_id: &str, status: Option<&str>) -> Result<Vec<RenameItemView>, NexusError> {
    let s = session_row(ctx, session_id)?;
    let index = load_index(ctx, &s)?;
    Ok(ctx
        .store
        .list_rename_items(session_id, status)?
        .into_iter()
        .map(|r| view_of(&index, r, None))
        .collect())
}

/// Ask the model for names. `symbol_ids` = only those items (e.g. "Try again"); `None` = every candidate
/// that has no decision yet. Returns how many proposals were made.
pub async fn propose_names(
    ctx: &Arc<AppCtx>,
    job_id: &str,
    session_id: &str,
    symbol_ids: Option<Vec<String>>,
    try_again: bool,
    progress: Arc<dyn Fn(RenameProgress) + Send + Sync>,
) -> Result<u32, NexusError> {
    let s = session_row(ctx, session_id)?;
    let index = load_index(ctx, &s)?;
    let stored = ctx.store.list_rename_items(session_id, None)?;
    let wanted: Option<std::collections::HashSet<String>> = symbol_ids.map(|v| v.into_iter().collect());
    let todo: std::collections::HashSet<String> = stored
        .iter()
        .filter(|r| match &wanted {
            Some(w) => w.contains(&r.symbol_id),
            None => r.status == "candidate",
        })
        .map(|r| r.symbol_id.clone())
        .collect();
    let cands: Vec<_> = detect(&index, &RuleOptions::default())
        .into_iter()
        .filter(|c| todo.contains(&c.symbol.0.to_string()))
        .collect();
    if cands.is_empty() {
        return Ok(0);
    }
    let mut opts = ProposeOptions { try_again, ..ProposeOptions::default() };
    for r in stored.iter().filter(|r| todo.contains(&r.symbol_id)) {
        if let (Some(p), Ok(n)) = (&r.proposed, r.symbol_id.parse::<u32>()) {
            opts.rejected.entry(SymbolId(n)).or_default().push(p.clone());
        }
    }
    ctx.store.set_rename_session_status(session_id, "proposing", None)?;
    let cancel = CancellationToken::new();
    ctx.renames.add_job(job_id, cancel.clone());
    let gate = gatekeeper(ctx, "rename");
    let llm = SchedulerJson(Arc::new(ctx.scheduler.clone()));
    let status = |_: String| {};
    let cx = PipelineCx { llm: &llm, gate: &gate, cancel: &cancel, status: &status, home: Some(home_dir()) };
    let total = cands.len() as u32;
    let result = propose(&cx, &index, &cands, &opts, &|done, total| {
        progress(RenameProgress { phase: "proposing".into(), done: done as u32, total: total as u32 });
    })
    .await;
    ctx.renames.end_job(job_id);
    let result = match result {
        Ok(r) => r,
        Err(e) => {
            let _ = ctx.store.set_rename_session_status(session_id, "ready", None);
            return Err(pipeline_err(e));
        }
    };
    let mut made = 0u32;
    for p in &result.proposals {
        let key = p.symbol.0.to_string();
        let patch = match p.status {
            ProposalStatus::Proposed => {
                made += 1;
                RenameItemPatch {
                    proposed: Some(p.new_name.clone()),
                    final_name: Some(p.new_name.clone()),
                    confidence: Some(f64::from(p.confidence)),
                    reason: Some(p.reason.clone()),
                    status: Some("proposed".into()),
                    ..RenameItemPatch::default()
                }
            }
            ProposalStatus::Unchanged => RenameItemPatch {
                reason: Some("already clear".into()),
                status: Some("candidate".into()),
                ..RenameItemPatch::default()
            },
            ProposalStatus::NeedsManual => RenameItemPatch {
                reason: Some(p.reason.clone()),
                status: Some("needs_manual".into()),
                ..RenameItemPatch::default()
            },
        };
        ctx.store.update_rename_item(session_id, &key, &patch)?;
    }
    ctx.store.set_rename_session_status(session_id, "ready", None)?;
    progress(RenameProgress { phase: "done".into(), done: total, total });
    let _ = ctx.bus.send(AppEvent::Notify(NewNotification {
        level: NotificationLevel::Success,
        kind: "job".into(),
        title: format!("{made} rename proposals ready"),
        body: Some(format!("Project: {}", s.root)),
        action: Some(NotificationAction { label: "Review".into(), route: format!("/rename?session={session_id}") }),
    }));
    Ok(made)
}

/// Save the user's decision on one item. A new name is validated against the project (keywords,
/// builtins, convention, collisions with the other accepted names); a refused name leaves the item
/// unchanged and comes back in `error`.
pub fn item_update(
    ctx: &AppCtx,
    session_id: &str,
    symbol_id: &str,
    final_name: Option<String>,
    status: Option<String>,
    include_strings: Option<bool>,
) -> Result<RenameItemView, NexusError> {
    let s = session_row(ctx, session_id)?;
    let index = load_index(ctx, &s)?;
    if let Some(st) = &status
        && !STATUSES.contains(&st.as_str())
    {
        return Err(NexusError::InvalidInput(format!("unknown status `{st}`")));
    }
    let id = SymbolId(symbol_id.parse().map_err(|_| NexusError::InvalidInput("bad symbol id".into()))?);
    let mut patch = RenameItemPatch { status, include_strings, ..RenameItemPatch::default() };
    let mut error = None;
    if let Some(name) = final_name {
        let name = name.trim().to_owned();
        let taken: HashMap<SymbolId, String> = ctx
            .store
            .list_rename_items(session_id, None)?
            .into_iter()
            .filter(|r| r.symbol_id != symbol_id && matches!(r.status.as_str(), "accepted" | "proposed"))
            .filter_map(|r| Some((SymbolId(r.symbol_id.parse().ok()?), r.final_name.or(r.proposed)?)))
            .collect();
        match Validator::new(&index).check(id, &name, &taken) {
            Ok(()) => {
                patch.final_name = Some(Some(name));
                if patch.status.is_none() {
                    patch.status = Some("accepted".into());
                }
            }
            Err(why) => {
                error = Some(why);
                patch.status = None;
            }
        }
    }
    let row = ctx.store.update_rename_item(session_id, symbol_id, &patch)?;
    Ok(view_of(&index, row, error))
}

/// Plan the accepted renames as a change set (nothing is written until the user applies it).
pub async fn plan(
    ctx: &Arc<AppCtx>,
    session_id: &str,
    opts: RenamePlanOptions,
) -> Result<RenamePlanView, NexusError> {
    let s = session_row(ctx, session_id)?;
    let index = load_index(ctx, &s)?;
    let gate = gatekeeper(ctx, "user");
    gate.authorize(Path::new(&s.root), Access::Write, "to plan these renames")
        .await
        .map_err(tool_err)?;
    let items: Vec<RenameItem> = ctx
        .store
        .list_rename_items(session_id, Some("accepted"))?
        .into_iter()
        .filter_map(|r| {
            Some(RenameItem { symbol: SymbolId(r.symbol_id.parse().ok()?), new_name: r.final_name.or(r.proposed)? })
        })
        .collect();
    if items.is_empty() {
        return Err(NexusError::InvalidInput("Accept at least one rename first.".into()));
    }
    let popts = PlanOptions { docs: opts.docs, comments: opts.comments, ambiguous: opts.ambiguous, public_api: opts.public_api };
    let planned = tokio::task::spawn_blocking(move || plan_renames(&index, &items, &popts, now_ms()))
        .await
        .map_err(|e| NexusError::Internal(e.to_string()))?
        .map_err(|e| match e {
            nexus_analyze::code::plan::PlanError::Stale(_) => NexusError::Conflict(e.to_string()),
            other => NexusError::InvalidInput(other.to_string()),
        })?;
    let id = super::fs::changeset_plan(ctx, &planned.change_set.title, "rename", planned.change_set.ops)?;
    ctx.store.set_rename_session_status(session_id, "planned", None)?;
    Ok(RenamePlanView {
        changeset_id: id.0,
        edits: planned.edit_count,
        skipped: planned.skipped.into_iter().map(|s| s.reason).collect(),
        dirty: planned.dirty,
    })
}

/// The project's own checkers (cargo check, tsc, …) and whether each is installed.
pub fn verify_checkers(ctx: &AppCtx, session_id: &str) -> Result<Vec<Checker>, NexusError> {
    let s = session_row(ctx, session_id)?;
    Ok(nexus_analyze::code::verify::detect(&PathBuf::from(&s.root)))
}

/// Run one checker (needs Write access to the project: builds create files there). Output lines stream
/// through `on_line`. Never automatic.
pub async fn verify(
    ctx: &Arc<AppCtx>,
    job_id: &str,
    session_id: &str,
    checker_id: &str,
    on_line: Arc<dyn Fn(String) + Send + Sync>,
) -> Result<VerifyResult, NexusError> {
    let s = session_row(ctx, session_id)?;
    let root = PathBuf::from(&s.root);
    let checker = nexus_analyze::code::verify::detect(&root)
        .into_iter()
        .find(|c| c.id == checker_id)
        .ok_or_else(|| NexusError::NotFound(format!("checker {checker_id}")))?;
    let gate = gatekeeper(ctx, "user");
    gate.authorize(&root, Access::Write, "to run the project's checker")
        .await
        .map_err(tool_err)?;
    let cancel = CancellationToken::new();
    ctx.renames.add_job(job_id, cancel.clone());
    let result = nexus_analyze::code::verify::run(&root, &checker, &cancel, |l| on_line(l.to_owned())).await;
    ctx.renames.end_job(job_id);
    Ok(result)
}

/// Where a symbol and its usages are (for the occurrence drawer).
pub fn usages(ctx: &AppCtx, session_id: &str, symbol_id: &str) -> Result<Vec<nexus_analyze::code::find::Usage>, NexusError> {
    let s = session_row(ctx, session_id)?;
    let index = load_index(ctx, &s)?;
    let id = SymbolId(symbol_id.parse().map_err(|_| NexusError::InvalidInput("bad symbol id".into()))?);
    Ok(nexus_analyze::code::find::usages(&index, id))
}

/// Search symbols, files and folders of the indexed project.
pub fn find(ctx: &AppCtx, session_id: &str, query: &str) -> Result<Vec<nexus_analyze::code::find::Found>, NexusError> {
    let s = session_row(ctx, session_id)?;
    let index = load_index(ctx, &s)?;
    Ok(nexus_analyze::code::find::find_symbols(&index, query, 50))
}

/// Stop a running index, proposal, or verify job.
pub fn cancel(ctx: &AppCtx, job_id: &str) -> bool {
    ctx.renames.cancel(job_id)
}
