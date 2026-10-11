//! Persistence for installed models and the download queue.

use rusqlite::{OptionalExtension, Row, params};

use crate::{Store, StoreError, now_ms};

/// An installed model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRow {
    pub id: String,
    /// `builtin | download | import | discovered`
    pub source: String,
    pub name: String,
    pub repo: Option<String>,
    pub revision: Option<String>,
    pub file: Option<String>,
    /// Absolute path of the `.gguf`.
    pub path: String,
    pub sha256: Option<String>,
    pub bytes: u64,
    pub quant: Option<String>,
    pub catalog_id: Option<String>,
    /// JSON of the parsed GGUF header.
    pub gguf_meta: Option<String>,
    pub license: Option<String>,
    /// JSON: sampling presets, capabilities, thinking support.
    pub extra: Option<String>,
    pub added_at: i64,
}

fn model_row(r: &Row<'_>) -> rusqlite::Result<ModelRow> {
    Ok(ModelRow {
        id: r.get("id")?,
        source: r.get("source")?,
        name: r.get("name")?,
        repo: r.get("repo")?,
        revision: r.get("revision")?,
        file: r.get("file")?,
        path: r.get("path")?,
        sha256: r.get("sha256")?,
        bytes: r.get::<_, i64>("bytes")?.max(0) as u64,
        quant: r.get("quant")?,
        catalog_id: r.get("catalog_id")?,
        gguf_meta: r.get("gguf_meta")?,
        license: r.get("license")?,
        extra: r.get("extra")?,
        added_at: r.get("added_at")?,
    })
}

/// A queued, running, or finished download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadRow {
    pub id: String,
    pub catalog_id: Option<String>,
    pub repo: String,
    pub revision: String,
    pub file: String,
    pub dest: String,
    pub sha256: String,
    pub bytes: u64,
    pub done_bytes: u64,
    /// `queued | running | paused | failed | done | cancelled`
    pub status: String,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

fn download_row(r: &Row<'_>) -> rusqlite::Result<DownloadRow> {
    Ok(DownloadRow {
        id: r.get("id")?,
        catalog_id: r.get("catalog_id")?,
        repo: r.get("repo")?,
        revision: r.get("revision")?,
        file: r.get("file")?,
        dest: r.get("dest")?,
        sha256: r.get("sha256")?,
        bytes: r.get::<_, i64>("bytes")?.max(0) as u64,
        done_bytes: r.get::<_, i64>("done_bytes")?.max(0) as u64,
        status: r.get("status")?,
        error: r.get("error")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

impl Store {
    // ---- models ---------------------------------------------------------------------------

    pub fn list_models(&self) -> Result<Vec<ModelRow>, StoreError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT * FROM models ORDER BY added_at, id")?;
            Ok(stmt.query_map([], model_row)?.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn get_model(&self, id: &str) -> Result<Option<ModelRow>, StoreError> {
        self.with_conn(|c| {
            Ok(
                c.query_row("SELECT * FROM models WHERE id = ?1", params![id], model_row)
                    .optional()?,
            )
        })
    }

    /// Insert or replace (by id).
    pub fn upsert_model(&self, m: &ModelRow) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO models (id, source, name, repo, revision, file, path, sha256, bytes, quant, catalog_id, gguf_meta, license, extra, added_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
                 ON CONFLICT(id) DO UPDATE SET source=?2, name=?3, repo=?4, revision=?5, file=?6, path=?7, sha256=?8, bytes=?9,
                   quant=?10, catalog_id=?11, gguf_meta=?12, license=?13, extra=?14",
                params![m.id, m.source, m.name, m.repo, m.revision, m.file, m.path, m.sha256, m.bytes as i64, m.quant, m.catalog_id, m.gguf_meta, m.license, m.extra, m.added_at],
            )?;
            Ok(())
        })
    }

    pub fn delete_model(&self, id: &str) -> Result<bool, StoreError> {
        self.with_conn(|c| Ok(c.execute("DELETE FROM models WHERE id = ?1", params![id])? > 0))
    }

    /// Remove `discovered` rows whose id is not in `keep` (a folder was unlinked or files vanished).
    pub fn prune_discovered(&self, keep: &[String]) -> Result<usize, StoreError> {
        self.with_conn(|c| {
            let ids: Vec<String> = {
                let mut stmt = c.prepare("SELECT id FROM models WHERE source = 'discovered'")?;
                stmt.query_map([], |r| r.get::<_, String>(0))?
                    .collect::<Result<_, _>>()?
            };
            let mut n = 0;
            for id in ids.into_iter().filter(|i| !keep.contains(i)) {
                n += c.execute("DELETE FROM models WHERE id = ?1", params![id])?;
            }
            Ok(n)
        })
    }

    // ---- downloads ------------------------------------------------------------------------

    pub fn insert_download(&self, d: &DownloadRow) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO downloads (id, catalog_id, repo, revision, file, dest, sha256, bytes, done_bytes, status, error, created_at, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                params![d.id, d.catalog_id, d.repo, d.revision, d.file, d.dest, d.sha256, d.bytes as i64, d.done_bytes as i64, d.status, d.error, d.created_at, d.updated_at],
            )?;
            Ok(())
        })
    }

    pub fn get_download(&self, id: &str) -> Result<Option<DownloadRow>, StoreError> {
        self.with_conn(|c| {
            Ok(
                c.query_row("SELECT * FROM downloads WHERE id = ?1", params![id], download_row)
                    .optional()?,
            )
        })
    }

    /// Newest first.
    pub fn list_downloads(&self) -> Result<Vec<DownloadRow>, StoreError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT * FROM downloads ORDER BY created_at DESC, id DESC")?;
            Ok(stmt.query_map([], download_row)?.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// The oldest queued download, if any.
    pub fn next_queued_download(&self) -> Result<Option<DownloadRow>, StoreError> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT * FROM downloads WHERE status = 'queued' ORDER BY created_at, id LIMIT 1",
                [],
                download_row,
            )
            .optional()?)
        })
    }

    pub fn set_download_status(&self, id: &str, status: &str, error: Option<&str>) -> Result<(), StoreError> {
        self.with_conn(|c| {
            let n = c.execute(
                "UPDATE downloads SET status = ?2, error = ?3, updated_at = ?4 WHERE id = ?1",
                params![id, status, error, now_ms()],
            )?;
            if n == 0 {
                Err(StoreError::NotFound("download"))
            } else {
                Ok(())
            }
        })
    }

    pub fn set_download_progress(&self, id: &str, done_bytes: u64) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE downloads SET done_bytes = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, done_bytes as i64, now_ms()],
            )?;
            Ok(())
        })
    }

    pub fn delete_download(&self, id: &str) -> Result<bool, StoreError> {
        self.with_conn(|c| Ok(c.execute("DELETE FROM downloads WHERE id = ?1", params![id])? > 0))
    }

    /// Startup: downloads that were `running` when the app quit become `paused` (never auto-resume
    /// metered downloads). Returns how many.
    pub fn pause_interrupted_downloads(&self) -> Result<usize, StoreError> {
        self.with_conn(|c| {
            Ok(c.execute(
                "UPDATE downloads SET status = 'paused', updated_at = ?1 WHERE status = 'running'",
                params![now_ms()],
            )?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, source: &str) -> ModelRow {
        ModelRow {
            id: id.into(),
            source: source.into(),
            name: id.into(),
            repo: Some("o/r".into()),
            revision: None,
            file: None,
            path: format!("/m/{id}.gguf"),
            sha256: None,
            bytes: 10,
            quant: Some("Q8_0".into()),
            catalog_id: None,
            gguf_meta: None,
            license: None,
            extra: None,
            added_at: 1,
        }
    }

    fn dl(id: &str, status: &str, created: i64) -> DownloadRow {
        DownloadRow {
            id: id.into(),
            catalog_id: Some("c".into()),
            repo: "o/r".into(),
            revision: "sha".into(),
            file: "f.gguf".into(),
            dest: "/d/f.gguf".into(),
            sha256: "ab".into(),
            bytes: 100,
            done_bytes: 0,
            status: status.into(),
            error: None,
            created_at: created,
            updated_at: created,
        }
    }

    #[test]
    fn models_upsert_list_delete() {
        let s = Store::open_in_memory().unwrap();
        s.upsert_model(&model("a", "download")).unwrap();
        s.upsert_model(&model("b", "builtin")).unwrap();
        let mut a2 = model("a", "download");
        a2.name = "renamed".into();
        s.upsert_model(&a2).unwrap();
        assert_eq!(s.list_models().unwrap().len(), 2);
        assert_eq!(s.get_model("a").unwrap().unwrap().name, "renamed");
        assert!(s.delete_model("a").unwrap() && !s.delete_model("a").unwrap());
        assert!(s.get_model("a").unwrap().is_none());
    }

    #[test]
    fn the_source_check_rejects_unknown_values() {
        let s = Store::open_in_memory().unwrap();
        assert!(s.upsert_model(&model("x", "mystery")).is_err());
    }

    #[test]
    fn discovered_rows_are_pruned_but_others_stay() {
        let s = Store::open_in_memory().unwrap();
        for (id, src) in [("d1", "discovered"), ("d2", "discovered"), ("b", "builtin")] {
            s.upsert_model(&model(id, src)).unwrap();
        }
        assert_eq!(s.prune_discovered(&["d1".to_string()]).unwrap(), 1);
        let ids: Vec<String> = s.list_models().unwrap().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, vec!["b", "d1"]);
    }

    #[test]
    fn download_queue_and_restart_behavior() {
        let s = Store::open_in_memory().unwrap();
        s.insert_download(&dl("d1", "running", 1)).unwrap();
        s.insert_download(&dl("d3", "queued", 3)).unwrap();
        s.insert_download(&dl("d2", "queued", 2)).unwrap();
        assert_eq!(s.next_queued_download().unwrap().unwrap().id, "d2");
        s.set_download_progress("d1", 40).unwrap();
        assert_eq!(s.pause_interrupted_downloads().unwrap(), 1);
        let d1 = s.get_download("d1").unwrap().unwrap();
        assert_eq!((d1.status.as_str(), d1.done_bytes), ("paused", 40));
        s.set_download_status("d2", "failed", Some("checksum")).unwrap();
        assert_eq!(
            s.get_download("d2").unwrap().unwrap().error.as_deref(),
            Some("checksum")
        );
        assert_eq!(s.next_queued_download().unwrap().unwrap().id, "d3");
        assert_eq!(s.list_downloads().unwrap()[0].id, "d3");
        assert!(matches!(
            s.set_download_status("nope", "done", None),
            Err(StoreError::NotFound(_))
        ));
        assert!(s.delete_download("d3").unwrap());
    }
}
