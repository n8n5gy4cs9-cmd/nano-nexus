//! Folder watches (ANALYZER §4, M11): re-scan a folder on a schedule or after it changed, compare with
//! the previous report, and notify when the change is big enough. The scan itself is an ordinary
//! `scan` job, so it shows up in the Jobs panel and can be cancelled there.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nexus_analyze::watch::{Debounce, FolderWatcher, ReportDelta, diff_reports, is_due, is_significant, notification_text, watch_folder};
use nexus_analyze::{DupMode, FolderReport};
use nexus_core::error::NexusError;
use nexus_core::events::{NotificationAction, NotificationLevel};
use nexus_core::fs::Access;
use nexus_core::ids::MessageId;
use nexus_store::{NewNotification, WatchRow};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::pipelines::home_dir;
use crate::state::{AppCtx, AppEvent};

/// A change watcher with its debounce state.
struct Active {
    debounce: Arc<Mutex<Debounce>>,
    _watcher: FolderWatcher,
}

/// Live parts of the watches: file watchers for `on_change` and the set of runs in flight.
#[derive(Default)]
pub struct WatchRuntime {
    active: Mutex<HashMap<String, Active>>,
    running: Mutex<HashSet<String>>,
}

/// One watch as the UI shows it.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct WatchView {
    pub id: String,
    pub root: String,
    /// `daily | weekly | on_change`
    pub schedule: String,
    pub enabled: bool,
    pub last_report_id: Option<String>,
    /// Unix milliseconds of the last run.
    pub last_run_at: Option<f64>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn last_run_ms(ctx: &AppCtx, w: &WatchRow) -> Option<i64> {
    let id = w.last_report_id.as_deref()?;
    ctx.store.get_report(id).ok().flatten().map(|r| r.created_at)
}

fn view(ctx: &AppCtx, w: WatchRow) -> WatchView {
    let last = last_run_ms(ctx, &w);
    WatchView { id: w.id, root: w.root, schedule: w.schedule, enabled: w.enabled, last_report_id: w.last_report_id, last_run_at: last.map(|t| t as f64) }
}

pub fn list(ctx: &AppCtx) -> Result<Vec<WatchView>, NexusError> {
    Ok(ctx.store.list_watches()?.into_iter().map(|w| view(ctx, w)).collect())
}

fn check_schedule(schedule: &str) -> Result<(), NexusError> {
    match schedule {
        "daily" | "weekly" | "on_change" => Ok(()),
        other => Err(NexusError::InvalidInput(format!("unknown schedule: {other}"))),
    }
}

/// Watch `root` (it must be a folder the app may read). The first run saves the baseline report.
pub fn create(ctx: &Arc<AppCtx>, root: &str, schedule: &str) -> Result<WatchView, NexusError> {
    check_schedule(schedule)?;
    let path = Path::new(root);
    if !path.is_dir() {
        return Err(NexusError::InvalidInput(format!("{root} is not a folder")));
    }
    if path.parent().is_none() || path == home_dir() {
        return Err(NexusError::InvalidInput("Pick a subfolder: watching the whole home or root folder is too noisy.".into()));
    }
    ctx.perms.check("watch", path, Access::Read).map_err(|e| NexusError::PermissionDenied(e.to_string()))?;
    let row = ctx.store.insert_watch(&MessageId::new().0, root, schedule)?;
    sync_watchers(ctx);
    Ok(view(ctx, row))
}

pub fn set_enabled(ctx: &Arc<AppCtx>, id: &str, enabled: bool) -> Result<(), NexusError> {
    ctx.store.set_watch_enabled(id, enabled)?;
    sync_watchers(ctx);
    Ok(())
}

pub fn set_schedule(ctx: &Arc<AppCtx>, id: &str, schedule: &str) -> Result<(), NexusError> {
    check_schedule(schedule)?;
    ctx.store.set_watch_schedule(id, schedule)?;
    sync_watchers(ctx);
    Ok(())
}

pub fn delete(ctx: &Arc<AppCtx>, id: &str) -> Result<bool, NexusError> {
    let removed = ctx.store.delete_watch(id)?;
    sync_watchers(ctx);
    Ok(removed)
}

/// Start file watchers for enabled `on_change` watches and stop the ones that no longer apply.
pub fn sync_watchers(ctx: &Arc<AppCtx>) {
    let Ok(watches) = ctx.store.list_watches() else { return };
    let wanted: HashMap<String, WatchRow> =
        watches.into_iter().filter(|w| w.enabled && w.schedule == "on_change").map(|w| (w.id.clone(), w)).collect();
    let Ok(mut active) = ctx.watches.active.lock() else { return };
    active.retain(|id, _| wanted.contains_key(id));
    let home = home_dir();
    for (id, w) in wanted {
        if active.contains_key(&id) {
            continue;
        }
        let debounce = Arc::new(Mutex::new(Debounce::default()));
        let sink = debounce.clone();
        let started = watch_folder(Path::new(&w.root), Some(&home), move || {
            if let Ok(mut d) = sink.lock() {
                d.event(now_ms());
            }
        });
        match started {
            Ok(watcher) => {
                active.insert(id, Active { debounce, _watcher: watcher });
            }
            Err(e) => tracing::warn!(root = %w.root, "could not watch folder: {e}"),
        }
    }
}

fn load_folder_report(ctx: &AppCtx, id: &str) -> Option<FolderReport> {
    let row = ctx.store.get_report(id).ok().flatten()?;
    let text = std::fs::read_to_string(row.json_path).ok()?;
    serde_json::from_str(&text).ok()
}

fn notify(ctx: &AppCtx, level: NotificationLevel, title: String, body: Option<String>, report_id: Option<&str>) {
    let action = report_id.map(|id| NotificationAction { label: "Open report".into(), route: format!("/analyzer?report={id}") });
    let _ = ctx.bus.send(AppEvent::Notify(NewNotification { level, kind: "watch".into(), title, body, action }));
}

/// Scan the watched folder now and compare with the previous report. `manual` (Run now) always tells
/// the user what happened; scheduled runs only notify when a threshold is crossed.
pub async fn run(ctx: &Arc<AppCtx>, id: &str, manual: bool) -> Result<Option<ReportDelta>, NexusError> {
    let w = ctx.store.get_watch(id)?.ok_or_else(|| NexusError::NotFound("watch".into()))?;
    {
        let mut running = ctx.watches.running.lock().map_err(|_| NexusError::Internal("watch lock".into()))?;
        if !running.insert(id.to_owned()) {
            return Err(NexusError::InvalidInput("This watch is already running.".into()));
        }
    }
    let result = run_inner(ctx, &w, manual).await;
    if let Ok(mut running) = ctx.watches.running.lock() {
        running.remove(id);
    }
    result
}

async fn run_inner(ctx: &Arc<AppCtx>, w: &WatchRow, manual: bool) -> Result<Option<ReportDelta>, NexusError> {
    // A background run never asks for access: without a grant the user is told once and the run is skipped.
    if ctx.perms.check("watch", Path::new(&w.root), Access::Read).is_err() {
        notify(ctx, NotificationLevel::Warning, format!("Watch needs access to {}", w.root), Some("Open the Analyzer and allow the folder again.".into()), None);
        return Err(NexusError::PermissionDenied(w.root.clone()));
    }
    let job_id = MessageId::new().0;
    let new_id = super::analyzer::analyze_folder(ctx, &job_id, &w.root, DupMode::Fast, Arc::new(|_| {})).await?;
    let prev = w.last_report_id.as_deref().and_then(|id| load_folder_report(ctx, id));
    ctx.store.set_watch_last_report(&w.id, &new_id)?;
    let (Some(prev), Some(new)) = (prev, load_folder_report(ctx, &new_id)) else {
        if manual {
            notify(ctx, NotificationLevel::Info, "Watch baseline saved".into(), Some(format!("{} will be compared with this scan next time.", w.root)), Some(&new_id));
        }
        return Ok(None);
    };
    let delta = diff_reports(&prev, &new);
    if is_significant(&delta) {
        let (title, body) = notification_text(&delta);
        notify(ctx, NotificationLevel::Info, title, Some(body), Some(&new_id));
    } else if manual {
        notify(ctx, NotificationLevel::Info, "No big changes".into(), Some(format!("{} is about the same as last time.", w.root)), Some(&new_id));
    }
    Ok(Some(delta))
}

/// One scheduler pass: start every watch that is due. Runs execute in the background.
pub fn tick(ctx: &Arc<AppCtx>) {
    let Ok(watches) = ctx.store.list_watches() else { return };
    let now = now_ms();
    for w in watches.into_iter().filter(|w| w.enabled) {
        let due = if w.schedule == "on_change" {
            ctx.watches
                .active
                .lock()
                .ok()
                .and_then(|a| a.get(&w.id).map(|x| x.debounce.clone()))
                .and_then(|d| d.lock().ok().map(|mut d| d.take_if_quiet(now)))
                .unwrap_or(false)
        } else {
            is_due(&w.schedule, last_run_ms(ctx, &w), now)
        };
        if due {
            let (ctx, id) = (Arc::clone(ctx), w.id.clone());
            tokio::spawn(async move {
                if let Err(e) = run(&ctx, &id, false).await {
                    tracing::warn!(watch = %id, "watch run failed: {e}");
                }
            });
        }
    }
}

/// Check once a minute while the app runs. Spawn this on the async runtime (it never returns); calling
/// `tokio::spawn` from the Tauri setup hook panics because no runtime is entered there.
pub async fn run_scheduler(ctx: Arc<AppCtx>) {
    sync_watchers(&ctx);
    let mut every = tokio::time::interval(Duration::from_secs(60));
    loop {
        every.tick().await;
        sync_watchers(&ctx);
        tick(&ctx);
    }
}
