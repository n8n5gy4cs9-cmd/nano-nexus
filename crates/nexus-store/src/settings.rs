//! Settings persistence: one JSON value per dotted key (DATA-AND-IPC §1).

use nexus_core::settings::{Settings, SettingsPatch};
use rusqlite::params;
use serde_json::Value;

use crate::{Store, StoreError};

impl Store {
    /// Load all settings: defaults overlaid with whatever is stored. Rows that no longer parse
    /// (renamed key, old value shape) are skipped with a warning instead of failing startup.
    pub fn load_settings(&self) -> Result<Settings, StoreError> {
        let rows: Vec<(String, String)> = self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT key, value FROM settings")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;
        let mut settings = Settings::default();
        for (key, raw) in rows {
            let applied = serde_json::from_str::<Value>(&raw)
                .map_err(|e| e.to_string())
                .and_then(|v| settings.set_key(&key, v).map_err(|e| e.to_string()));
            if let Err(reason) = applied {
                tracing::warn!(%key, %reason, "ignoring unreadable stored setting");
            }
        }
        Ok(settings)
    }

    /// Validate `value` against the typed `Settings`, persist it, and return the updated settings.
    pub fn set_setting(&self, current: &Settings, key: &str, value: Value) -> Result<Settings, StoreError> {
        self.set_settings(current, &SettingsPatch::from([(key.to_owned(), value)]))
    }

    /// Apply a patch atomically (all keys valid or nothing changes), persist it, return the result.
    pub fn set_settings(&self, current: &Settings, patch: &SettingsPatch) -> Result<Settings, StoreError> {
        let mut next = current.clone();
        next.apply(patch).map_err(|e| StoreError::Invalid {
            key: patch.keys().cloned().collect::<Vec<_>>().join(", "),
            reason: e.to_string(),
        })?;
        self.with_conn(|c| {
            let tx = c.transaction()?;
            for (key, value) in patch {
                tx.execute(
                    "INSERT INTO settings (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![key, value.to_string()],
                )?;
            }
            tx.commit()?;
            Ok(())
        })?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_store_yields_defaults() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.load_settings().unwrap(), Settings::default());
    }

    #[test]
    fn values_persist_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/nexus.db");
        {
            let s = Store::open(&path).unwrap();
            let cur = s.load_settings().unwrap();
            let next = s.set_setting(&cur, "ui.theme", json!("light")).unwrap();
            assert_eq!(next.ui_theme, nexus_core::settings::Theme::Light);
            s.set_setting(&next, "runtime.threads", json!(4)).unwrap();
        }
        let s = Store::open(&path).unwrap();
        let loaded = s.load_settings().unwrap();
        assert_eq!(loaded.ui_theme, nexus_core::settings::Theme::Light);
        assert_eq!(loaded.runtime_threads, Some(4));
    }

    #[test]
    fn invalid_values_are_rejected_and_not_stored() {
        let s = Store::open_in_memory().unwrap();
        let cur = s.load_settings().unwrap();
        assert!(s.set_setting(&cur, "ui.theme", json!("neon")).is_err());
        assert!(s.set_setting(&cur, "nope", json!(1)).is_err());
        let patch = SettingsPatch::from([
            ("share.port".to_owned(), json!(1234)),
            ("share.https".to_owned(), json!("x")),
        ]);
        assert!(s.set_settings(&cur, &patch).is_err());
        assert_eq!(s.load_settings().unwrap(), Settings::default());
    }

    #[test]
    fn garbage_rows_are_skipped_on_load() {
        let s = Store::open_in_memory().unwrap();
        s.with_conn(|c| {
            c.execute("INSERT INTO settings VALUES ('ui.theme', '\"dark\"'), ('old.key', '1'), ('share.port', 'not json')", [])?;
            Ok(())
        })
        .unwrap();
        let loaded = s.load_settings().unwrap();
        assert_eq!(loaded.share_port, 9889);
    }

    #[test]
    fn pragmas_are_applied() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("x.db")).unwrap();
        s.with_conn(|c| {
            let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
            let fk: i64 = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
            assert_eq!(mode, "wal");
            assert_eq!(fk, 1);
            Ok(())
        })
        .unwrap();
    }
}
