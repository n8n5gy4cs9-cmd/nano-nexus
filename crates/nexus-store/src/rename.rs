//! Persistence for Rename Forge sessions and the user's per-symbol decisions (the index itself is a
//! cache file; see migration 0007).

use rusqlite::{OptionalExtension, Row, params};

use crate::{Store, StoreError, now_ms};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameSessionRow {
    pub id: String,
    pub root: String,
    /// JSON array of language names.
    pub langs: String,
    pub index_path: String,
    /// `indexing | ready | proposing | planned | applied | failed`
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenameItemRow {
    pub session_id: String,
    pub symbol_id: String,
    pub old_name: String,
    pub kind: String,
    pub proposed: Option<String>,
    /// The name the user settled on (edited or accepted).
    pub final_name: Option<String>,
    pub confidence: Option<f64>,
    pub reason: Option<String>,
    /// JSON array of rule ids.
    pub issues: Option<String>,
    /// JSON array of flags.
    pub flags: Option<String>,
    /// `candidate | proposed | accepted | rejected | needs_manual | applied`
    pub status: String,
    pub include_strings: bool,
}

/// Fields to change on one item; `None` leaves a field alone.
#[derive(Clone, Debug, Default)]
pub struct RenameItemPatch {
    pub proposed: Option<Option<String>>,
    pub final_name: Option<Option<String>>,
    pub confidence: Option<f64>,
    pub reason: Option<String>,
    pub status: Option<String>,
    pub include_strings: Option<bool>,
}

fn session(r: &Row<'_>) -> rusqlite::Result<RenameSessionRow> {
    Ok(RenameSessionRow {
        id: r.get("id")?,
        root: r.get("root")?,
        langs: r.get("langs")?,
        index_path: r.get("index_path")?,
        status: r.get("status")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

fn item(r: &Row<'_>) -> rusqlite::Result<RenameItemRow> {
    Ok(RenameItemRow {
        session_id: r.get("session_id")?,
        symbol_id: r.get("symbol_id")?,
        old_name: r.get("old_name")?,
        kind: r.get("kind")?,
        proposed: r.get("proposed")?,
        final_name: r.get("final")?,
        confidence: r.get("confidence")?,
        reason: r.get("reason")?,
        issues: r.get("issues")?,
        flags: r.get("flags")?,
        status: r.get("status")?,
        include_strings: r.get::<_, i64>("include_strings")? != 0,
    })
}

impl Store {
    pub fn insert_rename_session(&self, s: &RenameSessionRow) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO rename_sessions (id, root, langs, index_path, status, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![s.id, s.root, s.langs, s.index_path, s.status, s.created_at, s.updated_at],
            )?;
            Ok(())
        })
    }

    pub fn get_rename_session(&self, id: &str) -> Result<Option<RenameSessionRow>, StoreError> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT * FROM rename_sessions WHERE id = ?1",
                params![id],
                session,
            )
            .optional()?)
        })
    }

    /// Newest first.
    pub fn list_rename_sessions(&self, limit: u32) -> Result<Vec<RenameSessionRow>, StoreError> {
        self.with_conn(|c| {
            let mut stmt =
                c.prepare("SELECT * FROM rename_sessions ORDER BY updated_at DESC, id DESC LIMIT ?1")?;
            Ok(stmt
                .query_map(params![limit], session)?
                .collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Set the status, and optionally the language list (after indexing).
    pub fn set_rename_session_status(
        &self,
        id: &str,
        status: &str,
        langs: Option<&str>,
    ) -> Result<(), StoreError> {
        self.with_conn(|c| {
            let n = c.execute(
                "UPDATE rename_sessions SET status = ?2, langs = COALESCE(?3, langs), updated_at = ?4 WHERE id = ?1",
                params![id, status, langs, now_ms()],
            )?;
            if n == 0 { Err(StoreError::NotFound("rename session")) } else { Ok(()) }
        })
    }

    /// Deletes the session and (by cascade) its items.
    pub fn delete_rename_session(&self, id: &str) -> Result<bool, StoreError> {
        self.with_conn(|c| Ok(c.execute("DELETE FROM rename_sessions WHERE id = ?1", params![id])? > 0))
    }

    /// Insert or replace items in one transaction.
    pub fn upsert_rename_items(&self, items: &[RenameItemRow]) -> Result<(), StoreError> {
        self.with_conn(|c| {
            let tx = c.transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT OR REPLACE INTO rename_items (session_id, symbol_id, old_name, kind, proposed, final, confidence, reason, issues, flags, status, include_strings)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                )?;
                for i in items {
                    stmt.execute(params![
                        i.session_id, i.symbol_id, i.old_name, i.kind, i.proposed, i.final_name, i.confidence,
                        i.reason, i.issues, i.flags, i.status, i.include_strings as i64
                    ])?;
                }
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Items of a session, optionally one status, in insertion order.
    pub fn list_rename_items(
        &self,
        session_id: &str,
        status: Option<&str>,
    ) -> Result<Vec<RenameItemRow>, StoreError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM rename_items WHERE session_id = ?1 AND (?2 IS NULL OR status = ?2) ORDER BY rowid",
            )?;
            Ok(stmt
                .query_map(params![session_id, status], item)?
                .collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn get_rename_item(
        &self,
        session_id: &str,
        symbol_id: &str,
    ) -> Result<Option<RenameItemRow>, StoreError> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT * FROM rename_items WHERE session_id = ?1 AND symbol_id = ?2",
                params![session_id, symbol_id],
                item,
            )
            .optional()?)
        })
    }

    /// Apply a patch and return the updated row.
    pub fn update_rename_item(
        &self,
        session_id: &str,
        symbol_id: &str,
        p: &RenameItemPatch,
    ) -> Result<RenameItemRow, StoreError> {
        self.with_conn(|c| {
            let tx = c.transaction()?;
            let mut cur = tx
                .query_row(
                    "SELECT * FROM rename_items WHERE session_id = ?1 AND symbol_id = ?2",
                    params![session_id, symbol_id],
                    item,
                )
                .optional()?
                .ok_or(StoreError::NotFound("rename item"))?;
            if let Some(v) = &p.proposed {
                cur.proposed.clone_from(v);
            }
            if let Some(v) = &p.final_name {
                cur.final_name.clone_from(v);
            }
            if let Some(v) = p.confidence {
                cur.confidence = Some(v);
            }
            if let Some(v) = &p.reason {
                cur.reason = Some(v.clone());
            }
            if let Some(v) = &p.status {
                cur.status.clone_from(v);
            }
            if let Some(v) = p.include_strings {
                cur.include_strings = v;
            }
            tx.execute(
                "UPDATE rename_items SET proposed = ?3, final = ?4, confidence = ?5, reason = ?6, status = ?7, include_strings = ?8
                 WHERE session_id = ?1 AND symbol_id = ?2",
                params![
                    session_id, symbol_id, cur.proposed, cur.final_name, cur.confidence, cur.reason, cur.status,
                    cur.include_strings as i64
                ],
            )?;
            tx.commit()?;
            Ok(cur)
        })
    }

    /// Set the status of every item of a session that has `from` (e.g. `accepted` → `applied`).
    pub fn bulk_set_rename_status(
        &self,
        session_id: &str,
        from: &str,
        to: &str,
    ) -> Result<usize, StoreError> {
        self.with_conn(|c| {
            Ok(c.execute(
                "UPDATE rename_items SET status = ?3 WHERE session_id = ?1 AND status = ?2",
                params![session_id, from, to],
            )?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sess(id: &str, at: i64) -> RenameSessionRow {
        RenameSessionRow {
            id: id.into(),
            root: "/p".into(),
            langs: "[\"python\"]".into(),
            index_path: format!("/cache/{id}/index.bin"),
            status: "indexing".into(),
            created_at: at,
            updated_at: at,
        }
    }

    fn it(sid: &str, sym: &str, old: &str) -> RenameItemRow {
        RenameItemRow {
            session_id: sid.into(),
            symbol_id: sym.into(),
            old_name: old.into(),
            kind: "function".into(),
            proposed: None,
            final_name: None,
            confidence: None,
            reason: None,
            issues: Some("[\"snake\"]".into()),
            flags: None,
            status: "candidate".into(),
            include_strings: false,
        }
    }

    #[test]
    fn sessions_roundtrip_and_cascade() {
        let s = Store::open_in_memory().unwrap();
        s.insert_rename_session(&sess("a", 1)).unwrap();
        s.insert_rename_session(&sess("b", 2)).unwrap();
        assert_eq!(s.list_rename_sessions(10).unwrap()[0].id, "b");
        s.set_rename_session_status("a", "ready", Some("[\"go\"]"))
            .unwrap();
        let a = s.get_rename_session("a").unwrap().unwrap();
        assert_eq!((a.status.as_str(), a.langs.as_str()), ("ready", "[\"go\"]"));
        s.set_rename_session_status("a", "proposing", None).unwrap();
        assert_eq!(s.get_rename_session("a").unwrap().unwrap().langs, "[\"go\"]");
        assert!(matches!(
            s.set_rename_session_status("zz", "x", None),
            Err(StoreError::NotFound(_))
        ));

        s.upsert_rename_items(&[it("a", "1", "doStuff"), it("a", "2", "Foo")])
            .unwrap();
        assert!(s.delete_rename_session("a").unwrap());
        assert!(
            s.list_rename_items("a", None).unwrap().is_empty(),
            "items cascade"
        );
        assert!(!s.delete_rename_session("a").unwrap());
    }

    #[test]
    fn items_are_patched_and_filtered() {
        let s = Store::open_in_memory().unwrap();
        s.insert_rename_session(&sess("a", 1)).unwrap();
        s.upsert_rename_items(&[it("a", "1", "doStuff"), it("a", "2", "Foo"), it("a", "3", "bar")])
            .unwrap();
        let u = s
            .update_rename_item(
                "a",
                "1",
                &RenameItemPatch {
                    proposed: Some(Some("do_stuff".into())),
                    final_name: Some(Some("do_stuff".into())),
                    confidence: Some(0.9),
                    status: Some("accepted".into()),
                    ..RenameItemPatch::default()
                },
            )
            .unwrap();
        assert_eq!(
            (u.final_name.as_deref(), u.status.as_str()),
            (Some("do_stuff"), "accepted")
        );
        assert_eq!(u.old_name, "doStuff", "untouched fields stay");
        // Clearing a field is explicit.
        let u = s
            .update_rename_item(
                "a",
                "1",
                &RenameItemPatch {
                    final_name: Some(None),
                    ..RenameItemPatch::default()
                },
            )
            .unwrap();
        assert_eq!(u.final_name, None);
        assert_eq!(s.list_rename_items("a", Some("accepted")).unwrap().len(), 1);
        assert_eq!(s.list_rename_items("a", Some("candidate")).unwrap().len(), 2);
        assert_eq!(
            s.list_rename_items("a", None)
                .unwrap()
                .iter()
                .map(|i| i.symbol_id.as_str())
                .collect::<Vec<_>>(),
            ["1", "2", "3"]
        );
        assert_eq!(s.bulk_set_rename_status("a", "accepted", "applied").unwrap(), 1);
        assert_eq!(s.get_rename_item("a", "1").unwrap().unwrap().status, "applied");
        assert!(matches!(
            s.update_rename_item("a", "9", &RenameItemPatch::default()),
            Err(StoreError::NotFound(_))
        ));
        assert!(
            s.update_rename_item(
                "a",
                "2",
                &RenameItemPatch {
                    status: Some("bogus".into()),
                    ..RenameItemPatch::default()
                }
            )
            .is_err(),
            "the CHECK constraint guards statuses"
        );
    }
}
