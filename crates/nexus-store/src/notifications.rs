//! Notification persistence.

use nexus_core::events::{NotificationAction, NotificationLevel, NotificationPayload};
use nexus_core::ids::NotificationId;
use rusqlite::{Row, params};

use crate::{Store, StoreError, now_ms};

/// What a caller supplies; id, `read`, and the timestamp are assigned on insert.
#[derive(Clone, Debug)]
pub struct NewNotification {
    pub level: NotificationLevel,
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub action: Option<NotificationAction>,
}

fn level_str(l: NotificationLevel) -> &'static str {
    match l {
        NotificationLevel::Info => "info",
        NotificationLevel::Success => "success",
        NotificationLevel::Warning => "warning",
        NotificationLevel::Error => "error",
    }
}

fn parse_level(s: &str) -> NotificationLevel {
    match s {
        "success" => NotificationLevel::Success,
        "warning" => NotificationLevel::Warning,
        "error" => NotificationLevel::Error,
        _ => NotificationLevel::Info,
    }
}

fn row(r: &Row<'_>) -> rusqlite::Result<NotificationPayload> {
    let action: Option<String> = r.get("action")?;
    Ok(NotificationPayload {
        id: NotificationId(r.get("id")?),
        level: parse_level(&r.get::<_, String>("level")?),
        kind: r.get("kind")?,
        title: r.get("title")?,
        body: r.get("body")?,
        action: action.and_then(|a| serde_json::from_str(&a).ok()),
        read: r.get::<_, i64>("read")? != 0,
        created_at: r.get("created_at")?,
    })
}

impl Store {
    /// Insert a notification and return it as stored.
    pub fn insert_notification(&self, n: NewNotification) -> Result<NotificationPayload, StoreError> {
        let payload = NotificationPayload {
            id: NotificationId::new(),
            level: n.level,
            kind: n.kind,
            title: n.title,
            body: n.body,
            action: n.action,
            read: false,
            created_at: now_ms(),
        };
        let action = payload
            .action
            .as_ref()
            .and_then(|a| serde_json::to_string(a).ok());
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO notifications (id, level, kind, title, body, action, read, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)",
                params![
                    payload.id.as_str(),
                    level_str(payload.level),
                    payload.kind,
                    payload.title,
                    payload.body,
                    action,
                    payload.created_at
                ],
            )?;
            Ok(())
        })?;
        Ok(payload)
    }

    /// Newest first.
    pub fn list_notifications(&self, limit: u32) -> Result<Vec<NotificationPayload>, StoreError> {
        self.with_conn(|c| {
            let mut stmt =
                c.prepare("SELECT * FROM notifications ORDER BY created_at DESC, id DESC LIMIT ?1")?;
            let rows = stmt.query_map([limit], row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn unread_notification_count(&self) -> Result<u32, StoreError> {
        self.with_conn(|c| {
            Ok(
                c.query_row("SELECT COUNT(*) FROM notifications WHERE read = 0", [], |r| {
                    r.get(0)
                })?,
            )
        })
    }

    /// Mark the given ids read (an empty slice marks everything read).
    pub fn mark_notifications_read(&self, ids: &[String]) -> Result<(), StoreError> {
        self.with_conn(|c| {
            if ids.is_empty() {
                c.execute("UPDATE notifications SET read = 1 WHERE read = 0", [])?;
            } else {
                let tx = c.transaction()?;
                for id in ids {
                    tx.execute("UPDATE notifications SET read = 1 WHERE id = ?1", [id])?;
                }
                tx.commit()?;
            }
            Ok(())
        })
    }

    /// Delete every notification.
    pub fn clear_notifications(&self) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute("DELETE FROM notifications", [])?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(title: &str) -> NewNotification {
        NewNotification {
            level: NotificationLevel::Success,
            kind: "download".into(),
            title: title.into(),
            body: Some("body".into()),
            action: Some(NotificationAction {
                label: "Open".into(),
                route: "/models".into(),
            }),
        }
    }

    #[test]
    fn insert_list_and_unread_count() {
        let s = Store::open_in_memory().unwrap();
        let a = s.insert_notification(new("one")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = s.insert_notification(new("two")).unwrap();
        let list = s.list_notifications(10).unwrap();
        assert_eq!(
            list.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(),
            ["two", "one"]
        );
        assert_eq!(list[0].id, b.id);
        assert_eq!(list[1].action.as_ref().unwrap().route, "/models");
        assert_eq!(list[1].level, NotificationLevel::Success);
        assert_eq!(s.unread_notification_count().unwrap(), 2);
        assert_eq!(s.list_notifications(1).unwrap().len(), 1);
        let _ = a;
    }

    #[test]
    fn mark_read_some_then_all_then_clear() {
        let s = Store::open_in_memory().unwrap();
        let a = s.insert_notification(new("a")).unwrap();
        s.insert_notification(new("b")).unwrap();
        s.mark_notifications_read(std::slice::from_ref(&a.id.0)).unwrap();
        assert_eq!(s.unread_notification_count().unwrap(), 1);
        s.mark_notifications_read(&[]).unwrap();
        assert_eq!(s.unread_notification_count().unwrap(), 0);
        assert!(s.list_notifications(10).unwrap().iter().all(|n| n.read));
        s.clear_notifications().unwrap();
        assert!(s.list_notifications(10).unwrap().is_empty());
    }

    #[test]
    fn level_check_constraint_is_enforced() {
        let s = Store::open_in_memory().unwrap();
        let r = s.with_conn(|c| {
            c.execute("INSERT INTO notifications (id, level, kind, title, created_at) VALUES ('x','bogus','k','t',1)", [])?;
            Ok(())
        });
        assert!(r.is_err());
    }
}
