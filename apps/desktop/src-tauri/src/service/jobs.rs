//! Jobs center: the shared registry of long-running work (scan, download, rename proposals).
//! Services start a job, hand its cancel token to the worker, and end it with [`end`].

use nexus_core::error::NexusError;
use nexus_core::events::JobSummary;
use nexus_core::ids::JobId;

use crate::state::AppCtx;

/// Running jobs first, then recent finished ones.
pub fn list(ctx: &AppCtx, limit: u32) -> Result<Vec<JobSummary>, NexusError> {
    Ok(ctx.jobs.list(limit.clamp(1, 200))?)
}

/// Ask a job to stop. `false` when it is not running.
pub fn cancel(ctx: &AppCtx, id: &str) -> bool {
    ctx.jobs.cancel(&JobId::from(id))
}

/// Close a job from the worker's result: `Cancelled` → cancelled, other errors → failed,
/// success → done with the route that opens the result.
pub fn end<T>(ctx: &AppCtx, id: &JobId, result: &Result<T, NexusError>, result_ref: impl FnOnce(&T) -> Option<String>) {
    match result {
        Ok(v) => {
            ctx.jobs.finish(id, result_ref(v));
        }
        Err(NexusError::Cancelled) => {
            ctx.jobs.cancelled(id);
        }
        Err(e) => {
            ctx.jobs.fail(id, &e.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::events::JobStatus;

    #[tokio::test]
    async fn end_maps_results_to_statuses() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppCtx::for_tests(dir.path());
        let a = ctx.jobs.start("scan", "A").unwrap();
        end(&ctx, &a.id, &Ok::<_, NexusError>("r1".to_owned()), |r| Some(format!("/analyzer?report={r}")));
        let b = ctx.jobs.start("scan", "B").unwrap();
        end(&ctx, &b.id, &Err::<String, _>(NexusError::Cancelled), |_| None);
        let c = ctx.jobs.start("scan", "C").unwrap();
        end(&ctx, &c.id, &Err::<String, _>(NexusError::InvalidInput("nope".into())), |_| None);
        let jobs = list(&ctx, 10).unwrap();
        let by = |t: &str| jobs.iter().find(|j| j.title == t).unwrap().clone();
        assert_eq!(by("A").status, JobStatus::Done);
        assert_eq!(by("A").result_ref.as_deref(), Some("/analyzer?report=r1"));
        assert_eq!(by("B").status, JobStatus::Cancelled);
        assert_eq!(by("C").status, JobStatus::Failed);
        assert!(by("C").error.is_some());
    }

    #[tokio::test]
    async fn cancel_stops_a_running_job() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppCtx::for_tests(dir.path());
        let h = ctx.jobs.start("download", "D").unwrap();
        assert!(cancel(&ctx, h.id.as_str()));
        assert!(h.token.is_cancelled());
        assert!(!cancel(&ctx, h.id.as_str()), "already finished");
    }
}
