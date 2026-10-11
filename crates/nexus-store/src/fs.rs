//! Persistence for grants, user deny rules, the audit log, and change sets.

use nexus_core::fs::{AuditRecord, Grant, GrantLevel, GrantScope, UserDenyRule};
use nexus_core::ids::GrantId;
use rusqlite::{OptionalExtension, Row, params};

use crate::{Store, StoreError, now_ms};

/// A row to append to the audit log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewAudit {
    pub ts: i64,
    pub actor: String,
    pub action: String,
    pub path: String,
    pub allowed: bool,
    pub detail: Option<String>,
}

/// A stored change set. `ops` is the JSON of the operation list (defined by `nexus-fs`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeSetRow {
    pub id: String,
    pub title: String,
    pub origin: String,
    pub ops: String,
    /// `planned | applied | rolled_back | undone | discarded | failed`
    pub status: String,
    pub stats: Option<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub applied_at: Option<i64>,
    pub undone_at: Option<i64>,
}

fn grant_row(r: &Row<'_>) -> rusqlite::Result<Grant> {
    let level: String = r.get("level")?;
    Ok(Grant {
        id: GrantId(r.get("id")?),
        root: r.get("root")?,
        level: GrantLevel::parse(&level).unwrap_or(GrantLevel::Read),
        recursive: r.get::<_, i64>("recursive")? != 0,
        scope: GrantScope::Persistent,
        expires_at: r.get("expires_at")?,
        label: r.get("label")?,
        created_at: r.get("created_at")?,
    })
}

fn changeset_row(r: &Row<'_>) -> rusqlite::Result<ChangeSetRow> {
    Ok(ChangeSetRow {
        id: r.get("id")?,
        title: r.get("title")?,
        origin: r.get("origin")?,
        ops: r.get("ops")?,
        status: r.get("status")?,
        stats: r.get("stats")?,
        error: r.get("error")?,
        created_at: r.get("created_at")?,
        applied_at: r.get("applied_at")?,
        undone_at: r.get("undone_at")?,
    })
}

impl Store {
    // ---- grants ---------------------------------------------------------------------------

    /// Persistent grants (loaded into the engine at startup).
    pub fn list_grants(&self) -> Result<Vec<Grant>, StoreError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT * FROM grants ORDER BY created_at")?;
            Ok(stmt.query_map([], grant_row)?.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Store a persistent grant, replacing any grant for the same root. Session grants are ignored.
    pub fn save_grant(&self, g: &Grant) -> Result<(), StoreError> {
        if g.scope == GrantScope::Session {
            return Ok(());
        }
        self.with_conn(|c| {
            let tx = c.transaction()?;
            tx.execute("DELETE FROM grants WHERE root = ?1", [&g.root])?;
            tx.execute(
                "INSERT INTO grants (id, root, level, recursive, label, expires_at, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![g.id.as_str(), g.root, g.level.as_str(), g.recursive, g.label, g.expires_at, g.created_at],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    pub fn delete_grant(&self, id: &GrantId) -> Result<bool, StoreError> {
        self.with_conn(|c| Ok(c.execute("DELETE FROM grants WHERE id = ?1", [id.as_str()])? > 0))
    }

    // ---- user deny rules -------------------------------------------------------------------

    pub fn list_user_deny(&self) -> Result<Vec<UserDenyRule>, StoreError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT id, root, created_at FROM deny_rules ORDER BY created_at")?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(UserDenyRule {
                        id: r.get(0)?,
                        root: r.get(1)?,
                        created_at: r.get(2)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Add a protected location (idempotent per root).
    pub fn add_user_deny(&self, root: &str) -> Result<UserDenyRule, StoreError> {
        let rule = UserDenyRule {
            id: GrantId::new().0,
            root: root.to_owned(),
            created_at: now_ms(),
        };
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO deny_rules (id, root, created_at) VALUES (?1, ?2, ?3) ON CONFLICT(root) DO NOTHING",
                params![rule.id, rule.root, rule.created_at],
            )?;
            Ok(c.query_row("SELECT id, root, created_at FROM deny_rules WHERE root = ?1", [root], |r| {
                Ok(UserDenyRule { id: r.get(0)?, root: r.get(1)?, created_at: r.get(2)? })
            })?)
        })
    }

    pub fn remove_user_deny(&self, root: &str) -> Result<bool, StoreError> {
        self.with_conn(|c| Ok(c.execute("DELETE FROM deny_rules WHERE root = ?1", [root])? > 0))
    }

    // ---- audit log -------------------------------------------------------------------------

    /// Append many entries in one transaction.
    pub fn insert_audit_batch(&self, entries: &[NewAudit]) -> Result<(), StoreError> {
        if entries.is_empty() {
            return Ok(());
        }
        self.with_conn(|c| {
            let tx = c.transaction()?;
            {
                let mut stmt = tx.prepare("INSERT INTO audit_log (ts, actor, action, path, allowed, detail) VALUES (?1,?2,?3,?4,?5,?6)")?;
                for e in entries {
                    stmt.execute(params![e.ts, e.actor, e.action, e.path, e.allowed, e.detail])?;
                }
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Newest first; `before` is the id of the last row of the previous page.
    pub fn list_audit(&self, limit: u32, before: Option<i64>) -> Result<Vec<AuditRecord>, StoreError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT id, ts, actor, action, path, allowed, detail FROM audit_log WHERE id < ?1 ORDER BY id DESC LIMIT ?2")?;
            let rows = stmt
                .query_map(params![before.unwrap_or(i64::MAX), limit], |r| {
                    Ok(AuditRecord {
                        id: r.get(0)?,
                        ts: r.get(1)?,
                        actor: r.get(2)?,
                        action: r.get(3)?,
                        path: r.get(4)?,
                        allowed: r.get::<_, i64>(5)? != 0,
                        detail: r.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Delete audit rows older than `before_ts`. Returns how many were removed.
    pub fn prune_audit(&self, before_ts: i64) -> Result<usize, StoreError> {
        self.with_conn(|c| Ok(c.execute("DELETE FROM audit_log WHERE ts < ?1", [before_ts])?))
    }

    // ---- change sets -----------------------------------------------------------------------

    pub fn insert_changeset(&self, row: &ChangeSetRow) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO changesets (id, title, origin, ops, status, stats, error, created_at, applied_at, undone_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![row.id, row.title, row.origin, row.ops, row.status, row.stats, row.error, row.created_at, row.applied_at, row.undone_at],
            )?;
            Ok(())
        })
    }

    pub fn get_changeset(&self, id: &str) -> Result<Option<ChangeSetRow>, StoreError> {
        self.with_conn(|c| {
            Ok(
                c.query_row("SELECT * FROM changesets WHERE id = ?1", [id], changeset_row)
                    .optional()?,
            )
        })
    }

    /// Newest first.
    pub fn list_changesets(&self, limit: u32) -> Result<Vec<ChangeSetRow>, StoreError> {
        self.with_conn(|c| {
            let mut stmt =
                c.prepare("SELECT * FROM changesets ORDER BY created_at DESC, id DESC LIMIT ?1")?;
            Ok(stmt
                .query_map([limit], changeset_row)?
                .collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Move a change set to a new status, stamping `applied_at`/`undone_at` as appropriate.
    pub fn set_changeset_status(
        &self,
        id: &str,
        status: &str,
        stats: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), StoreError> {
        let now = now_ms();
        self.with_conn(|c| {
            let n = c.execute(
                "UPDATE changesets SET status = ?2, stats = COALESCE(?3, stats), error = ?4,
                   applied_at = CASE WHEN ?2 = 'applied' THEN ?5 ELSE applied_at END,
                   undone_at = CASE WHEN ?2 = 'undone' THEN ?5 ELSE undone_at END
                 WHERE id = ?1",
                params![id, status, stats, error, now],
            )?;
            if n == 0 {
                Err(StoreError::NotFound("change set"))
            } else {
                Ok(())
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(root: &str, level: GrantLevel) -> Grant {
        Grant {
            id: GrantId::new(),
            root: root.into(),
            level,
            recursive: true,
            scope: GrantScope::Persistent,
            expires_at: None,
            label: Some("x".into()),
            created_at: 1,
        }
    }

    #[test]
    fn grants_round_trip_replace_by_root_and_skip_session() {
        let s = Store::open_in_memory().unwrap();
        let a = grant("/a", GrantLevel::Read);
        s.save_grant(&a).unwrap();
        let b = grant("/a", GrantLevel::Full);
        s.save_grant(&b).unwrap();
        s.save_grant(&Grant {
            scope: GrantScope::Session,
            ..grant("/tmp/once", GrantLevel::Read)
        })
        .unwrap();
        let all = s.list_grants().unwrap();
        assert_eq!(all.len(), 1, "one per root, session grants not stored");
        assert_eq!(
            (all[0].level, all[0].label.as_deref()),
            (GrantLevel::Full, Some("x"))
        );
        assert!(s.delete_grant(&b.id).unwrap());
        assert!(!s.delete_grant(&b.id).unwrap());
        assert!(s.list_grants().unwrap().is_empty());
    }

    #[test]
    fn user_deny_rules_are_unique_per_root() {
        let s = Store::open_in_memory().unwrap();
        let a = s.add_user_deny("/data/private").unwrap();
        let b = s.add_user_deny("/data/private").unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(s.list_user_deny().unwrap().len(), 1);
        assert!(s.remove_user_deny("/data/private").unwrap());
        assert!(s.list_user_deny().unwrap().is_empty());
    }

    #[test]
    fn audit_batches_page_newest_first_and_prune() {
        let s = Store::open_in_memory().unwrap();
        let rows: Vec<NewAudit> = (0..5)
            .map(|i| NewAudit {
                ts: i * 100,
                actor: "t".into(),
                action: "read".into(),
                path: format!("/p{i}"),
                allowed: i % 2 == 0,
                detail: None,
            })
            .collect();
        s.insert_audit_batch(&rows).unwrap();
        s.insert_audit_batch(&[]).unwrap();
        let first = s.list_audit(2, None).unwrap();
        assert_eq!(
            first.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
            ["/p4", "/p3"]
        );
        assert!(first[0].allowed && !first[1].allowed);
        let next = s.list_audit(10, Some(first[1].id)).unwrap();
        assert_eq!(next.len(), 3);
        assert_eq!(s.prune_audit(200).unwrap(), 2);
        assert_eq!(s.list_audit(10, None).unwrap().len(), 3);
    }

    #[test]
    fn changeset_lifecycle() {
        let s = Store::open_in_memory().unwrap();
        let row = ChangeSetRow {
            id: "cs1".into(),
            title: "Rename".into(),
            origin: "rename-forge".into(),
            ops: "[]".into(),
            status: "planned".into(),
            stats: None,
            error: None,
            created_at: 5,
            applied_at: None,
            undone_at: None,
        };
        s.insert_changeset(&row).unwrap();
        assert_eq!(s.get_changeset("cs1").unwrap().unwrap(), row);
        s.set_changeset_status("cs1", "applied", Some("{\"files\":3}"), None)
            .unwrap();
        let got = s.get_changeset("cs1").unwrap().unwrap();
        assert_eq!(
            (got.status.as_str(), got.stats.as_deref()),
            ("applied", Some("{\"files\":3}"))
        );
        assert!(got.applied_at.is_some() && got.undone_at.is_none());
        s.set_changeset_status("cs1", "undone", None, None).unwrap();
        let got = s.get_changeset("cs1").unwrap().unwrap();
        assert_eq!(got.stats.as_deref(), Some("{\"files\":3}"), "stats are kept");
        assert!(got.undone_at.is_some());
        assert!(s.set_changeset_status("nope", "applied", None, None).is_err());
        assert!(
            s.set_changeset_status("cs1", "bogus", None, None).is_err(),
            "CHECK constraint"
        );
        assert_eq!(s.list_changesets(10).unwrap().len(), 1);
    }
}
