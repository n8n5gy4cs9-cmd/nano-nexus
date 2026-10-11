//! Background jobs: persistence in the `jobs` table plus the [`JobRegistry`] that the app's long
//! operations report to (M11). `job:update` events are throttled to about 4 Hz per job; a job that was
//! `running` or `queued` when the app died is marked failed at startup.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nexus_core::events::{JobStatus, JobSummary};
use nexus_core::ids::JobId;
use rusqlite::{OptionalExtension, Row, params};
use tokio_util::sync::CancellationToken;

use crate::{Store, StoreError, now_ms};

/// Finished jobs older than this many are pruned on startup.
const KEEP_FINISHED: u32 = 200;

fn status_str(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Queued => "queued",
        JobStatus::Running => "running",
        JobStatus::Done => "done",
        JobStatus::Failed => "failed",
        JobStatus::Cancelled => "cancelled",
    }
}

fn parse_status(s: &str) -> JobStatus {
    match s {
        "queued" => JobStatus::Queued,
        "running" => JobStatus::Running,
        "done" => JobStatus::Done,
        "cancelled" => JobStatus::Cancelled,
        _ => JobStatus::Failed,
    }
}

fn row(r: &Row<'_>) -> rusqlite::Result<JobSummary> {
    Ok(JobSummary {
        id: JobId(r.get("id")?),
        kind: r.get("kind")?,
        title: r.get("title")?,
        status: parse_status(&r.get::<_, String>("status")?),
        progress: r.get::<_, Option<f64>>("progress")?.map(|p| p as f32),
        detail: r.get("detail")?,
        result_ref: r.get("result_ref")?,
        error: r.get("error")?,
    })
}

impl Store {
    pub fn insert_job(&self, j: &JobSummary) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO jobs (id, kind, title, status, progress, detail, result_ref, error, created_at, finished_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL)",
                params![
                    j.id.as_str(), j.kind, j.title, status_str(j.status), j.progress.map(f64::from), j.detail,
                    j.result_ref, j.error, now_ms()
                ],
            )?;
            Ok(())
        })
    }

    /// Save the mutable fields; `finished_at` is set once the status is terminal.
    pub fn update_job(&self, j: &JobSummary) -> Result<(), StoreError> {
        let terminal = matches!(
            j.status,
            JobStatus::Done | JobStatus::Failed | JobStatus::Cancelled
        );
        self.with_conn(|c| {
            let n = c.execute(
                "UPDATE jobs SET status = ?2, progress = ?3, detail = ?4, result_ref = ?5, error = ?6,
                 finished_at = CASE WHEN ?7 THEN COALESCE(finished_at, ?8) ELSE finished_at END WHERE id = ?1",
                params![
                    j.id.as_str(), status_str(j.status), j.progress.map(f64::from), j.detail, j.result_ref,
                    j.error, terminal, now_ms()
                ],
            )?;
            if n == 0 { Err(StoreError::NotFound("job")) } else { Ok(()) }
        })
    }

    pub fn get_job(&self, id: &str) -> Result<Option<JobSummary>, StoreError> {
        self.with_conn(|c| {
            Ok(c.query_row("SELECT * FROM jobs WHERE id = ?1", params![id], row)
                .optional()?)
        })
    }

    /// Newest first.
    pub fn list_jobs(&self, limit: u32) -> Result<Vec<JobSummary>, StoreError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT * FROM jobs ORDER BY created_at DESC, id DESC LIMIT ?1")?;
            Ok(stmt
                .query_map(params![limit], row)?
                .collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Mark jobs that were queued or running when the app stopped as failed. Returns how many.
    pub fn recover_interrupted_jobs(&self) -> Result<usize, StoreError> {
        self.with_conn(|c| {
            Ok(c.execute(
                "UPDATE jobs SET status = 'failed', error = 'Interrupted by a restart.', finished_at = ?1
                 WHERE status IN ('queued','running')",
                params![now_ms()],
            )?)
        })
    }

    /// Keep the newest `keep` finished jobs and drop the rest. Returns how many were removed.
    pub fn prune_jobs(&self, keep: u32) -> Result<usize, StoreError> {
        self.with_conn(|c| {
            Ok(c.execute(
                "DELETE FROM jobs WHERE status IN ('done','failed','cancelled') AND id NOT IN
                 (SELECT id FROM jobs WHERE status IN ('done','failed','cancelled') ORDER BY created_at DESC, id DESC LIMIT ?1)",
                params![keep],
            )?)
        })
    }
}

struct Live {
    token: CancellationToken,
    summary: JobSummary,
    last_emit: Option<Instant>,
}

/// What a worker gets when it starts a job.
#[derive(Clone)]
pub struct JobHandle {
    pub id: JobId,
    /// Cancelled when the user cancels the job: workers should stop soon after.
    pub token: CancellationToken,
}

/// Tracks running jobs, persists them, and emits throttled updates.
pub struct JobRegistry {
    store: Arc<Store>,
    emit: Arc<dyn Fn(JobSummary) + Send + Sync>,
    live: Mutex<HashMap<String, Live>>,
    min_interval: Duration,
}

impl JobRegistry {
    /// `emit` is called with every update (the app forwards it as the `job:update` event).
    pub fn new(store: Arc<Store>, emit: Arc<dyn Fn(JobSummary) + Send + Sync>) -> Self {
        Self::with_interval(store, emit, Duration::from_millis(250))
    }

    pub fn with_interval(
        store: Arc<Store>,
        emit: Arc<dyn Fn(JobSummary) + Send + Sync>,
        min_interval: Duration,
    ) -> Self {
        Self {
            store,
            emit,
            live: Mutex::new(HashMap::new()),
            min_interval,
        }
    }

    /// Call once at startup: fail jobs left over from a crash and prune old ones.
    pub fn recover(&self) -> Result<usize, StoreError> {
        let n = self.store.recover_interrupted_jobs()?;
        self.store.prune_jobs(KEEP_FINISHED)?;
        Ok(n)
    }

    /// Start a running job.
    pub fn start(&self, kind: &str, title: &str) -> Result<JobHandle, StoreError> {
        let summary = JobSummary {
            id: JobId::new(),
            kind: kind.to_owned(),
            title: title.to_owned(),
            status: JobStatus::Running,
            progress: None,
            detail: None,
            result_ref: None,
            error: None,
        };
        self.store.insert_job(&summary)?;
        let token = CancellationToken::new();
        let handle = JobHandle {
            id: summary.id.clone(),
            token: token.clone(),
        };
        (self.emit)(summary.clone());
        if let Ok(mut m) = self.live.lock() {
            m.insert(
                summary.id.0.clone(),
                Live {
                    token,
                    summary,
                    last_emit: Some(Instant::now()),
                },
            );
        }
        Ok(handle)
    }

    /// Report progress (0..=1) and an optional detail line. Persisted and emitted at most every
    /// `min_interval`; the latest values are always the ones kept in memory.
    pub fn progress(&self, id: &JobId, progress: Option<f32>, detail: Option<String>) {
        let due = {
            let Ok(mut m) = self.live.lock() else { return };
            let Some(l) = m.get_mut(id.as_str()) else { return };
            if l.summary.status != JobStatus::Running {
                return;
            }
            l.summary.progress = progress.map(|p| p.clamp(0.0, 1.0));
            l.summary.detail = detail;
            let due = l.last_emit.is_none_or(|t| t.elapsed() >= self.min_interval);
            if due {
                l.last_emit = Some(Instant::now());
            }
            due.then(|| l.summary.clone())
        };
        if let Some(s) = due {
            if let Err(e) = self.store.update_job(&s) {
                tracing::warn!("could not save job progress: {e}");
            }
            (self.emit)(s);
        }
    }

    fn end(&self, id: &JobId, status: JobStatus, result_ref: Option<String>, error: Option<String>) -> bool {
        let summary = {
            let Ok(mut m) = self.live.lock() else { return false };
            let Some(mut l) = m.remove(id.as_str()) else {
                return false;
            };
            l.summary.status = status;
            l.summary.result_ref = result_ref;
            l.summary.error = error;
            if status == JobStatus::Done {
                l.summary.progress = Some(1.0);
            }
            l.summary
        };
        if let Err(e) = self.store.update_job(&summary) {
            tracing::warn!("could not save a finished job: {e}");
        }
        (self.emit)(summary);
        true
    }

    /// The job finished; `result_ref` says where to open the result (`/analyzer?report=…`).
    pub fn finish(&self, id: &JobId, result_ref: Option<String>) -> bool {
        self.end(id, JobStatus::Done, result_ref, None)
    }

    pub fn fail(&self, id: &JobId, error: &str) -> bool {
        self.end(id, JobStatus::Failed, None, Some(error.to_owned()))
    }

    /// The worker stopped because of a cancel (or on its own account).
    pub fn cancelled(&self, id: &JobId) -> bool {
        self.end(id, JobStatus::Cancelled, None, None)
    }

    /// Ask a job to stop: its token is cancelled and the job is shown as cancelled right away.
    /// `false` when no such job is running.
    pub fn cancel(&self, id: &JobId) -> bool {
        let token = self
            .live
            .lock()
            .ok()
            .and_then(|m| m.get(id.as_str()).map(|l| l.token.clone()));
        match token {
            Some(t) => {
                t.cancel();
                self.cancelled(id)
            }
            None => false,
        }
    }

    /// Jobs in flight, then recent finished ones.
    pub fn list(&self, limit: u32) -> Result<Vec<JobSummary>, StoreError> {
        let mut out: Vec<JobSummary> = self.store.list_jobs(limit)?;
        if let Ok(m) = self.live.lock() {
            for j in &mut out {
                if let Some(l) = m.get(j.id.as_str()) {
                    *j = l.summary.clone();
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(interval: Duration) -> (Arc<Store>, JobRegistry, Arc<Mutex<Vec<JobSummary>>>) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let seen: Arc<Mutex<Vec<JobSummary>>> = Arc::default();
        let sink = seen.clone();
        let reg = JobRegistry::with_interval(
            store.clone(),
            Arc::new(move |j| sink.lock().unwrap().push(j)),
            interval,
        );
        (store, reg, seen)
    }

    #[test]
    fn lifecycle_is_persisted_and_emitted() {
        let (store, reg, seen) = registry(Duration::ZERO);
        let h = reg.start("scan", "Scanning Downloads").unwrap();
        reg.progress(&h.id, Some(0.5), Some("1 000 files".into()));
        assert_eq!(store.get_job(h.id.as_str()).unwrap().unwrap().progress, Some(0.5));
        assert!(reg.finish(&h.id, Some("/analyzer?report=r1".into())));
        let j = store.get_job(h.id.as_str()).unwrap().unwrap();
        assert_eq!(
            (j.status, j.progress, j.result_ref.as_deref()),
            (JobStatus::Done, Some(1.0), Some("/analyzer?report=r1"))
        );
        let statuses: Vec<JobStatus> = seen.lock().unwrap().iter().map(|j| j.status).collect();
        assert_eq!(
            statuses,
            [JobStatus::Running, JobStatus::Running, JobStatus::Done]
        );
        assert!(!reg.finish(&h.id, None), "a finished job cannot finish again");
        reg.progress(&h.id, Some(0.1), None);
        assert_eq!(seen.lock().unwrap().len(), 3, "no updates after the end");
    }

    #[test]
    fn progress_is_throttled_but_the_end_always_goes_out() {
        let (_s, reg, seen) = registry(Duration::from_secs(3600));
        let h = reg.start("download", "Model").unwrap();
        for i in 0..100 {
            reg.progress(&h.id, Some(i as f32 / 100.0), None);
        }
        assert_eq!(seen.lock().unwrap().len(), 1, "only the start so far");
        reg.fail(&h.id, "disk full");
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1].status, JobStatus::Failed);
        assert_eq!(seen[1].error.as_deref(), Some("disk full"));
        assert_eq!(seen[1].progress, Some(0.99), "the latest progress is kept");
    }

    #[test]
    fn cancel_trips_the_token_and_marks_the_job() {
        let (store, reg, _seen) = registry(Duration::ZERO);
        let h = reg.start("rename", "Proposals").unwrap();
        assert!(!h.token.is_cancelled());
        assert!(reg.cancel(&h.id));
        assert!(h.token.is_cancelled());
        assert_eq!(
            store.get_job(h.id.as_str()).unwrap().unwrap().status,
            JobStatus::Cancelled
        );
        assert!(!reg.cancel(&h.id));
        assert!(!reg.cancel(&JobId::from("nope")));
    }

    #[test]
    fn startup_recovery_fails_interrupted_jobs_and_prunes() {
        let (store, reg, _seen) = registry(Duration::ZERO);
        let running = reg.start("scan", "A").unwrap();
        let done = reg.start("scan", "B").unwrap();
        reg.finish(&done.id, None);
        // A new process: same database, empty registry.
        let reg2 = JobRegistry::new(store.clone(), Arc::new(|_| {}));
        assert_eq!(reg2.recover().unwrap(), 1);
        let j = store.get_job(running.id.as_str()).unwrap().unwrap();
        assert_eq!(j.status, JobStatus::Failed);
        assert!(j.error.unwrap().contains("restart"));
        assert_eq!(
            store.get_job(done.id.as_str()).unwrap().unwrap().status,
            JobStatus::Done
        );
        assert_eq!(store.prune_jobs(1).unwrap(), 1);
        assert_eq!(store.list_jobs(10).unwrap().len(), 1);
    }

    #[test]
    fn list_prefers_live_state() {
        let (_s, reg, _seen) = registry(Duration::from_secs(3600));
        let h = reg.start("scan", "A").unwrap();
        reg.progress(&h.id, Some(0.7), None);
        let listed = reg.list(10).unwrap();
        assert_eq!(listed[0].progress, Some(0.7), "not yet persisted, but listed");
    }
}
