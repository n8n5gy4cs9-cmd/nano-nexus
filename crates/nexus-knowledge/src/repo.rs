//! Library, document, and chunk repositories over the shared [`Store`] (tables from `0006_knowledge.sql`).

use nexus_store::Store;
use rusqlite::{OptionalExtension, Row, params};

use crate::chunk::ChunkOut;
use crate::types::{DocumentRow, KbError, Library, LibrarySource};

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

const LIB_COLS: &str =
    "id, name, source, path, mode, enabled, doc_count, chunk_count, bytes, indexed_at, created_at";

fn library(r: &Row<'_>) -> rusqlite::Result<Library> {
    Ok(Library {
        id: r.get(0)?,
        name: r.get(1)?,
        source: LibrarySource::parse(&r.get::<_, String>(2)?),
        path: r.get(3)?,
        mode: r.get(4)?,
        enabled: r.get::<_, i64>(5)? != 0,
        doc_count: r.get(6)?,
        chunk_count: r.get(7)?,
        bytes: r.get(8)?,
        indexed_at: r.get(9)?,
        created_at: r.get(10)?,
    })
}

const DOC_COLS: &str = "id, library_id, path, title, hash, kind, pages, bytes, status, note";

fn document(r: &Row<'_>) -> rusqlite::Result<DocumentRow> {
    Ok(DocumentRow {
        id: r.get(0)?,
        library_id: r.get(1)?,
        path: r.get(2)?,
        title: r.get(3)?,
        hash: r.get(4)?,
        kind: r.get(5)?,
        pages: r.get(6)?,
        bytes: r.get(7)?,
        status: r.get(8)?,
        note: r.get(9)?,
    })
}

/// New document to store together with its chunks.
pub struct NewDocument {
    pub path: String,
    pub title: Option<String>,
    pub hash: String,
    pub kind: String,
    pub pages: i64,
    pub bytes: i64,
    /// `indexed | skipped | failed`
    pub status: String,
    pub note: Option<String>,
}

/// Create a library row (no documents yet).
pub fn create_library(
    store: &Store,
    name: &str,
    source: LibrarySource,
    path: &str,
    mode: &str,
) -> Result<Library, KbError> {
    if name.trim().is_empty() {
        return Err(KbError::Invalid("a library needs a name".into()));
    }
    if !matches!(mode, "copy" | "reference") {
        return Err(KbError::Invalid(format!("unknown library mode `{mode}`")));
    }
    let id = uuid::Uuid::now_v7().to_string();
    let now = now_ms();
    store.with_conn(|c| {
        c.execute(
            "INSERT INTO kb_libraries (id, name, source, path, mode, enabled, created_at) VALUES (?1,?2,?3,?4,?5,1,?6)",
            params![id, name.trim(), source.as_str(), path, mode, now],
        )?;
        Ok(())
    })?;
    get_library(store, &id)
}

/// One library by id.
pub fn get_library(store: &Store, id: &str) -> Result<Library, KbError> {
    let found = store.with_conn(|c| {
        Ok(c.query_row(
            &format!("SELECT {LIB_COLS} FROM kb_libraries WHERE id = ?1"),
            [id],
            library,
        )
        .optional()?)
    })?;
    found.ok_or(KbError::NotFound("library"))
}

/// All libraries, newest last.
pub fn list_libraries(store: &Store) -> Result<Vec<Library>, KbError> {
    Ok(store.with_conn(|c| {
        let mut st = c.prepare(&format!(
            "SELECT {LIB_COLS} FROM kb_libraries ORDER BY created_at, id"
        ))?;
        let rows = st.query_map([], library)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?)
}

/// Enable or disable a library (disabled libraries are invisible to search and chat).
pub fn set_enabled(store: &Store, id: &str, enabled: bool) -> Result<(), KbError> {
    let n = store.with_conn(|c| {
        Ok(c.execute(
            "UPDATE kb_libraries SET enabled = ?2 WHERE id = ?1",
            params![id, enabled as i64],
        )?)
    })?;
    if n == 0 {
        Err(KbError::NotFound("library"))
    } else {
        Ok(())
    }
}

/// Rename a library.
pub fn rename_library(store: &Store, id: &str, name: &str) -> Result<(), KbError> {
    if name.trim().is_empty() {
        return Err(KbError::Invalid("a library needs a name".into()));
    }
    let n = store.with_conn(|c| {
        Ok(c.execute(
            "UPDATE kb_libraries SET name = ?2 WHERE id = ?1",
            params![id, name.trim()],
        )?)
    })?;
    if n == 0 {
        Err(KbError::NotFound("library"))
    } else {
        Ok(())
    }
}

/// Remove a library with its documents and chunks (never touches files on disk).
pub fn remove_library(store: &Store, id: &str) -> Result<(), KbError> {
    store.with_conn(|c| {
        let tx = c.transaction()?;
        // Chunks first so the FTS delete trigger runs for every row.
        tx.execute(
            "DELETE FROM kb_chunks WHERE document_id IN (SELECT id FROM kb_documents WHERE library_id = ?1)",
            [id],
        )?;
        tx.execute("DELETE FROM kb_documents WHERE library_id = ?1", [id])?;
        tx.execute("DELETE FROM kb_libraries WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    })?;
    Ok(())
}

/// Documents of a library, by path.
pub fn list_documents(store: &Store, library_id: &str) -> Result<Vec<DocumentRow>, KbError> {
    Ok(store.with_conn(|c| {
        let mut st = c.prepare(&format!(
            "SELECT {DOC_COLS} FROM kb_documents WHERE library_id = ?1 ORDER BY path"
        ))?;
        let rows = st
            .query_map([library_id], document)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?)
}

/// Insert or replace a document and its chunks atomically (old chunks are removed first).
pub fn replace_document(
    store: &Store,
    library_id: &str,
    doc: &NewDocument,
    chunks: &[ChunkOut],
) -> Result<i64, KbError> {
    Ok(store.with_conn(|c| {
        let tx = c.transaction()?;
        let old: Option<i64> = tx
            .query_row(
                "SELECT id FROM kb_documents WHERE library_id = ?1 AND path = ?2",
                params![library_id, doc.path],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = old {
            tx.execute("DELETE FROM kb_chunks WHERE document_id = ?1", [old])?;
            tx.execute("DELETE FROM kb_documents WHERE id = ?1", [old])?;
        }
        tx.execute(
            "INSERT INTO kb_documents (library_id, path, title, hash, kind, pages, bytes, status, note)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                library_id, doc.path, doc.title, doc.hash, doc.kind, doc.pages, doc.bytes, doc.status, doc.note
            ],
        )?;
        let id = tx.last_insert_rowid();
        {
            let mut ins = tx.prepare(
                "INSERT INTO kb_chunks (document_id, ord, heading_path, page, text, tokens) VALUES (?1,?2,?3,?4,?5,?6)",
            )?;
            for ch in chunks {
                ins.execute(params![id, ch.ord, ch.heading_path, ch.page, ch.text, ch.tokens])?;
            }
        }
        tx.commit()?;
        Ok(id)
    })?)
}

/// Remove one document (and its chunks) by relative path.
pub fn remove_document(store: &Store, library_id: &str, path: &str) -> Result<(), KbError> {
    store.with_conn(|c| {
        let tx = c.transaction()?;
        tx.execute(
            "DELETE FROM kb_chunks WHERE document_id IN (SELECT id FROM kb_documents WHERE library_id = ?1 AND path = ?2)",
            params![library_id, path],
        )?;
        tx.execute(
            "DELETE FROM kb_documents WHERE library_id = ?1 AND path = ?2",
            params![library_id, path],
        )?;
        tx.commit()?;
        Ok(())
    })?;
    Ok(())
}

/// Recompute the library counters after an index run and stamp `indexed_at`.
pub fn refresh_counts(store: &Store, library_id: &str) -> Result<(), KbError> {
    let now = now_ms();
    store.with_conn(|c| {
        c.execute(
            "UPDATE kb_libraries SET
               doc_count = (SELECT COUNT(*) FROM kb_documents WHERE library_id = ?1 AND status = 'indexed'),
               chunk_count = (SELECT COUNT(*) FROM kb_chunks k JOIN kb_documents d ON d.id = k.document_id WHERE d.library_id = ?1),
               bytes = (SELECT COALESCE(SUM(bytes), 0) FROM kb_documents WHERE library_id = ?1),
               indexed_at = ?2
             WHERE id = ?1",
            params![library_id, now],
        )?;
        Ok(())
    })?;
    Ok(())
}

/// One chunk by id, with its citation fields (for "open passage").
pub fn get_passage(store: &Store, chunk_id: i64) -> Result<crate::types::Passage, KbError> {
    let found = store.with_conn(|c| {
        Ok(c.query_row(
            "SELECT c.id, c.document_id, l.id, l.name, COALESCE(d.title, d.path), d.path, c.heading_path, c.page, c.text, c.tokens
             FROM kb_chunks c JOIN kb_documents d ON d.id = c.document_id JOIN kb_libraries l ON l.id = d.library_id
             WHERE c.id = ?1",
            [chunk_id],
            |r| {
                Ok(crate::types::Passage {
                    chunk_id: r.get(0)?,
                    document_id: r.get(1)?,
                    library_id: r.get(2)?,
                    library_name: r.get(3)?,
                    document: r.get(4)?,
                    doc_path: r.get(5)?,
                    heading_path: r.get(6)?,
                    page: r.get::<_, Option<i64>>(7)?.map(|p| p as u32),
                    text: r.get(8)?,
                    tokens: r.get::<_, i64>(9)? as u32,
                    score: 0.0,
                })
            },
        )
        .optional()?)
    })?;
    found.ok_or(KbError::NotFound("passage"))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn chunk(ord: u32, path: &str, text: &str) -> ChunkOut {
        ChunkOut {
            ord,
            heading_path: path.into(),
            page: Some(ord + 1),
            text: text.into(),
            tokens: 10,
        }
    }

    fn doc(path: &str, hash: &str) -> NewDocument {
        NewDocument {
            path: path.into(),
            title: Some("T".into()),
            hash: hash.into(),
            kind: "markdown".into(),
            pages: 1,
            bytes: 10,
            status: "indexed".into(),
            note: None,
        }
    }

    #[test]
    fn library_lifecycle() {
        let s = Store::open_in_memory().unwrap();
        let l = create_library(&s, " Reaper ", LibrarySource::User, "/x", "copy").unwrap();
        assert_eq!(l.name, "Reaper");
        assert!(l.enabled);
        set_enabled(&s, &l.id, false).unwrap();
        assert!(!get_library(&s, &l.id).unwrap().enabled);
        rename_library(&s, &l.id, "Reaper manual").unwrap();
        assert_eq!(list_libraries(&s).unwrap()[0].name, "Reaper manual");
        assert!(matches!(set_enabled(&s, "nope", true), Err(KbError::NotFound(_))));
        assert!(create_library(&s, "", LibrarySource::User, "/x", "copy").is_err());
        assert!(create_library(&s, "a", LibrarySource::User, "/x", "weird").is_err());
    }

    #[test]
    fn replace_document_swaps_chunks_and_fts() {
        let s = Store::open_in_memory().unwrap();
        let l = create_library(&s, "L", LibrarySource::User, "/x", "copy").unwrap();
        replace_document(
            &s,
            &l.id,
            &doc("a.md", "h1"),
            &[chunk(0, "T", "sidechain compression")],
        )
        .unwrap();
        replace_document(&s, &l.id, &doc("a.md", "h2"), &[chunk(0, "T", "limiter ceiling")]).unwrap();
        refresh_counts(&s, &l.id).unwrap();
        let l = get_library(&s, &l.id).unwrap();
        assert_eq!((l.doc_count, l.chunk_count), (1, 1));
        assert!(l.indexed_at.is_some());
        let hits = |q: &str| -> i64 {
            s.with_conn(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM kb_chunks_fts WHERE kb_chunks_fts MATCH ?1",
                    [q],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
        };
        assert_eq!(hits("sidechain"), 0);
        assert_eq!(hits("limiter"), 1);
    }

    #[test]
    fn remove_library_cleans_everything() {
        let s = Store::open_in_memory().unwrap();
        let l = create_library(&s, "L", LibrarySource::User, "/x", "copy").unwrap();
        replace_document(
            &s,
            &l.id,
            &doc("a.md", "h"),
            &[chunk(0, "T", "alpha"), chunk(1, "T", "beta")],
        )
        .unwrap();
        remove_library(&s, &l.id).unwrap();
        let n: i64 = s
            .with_conn(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM kb_chunks_fts WHERE kb_chunks_fts MATCH 'alpha'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(n, 0);
        assert!(list_libraries(&s).unwrap().is_empty());
    }

    #[test]
    fn passage_lookup_has_citation_fields() {
        let s = Store::open_in_memory().unwrap();
        let l = create_library(&s, "L", LibrarySource::User, "/x", "copy").unwrap();
        replace_document(&s, &l.id, &doc("a.md", "h"), &[chunk(0, "T > Sends", "alpha")]).unwrap();
        let id: i64 = s
            .with_conn(|c| Ok(c.query_row("SELECT id FROM kb_chunks LIMIT 1", [], |r| r.get(0))?))
            .unwrap();
        let p = get_passage(&s, id).unwrap();
        assert_eq!(
            (p.heading_path.as_str(), p.page, p.library_name.as_str()),
            ("T > Sends", Some(1), "L")
        );
        assert!(matches!(get_passage(&s, 9999), Err(KbError::NotFound(_))));
    }
}
