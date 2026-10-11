use nexus_core::error::NexusError;
use nexus_core::events::NotificationPayload;

use crate::state::AppCtx;

pub fn list(ctx: &AppCtx, limit: u32) -> Result<Vec<NotificationPayload>, NexusError> {
    Ok(ctx.store.list_notifications(limit.min(200))?)
}

pub fn unread_count(ctx: &AppCtx) -> Result<u32, NexusError> {
    Ok(ctx.store.unread_notification_count()?)
}

/// Mark the given ids read; an empty list marks everything read.
pub fn mark_read(ctx: &AppCtx, ids: &[String]) -> Result<(), NexusError> {
    Ok(ctx.store.mark_notifications_read(ids)?)
}

pub fn clear(ctx: &AppCtx) -> Result<(), NexusError> {
    Ok(ctx.store.clear_notifications()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::events::NotificationLevel;
    use nexus_store::NewNotification;

    #[tokio::test]
    async fn list_count_mark_clear() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppCtx::for_tests(dir.path());
        for t in ["a", "b"] {
            ctx.store
                .insert_notification(NewNotification {
                    level: NotificationLevel::Info,
                    kind: "k".into(),
                    title: t.into(),
                    body: None,
                    action: None,
                })
                .unwrap();
        }
        assert_eq!(unread_count(&ctx).unwrap(), 2);
        assert_eq!(list(&ctx, 1).unwrap().len(), 1);
        mark_read(&ctx, &[]).unwrap();
        assert_eq!(unread_count(&ctx).unwrap(), 0);
        clear(&ctx).unwrap();
        assert!(list(&ctx, 10).unwrap().is_empty());
    }
}
