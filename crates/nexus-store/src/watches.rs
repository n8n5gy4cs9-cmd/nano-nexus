//! Persistence for folder watches (ANALYZER §4, M11).

use rusqlite::{OptionalExtension, Row, params};

use crate::{Store, StoreError, now_ms};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchRow {
    pub id: String,
    pub root: String,
    /// `daily | weekly | on_change`
    pub schedule: String,
    pub last_report_id: Option<String>,
    pub enabled: bool,
    pub created_at: i64,
}

fn row(r: &Row<'_>) -> rusqlite::Result<WatchRow> {
    Ok(WatchRow {
        id: r.get("id")?,
        root: r.get("root")?,
        schedule: r.get("schedule")?,
        last_report_id: r.get("last_report_id")?,
        enabled: r.get::<_, i64>("enabled")? != 0,
        created_at: r.get("created_at")?,
    })
}

impl Store {
    pub fn insert_watch(&self, id: &str, root: &str, schedule: &str) -> Result<WatchRow, StoreError> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO watches (id, root, schedule, enabled, created_at) VALUES (?1,?2,?3,1,?4)",
                params![id, root, schedule, now_ms()],
            )?;
            Ok(c.query_row("SELECT * FROM watches WHERE id = ?1", params![id], row)?)
        })
    }

    pub fn get_watch(&self, id: &str) -> Result<Option<WatchRow>, StoreError> {
        self.with_conn(|c| {
            Ok(
                c.query_row("SELECT * FROM watches WHERE id = ?1", params![id], row)
                    .optional()?,
            )
        })
    }

    /// Oldest first.
    pub fn list_watches(&self) -> Result<Vec<WatchRow>, StoreError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT * FROM watches ORDER BY created_at, id")?;
            Ok(stmt.query_map([], row)?.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn set_watch_enabled(&self, id: &str, enabled: bool) -> Result<(), StoreError> {
        self.with_conn(|c| {
            let n = c.execute(
                "UPDATE watches SET enabled = ?2 WHERE id = ?1",
                params![id, enabled as i64],
            )?;
            if n == 0 {
                Err(StoreError::NotFound("watch"))
            } else {
                Ok(())
            }
        })
    }

    pub fn set_watch_schedule(&self, id: &str, schedule: &str) -> Result<(), StoreError> {
        self.with_conn(|c| {
            let n = c.execute(
                "UPDATE watches SET schedule = ?2 WHERE id = ?1",
                params![id, schedule],
            )?;
            if n == 0 {
                Err(StoreError::NotFound("watch"))
            } else {
                Ok(())
            }
        })
    }

    /// Remember the report of the latest run (the next run diffs against it).
    pub fn set_watch_last_report(&self, id: &str, report_id: &str) -> Result<(), StoreError> {
        self.with_conn(|c| {
            let n = c.execute(
                "UPDATE watches SET last_report_id = ?2 WHERE id = ?1",
                params![id, report_id],
            )?;
            if n == 0 {
                Err(StoreError::NotFound("watch"))
            } else {
                Ok(())
            }
        })
    }

    pub fn delete_watch(&self, id: &str) -> Result<bool, StoreError> {
        self.with_conn(|c| Ok(c.execute("DELETE FROM watches WHERE id = ?1", params![id])? > 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud() {
        let store = Store::open_in_memory().unwrap();
        let w = store.insert_watch("w1", "/data/Downloads", "weekly").unwrap();
        assert!(w.enabled && w.last_report_id.is_none());
        assert!(
            store.insert_watch("w2", "/x", "hourly").is_err(),
            "schedule is constrained"
        );
        store.set_watch_schedule("w1", "daily").unwrap();
        store.set_watch_enabled("w1", false).unwrap();
        let got = store.get_watch("w1").unwrap().unwrap();
        assert_eq!((got.schedule.as_str(), got.enabled), ("daily", false));
        assert!(store.set_watch_enabled("nope", true).is_err());
        assert_eq!(store.list_watches().unwrap().len(), 1);
        assert!(store.delete_watch("w1").unwrap());
        assert!(!store.delete_watch("w1").unwrap());
    }
}
