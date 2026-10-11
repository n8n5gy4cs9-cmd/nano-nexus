//! Persistence for the report index (the report bodies are JSON files in the data folder).

use rusqlite::{OptionalExtension, Row, params};

use crate::{Store, StoreError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportRow {
    pub id: String,
    /// `folder | file | rename | secrets | logs | data`
    pub kind: String,
    /// The folder or file that was analyzed.
    pub target: String,
    pub title: String,
    pub summary: Option<String>,
    pub json_path: String,
    pub ai_narrative: Option<String>,
    pub created_at: i64,
}

fn row(r: &Row<'_>) -> rusqlite::Result<ReportRow> {
    Ok(ReportRow {
        id: r.get("id")?,
        kind: r.get("kind")?,
        target: r.get("target")?,
        title: r.get("title")?,
        summary: r.get("summary")?,
        json_path: r.get("json_path")?,
        ai_narrative: r.get("ai_narrative")?,
        created_at: r.get("created_at")?,
    })
}

impl Store {
    pub fn insert_report(&self, r: &ReportRow) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO reports (id, kind, target, title, summary, json_path, ai_narrative, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![r.id, r.kind, r.target, r.title, r.summary, r.json_path, r.ai_narrative, r.created_at],
            )?;
            Ok(())
        })
    }

    pub fn get_report(&self, id: &str) -> Result<Option<ReportRow>, StoreError> {
        self.with_conn(|c| Ok(c.query_row("SELECT * FROM reports WHERE id = ?1", params![id], row).optional()?))
    }

    /// Newest first, optionally one kind only.
    pub fn list_reports(&self, kind: Option<&str>, limit: u32) -> Result<Vec<ReportRow>, StoreError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT * FROM reports WHERE (?1 IS NULL OR kind = ?1) ORDER BY created_at DESC, id DESC LIMIT ?2")?;
            Ok(stmt.query_map(params![kind, limit], row)?.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// The newest report of `kind` for `target` (to compare scans).
    pub fn latest_report_for(&self, kind: &str, target: &str, before: i64) -> Result<Option<ReportRow>, StoreError> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT * FROM reports WHERE kind = ?1 AND target = ?2 AND created_at < ?3 ORDER BY created_at DESC LIMIT 1",
                params![kind, target, before],
                row,
            )
            .optional()?)
        })
    }

    pub fn set_report_narrative(&self, id: &str, text: &str) -> Result<(), StoreError> {
        self.with_conn(|c| {
            let n = c.execute("UPDATE reports SET ai_narrative = ?2 WHERE id = ?1", params![id, text])?;
            if n == 0 { Err(StoreError::NotFound("report")) } else { Ok(()) }
        })
    }

    pub fn delete_report(&self, id: &str) -> Result<bool, StoreError> {
        self.with_conn(|c| Ok(c.execute("DELETE FROM reports WHERE id = ?1", params![id])? > 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(id: &str, kind: &str, target: &str, at: i64) -> ReportRow {
        ReportRow { id: id.into(), kind: kind.into(), target: target.into(), title: format!("t {id}"), summary: None, json_path: format!("/r/{id}.json"), ai_narrative: None, created_at: at }
    }

    #[test]
    fn insert_list_filter_and_delete() {
        let s = Store::open_in_memory().unwrap();
        s.insert_report(&r("a", "folder", "/d", 1)).unwrap();
        s.insert_report(&r("b", "secrets", "/d", 2)).unwrap();
        s.insert_report(&r("c", "folder", "/d", 3)).unwrap();
        assert_eq!(s.list_reports(None, 10).unwrap().iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["c", "b", "a"]);
        assert_eq!(s.list_reports(Some("folder"), 10).unwrap().len(), 2);
        assert_eq!(s.list_reports(None, 1).unwrap().len(), 1);
        assert_eq!(s.latest_report_for("folder", "/d", 3).unwrap().unwrap().id, "a");
        assert!(s.latest_report_for("folder", "/other", 99).unwrap().is_none());
        assert!(s.delete_report("a").unwrap() && !s.delete_report("a").unwrap());
    }

    #[test]
    fn narratives_are_saved_and_missing_reports_error() {
        let s = Store::open_in_memory().unwrap();
        s.insert_report(&r("a", "folder", "/d", 1)).unwrap();
        s.set_report_narrative("a", "You have 3 files.").unwrap();
        assert_eq!(s.get_report("a").unwrap().unwrap().ai_narrative.as_deref(), Some("You have 3 files."));
        assert!(matches!(s.set_report_narrative("zz", "x"), Err(StoreError::NotFound(_))));
    }
}
