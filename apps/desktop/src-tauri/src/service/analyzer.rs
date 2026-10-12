//! The analyzer service: run a folder analysis with progress and cancel, keep the report index,
//! export reports, and turn a report into a cleanup plan (ANALYZER.md). Nothing here writes user files;
//! a cleanup plan is only *planned* (a change set the user reviews on the Changes page).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nexus_agent::tools::Gatekeeper;
use nexus_analyze::file_report::{FileReport, file_report};
use nexus_analyze::scan::{ScanOptions, display_root, scan_folder};
use nexus_analyze::{DupMode, FolderReport, ScanProgress, render};
use nexus_core::changes::FileOp;
use nexus_core::error::NexusError;
use nexus_core::fs::Access;
use nexus_core::ids::ChangeSetId;
use nexus_store::ReportRow;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

use super::pipelines::{gatekeeper, home_dir, save_report, tool_err};
use crate::state::AppCtx;

/// Folder analyses in flight, by job id.
#[derive(Default)]
pub struct Analyses(Mutex<HashMap<String, CancellationToken>>);

impl Analyses {
    fn add(&self, id: &str, t: CancellationToken) {
        if let Ok(mut m) = self.0.lock() {
            m.insert(id.to_owned(), t);
        }
    }
    fn remove(&self, id: &str) {
        if let Ok(mut m) = self.0.lock() {
            m.remove(id);
        }
    }
    /// Cancel a running analysis. `false` when there is none with that id.
    pub fn cancel(&self, id: &str) -> bool {
        match self.0.lock().ok().and_then(|m| m.get(id).cloned()) {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }
}

/// One line of the saved-reports list.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct ReportSummary {
    pub id: String,
    /// `folder | file | rename | secrets | logs | data`
    pub kind: String,
    pub target: String,
    pub title: String,
    pub summary: Option<String>,
    pub has_narrative: bool,
    pub created_at: f64,
}

impl From<ReportRow> for ReportSummary {
    fn from(r: ReportRow) -> Self {
        Self {
            id: r.id,
            kind: r.kind,
            target: r.target,
            title: r.title,
            summary: r.summary,
            has_narrative: r.ai_narrative.is_some(),
            created_at: r.created_at as f64,
        }
    }
}

/// A folder report with its AI narrative (if one was saved).
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct FolderReportView {
    pub report: FolderReport,
    pub shown_root: String,
    pub narrative: Option<String>,
}

/// `markdown | json | csv_largest | csv_duplicates | csv_junk`
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Markdown,
    Json,
    CsvLargest,
    CsvDuplicates,
    CsvJunk,
}

/// A ready-to-save export: the webview downloads it (the app never writes arbitrary paths).
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct ExportFile {
    pub file_name: String,
    pub mime: String,
    pub contents: String,
}

/// What a cleanup plan would do.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct CleanupPlan {
    pub changeset_id: String,
    pub files: u32,
    pub bytes: f64,
}

fn row_of(ctx: &AppCtx, id: &str) -> Result<ReportRow, NexusError> {
    ctx.store
        .get_report(id)?
        .ok_or_else(|| NexusError::NotFound(format!("report {id}")))
}

fn read_json<T: serde::de::DeserializeOwned>(row: &ReportRow) -> Result<T, NexusError> {
    let text = std::fs::read_to_string(&row.json_path)?;
    serde_json::from_str(&text).map_err(|e| NexusError::Internal(format!("the saved report is damaged: {e}")))
}

fn load_folder(ctx: &AppCtx, id: &str) -> Result<(ReportRow, FolderReport), NexusError> {
    let row = row_of(ctx, id)?;
    if row.kind != "folder" {
        return Err(NexusError::InvalidInput("that is not a folder report".into()));
    }
    let mut r: FolderReport = read_json(&row)?;
    r.id = row.id.clone();
    Ok((row, r))
}

/// Scan a folder and save the report. Asks for access when the folder is not granted yet.
/// Returns the report id; the job id is the caller's handle for [`cancel`].
pub async fn analyze_folder(
    ctx: &Arc<AppCtx>,
    job_id: &str,
    root: &str,
    duplicates: DupMode,
    progress: Arc<dyn Fn(ScanProgress) + Send + Sync>,
) -> Result<String, NexusError> {
    let shown = display_root(Path::new(root), Some(&home_dir()));
    let job = ctx.jobs.start("scan", &format!("Scan {shown}"))?;
    // The Jobs panel and `analyze_cancel` stop the scan through the same token.
    ctx.analyses.add(job_id, job.token.clone());
    let jobs_progress = {
        let (ctx, id) = (Arc::clone(ctx), job.id.clone());
        move |p: &ScanProgress| {
            ctx.jobs.progress(
                &id,
                None,
                Some(format!(
                    "{} files · {}",
                    nexus_analyze::fmt::count(p.files_seen),
                    p.phase
                )),
            )
        }
    };
    let progress: Arc<dyn Fn(ScanProgress) + Send + Sync> = Arc::new(move |p| {
        jobs_progress(&p);
        progress(p);
    });
    let result = scan_and_save(ctx, &job.token, root, duplicates, progress).await;
    ctx.analyses.remove(job_id);
    super::jobs::end(ctx, &job.id, &result, |id| Some(format!("/analyzer?report={id}")));
    result
}

async fn scan_and_save(
    ctx: &Arc<AppCtx>,
    cancel: &CancellationToken,
    root: &str,
    duplicates: DupMode,
    progress: Arc<dyn Fn(ScanProgress) + Send + Sync>,
) -> Result<String, NexusError> {
    let gate = gatekeeper(ctx, "user");
    let canon = gate
        .authorize(Path::new(root), Access::Read, "to analyze this folder")
        .await
        .map_err(tool_err)?;
    if !canon.is_dir() {
        return Err(NexusError::InvalidInput(format!(
            "{} is not a folder",
            canon.display()
        )));
    }
    let mut opts = ScanOptions::new(&canon);
    opts.duplicates = duplicates;
    let scan_cancel = cancel.clone();
    let result = tokio::task::spawn_blocking(move || scan_folder(&opts, &*progress, &scan_cancel)).await;
    let report = match result.map_err(|e| NexusError::Internal(e.to_string()))? {
        Ok(r) => r,
        Err(nexus_analyze::ScanError::Cancelled) => return Err(NexusError::Cancelled),
        Err(e) => return Err(NexusError::InvalidInput(e.to_string())),
    };
    let shown = display_root(&canon, Some(&home_dir()));
    let json = render::json(&report).map_err(|e| NexusError::Internal(e.to_string()))?;
    let summary = format!(
        "{} files · {}",
        nexus_analyze::fmt::count(report.totals.files),
        nexus_analyze::fmt::bytes(report.totals.bytes)
    );
    save_report(
        ctx,
        "folder",
        &canon.display().to_string(),
        &format!("Folder report: {shown}"),
        Some(summary),
        &json,
    )
}

/// Stop a running analysis.
pub fn cancel(ctx: &AppCtx, job_id: &str) -> bool {
    ctx.analyses.cancel(job_id)
}

pub fn reports_list(ctx: &AppCtx, kind: Option<&str>, limit: u32) -> Result<Vec<ReportSummary>, NexusError> {
    Ok(ctx
        .store
        .list_reports(kind, limit.clamp(1, 500))?
        .into_iter()
        .map(Into::into)
        .collect())
}

pub fn report_folder(ctx: &AppCtx, id: &str) -> Result<FolderReportView, NexusError> {
    let (row, report) = load_folder(ctx, id)?;
    let shown_root = display_root(Path::new(&row.target), Some(&home_dir()));
    Ok(FolderReportView {
        report,
        shown_root,
        narrative: row.ai_narrative,
    })
}

/// The stored body of a non-folder report (file, secrets, logs) as JSON text for the viewer.
pub fn report_body(ctx: &AppCtx, id: &str) -> Result<String, NexusError> {
    Ok(std::fs::read_to_string(row_of(ctx, id)?.json_path)?)
}

pub fn report_delete(ctx: &AppCtx, id: &str) -> Result<bool, NexusError> {
    let Some(row) = ctx.store.get_report(id)? else {
        return Ok(false);
    };
    // Only ever remove a file inside our own reports folder.
    let p = PathBuf::from(&row.json_path);
    if p.starts_with(&ctx.paths.reports) {
        let _ = std::fs::remove_file(p);
    }
    Ok(ctx.store.delete_report(id)?)
}

/// Save the narrative the model wrote for a report (shown above the numbers next time).
pub fn report_narrative_save(ctx: &AppCtx, id: &str, text: &str) -> Result<(), NexusError> {
    ctx.store.set_report_narrative(id, text.trim())?;
    Ok(())
}

pub fn report_export(ctx: &AppCtx, id: &str, format: ExportFormat) -> Result<ExportFile, NexusError> {
    let (row, r) = load_folder(ctx, id)?;
    let shown = display_root(Path::new(&row.target), Some(&home_dir()));
    let stem = format!("report-{}", row.id.chars().take(8).collect::<String>());
    let (suffix, mime, contents) = match format {
        ExportFormat::Markdown => (
            "md",
            "text/markdown",
            render::markdown(&r, &shown, row.ai_narrative.as_deref()),
        ),
        ExportFormat::Json => (
            "json",
            "application/json",
            render::json(&r).map_err(|e| NexusError::Internal(e.to_string()))?,
        ),
        ExportFormat::CsvLargest => ("largest.csv", "text/csv", render::csv_largest(&r)),
        ExportFormat::CsvDuplicates => ("duplicates.csv", "text/csv", render::csv_duplicates(&r)),
        ExportFormat::CsvJunk => ("junk.csv", "text/csv", render::csv_junk(&r)),
    };
    Ok(ExportFile {
        file_name: format!("{stem}.{suffix}"),
        mime: mime.into(),
        contents,
    })
}

/// A report for one file (Read access required; asks when missing).
pub async fn file_report_for(ctx: &Arc<AppCtx>, path: &str) -> Result<String, NexusError> {
    let gate = gatekeeper(ctx, "user");
    let canon = gate
        .authorize(Path::new(path), Access::Read, "to look at this file")
        .await
        .map_err(tool_err)?;
    let report: FileReport = tokio::task::spawn_blocking(move || file_report(&canon))
        .await
        .map_err(|e| NexusError::Internal(e.to_string()))?
        .map_err(|e| NexusError::InvalidInput(e.to_string()))?;
    let json = serde_json::to_string(&report).map_err(|e| NexusError::Internal(e.to_string()))?;
    save_report(
        ctx,
        "file",
        path,
        &format!("File report: {}", report.name),
        None,
        &json,
    )
}

fn modified_ms(p: &Path) -> i64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(i64::MAX, |d| d.as_millis() as i64)
}

/// Plan a cleanup from a folder report: safe junk and extra copies of duplicates (the oldest copy of each
/// group is kept) go to the Trash. Needs Write access to the folder; nothing happens until the plan is applied.
pub async fn cleanup_plan(
    ctx: &Arc<AppCtx>,
    id: &str,
    include_duplicates: bool,
    include_junk: bool,
) -> Result<CleanupPlan, NexusError> {
    let (row, r) = load_folder(ctx, id)?;
    let gate = gatekeeper(ctx, "user");
    let root = gate
        .authorize(Path::new(&row.target), Access::Write, "to plan a cleanup")
        .await
        .map_err(tool_err)?;
    let mut ops: Vec<FileOp> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let (mut files, mut bytes) = (0u32, 0u64);
    let mut add = |rel: &str, size: u64, ops: &mut Vec<FileOp>| {
        let abs = root.join(rel);
        if seen.insert(abs.clone()) {
            ops.push(FileOp::Trash {
                path: abs.display().to_string(),
            });
            files += 1;
            bytes += size;
        }
    };
    if include_junk {
        for j in r.junk.iter().filter(|j| j.safe_to_trash && !j.regenerable) {
            add(&j.rel_path, j.bytes, &mut ops);
        }
    }
    if include_duplicates {
        for g in r.duplicates.iter().filter(|g| g.confirmed) {
            let keep = g
                .paths
                .iter()
                .min_by_key(|p| (modified_ms(&root.join(p)), p.len()))
                .cloned();
            for p in g.paths.iter().filter(|p| Some(*p) != keep.as_ref()) {
                add(p, g.bytes_each, &mut ops);
            }
        }
    }
    if ops.is_empty() {
        return Err(NexusError::InvalidInput(
            "Nothing to clean up in this report.".into(),
        ));
    }
    let cs: ChangeSetId = super::fs::changeset_plan(
        ctx,
        &format!("Clean up {}", display_root(&root, Some(&home_dir()))),
        "analyzer",
        ops,
    )?;
    Ok(CleanupPlan {
        changeset_id: cs.0,
        files,
        bytes: bytes as f64,
    })
}
