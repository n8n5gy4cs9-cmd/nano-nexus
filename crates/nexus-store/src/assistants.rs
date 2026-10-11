//! Persistence for user assistants and per-built-in `enabled`/`sort` overrides.
//! (Built-in definitions come from the binary; the `assistants` table holds only these rows.)

use rusqlite::{Row, params};

use crate::{Store, StoreError, now_ms};

/// One stored row. `source` is `user` (TOML text is the assistant) or `builtin_override`
/// (only `enabled` and `sort` matter).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssistantRow {
    pub id: String,
    pub source: String,
    pub toml: String,
    pub enabled: bool,
    pub sort: i32,
}

fn row(r: &Row<'_>) -> rusqlite::Result<AssistantRow> {
    Ok(AssistantRow {
        id: r.get("id")?,
        source: r.get("source")?,
        toml: r.get("toml")?,
        enabled: r.get::<_, i64>("enabled")? != 0,
        sort: r.get::<_, i64>("sort")? as i32,
    })
}

impl Store {
    /// Every stored assistant row, in `sort` order.
    pub fn list_assistant_rows(&self) -> Result<Vec<AssistantRow>, StoreError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT * FROM assistants ORDER BY sort, id")?;
            Ok(stmt.query_map([], row)?.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Insert or replace a user assistant (the TOML text is the source of truth).
    pub fn save_user_assistant(
        &self,
        id: &str,
        toml: &str,
        enabled: bool,
        sort: i32,
    ) -> Result<(), StoreError> {
        let now = now_ms();
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO assistants (id, source, toml, enabled, sort, created_at, updated_at)
                 VALUES (?1, 'user', ?2, ?3, ?4, ?5, ?5)
                 ON CONFLICT(id) DO UPDATE SET toml = ?2, enabled = ?3, sort = ?4, updated_at = ?5
                 WHERE source = 'user'",
                params![id, toml, i64::from(enabled), sort, now],
            )?;
            Ok(())
        })
    }

    /// Remember `enabled`/`sort` for a built-in.
    pub fn set_builtin_override(&self, id: &str, enabled: bool, sort: i32) -> Result<(), StoreError> {
        let now = now_ms();
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO assistants (id, source, toml, enabled, sort, created_at, updated_at)
                 VALUES (?1, 'builtin_override', '', ?2, ?3, ?4, ?4)
                 ON CONFLICT(id) DO UPDATE SET enabled = ?2, sort = ?3, updated_at = ?4
                 WHERE source = 'builtin_override'",
                params![id, i64::from(enabled), sort, now],
            )?;
            Ok(())
        })
    }

    /// Update only `enabled`/`sort` of a user assistant.
    pub fn set_user_assistant_state(&self, id: &str, enabled: bool, sort: i32) -> Result<bool, StoreError> {
        self.with_conn(|c| {
            let n = c.execute(
                "UPDATE assistants SET enabled = ?2, sort = ?3, updated_at = ?4 WHERE id = ?1 AND source = 'user'",
                params![id, i64::from(enabled), sort, now_ms()],
            )?;
            Ok(n > 0)
        })
    }

    /// Delete a user assistant. `false` if there was none.
    pub fn delete_user_assistant(&self, id: &str) -> Result<bool, StoreError> {
        self.with_conn(|c| {
            Ok(c.execute(
                "DELETE FROM assistants WHERE id = ?1 AND source = 'user'",
                params![id],
            )? > 0)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_rows_round_trip_and_overrides_stay_separate() {
        let s = Store::open_in_memory().unwrap();
        s.save_user_assistant("regex-smith", "toml text", true, 5)
            .unwrap();
        s.save_user_assistant("regex-smith", "toml v2", false, 6).unwrap();
        s.set_builtin_override("tldr", false, 99).unwrap();
        let rows = s.list_assistant_rows().unwrap();
        assert_eq!(rows.len(), 2);
        let user = rows.iter().find(|r| r.id == "regex-smith").unwrap();
        assert_eq!(
            (user.toml.as_str(), user.enabled, user.sort, user.source.as_str()),
            ("toml v2", false, 6, "user")
        );
        let b = rows.iter().find(|r| r.id == "tldr").unwrap();
        assert_eq!(
            (b.source.as_str(), b.enabled, b.sort),
            ("builtin_override", false, 99)
        );
    }

    #[test]
    fn a_user_save_cannot_overwrite_an_override_row() {
        let s = Store::open_in_memory().unwrap();
        s.set_builtin_override("tldr", true, 1).unwrap();
        s.save_user_assistant("tldr", "evil", true, 1).unwrap();
        assert_eq!(s.list_assistant_rows().unwrap()[0].source, "builtin_override");
        assert!(!s.delete_user_assistant("tldr").unwrap());
    }

    #[test]
    fn delete_and_state_updates() {
        let s = Store::open_in_memory().unwrap();
        s.save_user_assistant("a-b", "t", true, 0).unwrap();
        assert!(s.set_user_assistant_state("a-b", false, 3).unwrap());
        assert!(!s.set_user_assistant_state("nope", false, 3).unwrap());
        assert!(s.delete_user_assistant("a-b").unwrap());
        assert!(s.list_assistant_rows().unwrap().is_empty());
    }
}
