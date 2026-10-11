use rusqlite::Connection;

use crate::StoreError;

/// Ordered migrations: index + 1 is the schema version after applying it.
pub const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_init", include_str!("../migrations/0001_init.sql")),
    ("0002_fs", include_str!("../migrations/0002_fs.sql")),
    (
        "0003_models_jobs",
        include_str!("../migrations/0003_models_jobs.sql"),
    ),
    ("0004_reports", include_str!("../migrations/0004_reports.sql")),
    ("0005_share", include_str!("../migrations/0005_share.sql")),
];

/// Current `PRAGMA user_version`.
pub fn schema_version(conn: &Connection) -> Result<u32, StoreError> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

/// Apply every migration newer than the database, each inside its own transaction.
pub(crate) fn run(conn: &mut Connection) -> Result<(), StoreError> {
    let current = schema_version(conn)?;
    let known = MIGRATIONS.len() as u32;
    if current > known {
        return Err(StoreError::TooNew {
            found: current,
            known,
        });
    }
    for (i, (name, sql)) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let version = i as u32 + 1;
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
        tracing::info!(migration = name, version, "applied migration");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_in_order_and_are_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), MIGRATIONS.len() as u32);
        run(&mut conn).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), MIGRATIONS.len() as u32);
    }

    #[test]
    fn newer_database_is_refused() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
        assert!(matches!(
            run(&mut conn),
            Err(StoreError::TooNew { found: 999, .. })
        ));
    }

    #[test]
    fn migration_names_are_numbered_sequentially() {
        for (i, (name, _)) in MIGRATIONS.iter().enumerate() {
            assert!(name.starts_with(&format!("{:04}_", i + 1)), "{name}");
        }
    }
}
