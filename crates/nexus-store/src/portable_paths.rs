//! Non-destructive reference normalization for existing owned report and index files.

use nexus_core::paths::AppPaths;
use rusqlite::params;

use crate::{Store, StoreError};

impl Store {
    /// Relativize only currently resolvable files in their exact owned subtree.
    /// Missing old mount paths remain unchanged for explicit recovery; no suffix guessing.
    pub fn normalize_portable_file_references(&self, paths: &AppPaths) -> Result<usize, StoreError> {
        if paths.portable_root.is_none() {
            return Ok(0);
        }
        self.with_conn(|connection| {
            let transaction = connection.unchecked_transaction()?;
            let mut changed = 0;
            for (table, column, owner) in [
                ("reports", "json_path", paths.reports.clone()),
                ("rename_sessions", "index_path", paths.cache.join("rename")),
            ] {
                // Table/column names are fixed above, never supplied by a caller or database row.
                let rows: Vec<(String, String)> = {
                    let mut statement = transaction.prepare(&format!("SELECT id, {column} FROM {table}"))?;
                    let values = statement
                        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                        .collect::<Result<_, _>>()?;
                    values
                };
                for (id, previous) in rows {
                    if previous.starts_with("dist:") {
                        continue;
                    }
                    let Ok(file) = paths.resolve_storage_reference(&previous, &owner) else {
                        continue;
                    };
                    if !file.is_file() {
                        continue;
                    }
                    let reference = paths.storage_reference(&file)?;
                    changed += transaction.execute(
                        &format!("UPDATE {table} SET {column}=?3 WHERE id=?1 AND {column}=?2"),
                        params![id, previous, reference],
                    )?;
                }
            }
            transaction.commit()?;
            Ok(changed)
        })
    }
}
