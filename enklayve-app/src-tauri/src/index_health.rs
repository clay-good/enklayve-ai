//! Search-index integrity checks and repair.
//!
//! The index is derived data (embeddings over stored chunk text), so it can
//! always be rebuilt from `chunks.chunk_text` without the original files. This
//! makes recovery from a corrupt or out-of-sync index safe and self-contained.

use anyhow::Result;
use serde::Serialize;
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize)]
pub struct IndexHealth {
    pub documents: i64,
    pub chunks: i64,
    pub invalid_embeddings: i64,
    pub orphan_chunks: i64,
    pub documents_without_chunks: i64,
    pub healthy: bool,
}

/// Inspect the index for problems without changing anything.
pub fn check_integrity(app_handle: &AppHandle) -> Result<IndexHealth> {
    let conn = crate::database::get_connection(app_handle)?;

    let documents: i64 = conn.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))?;
    let chunks: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
    let orphan_chunks: i64 = conn.query_row(
        "SELECT COUNT(*) FROM chunks WHERE document_id NOT IN (SELECT id FROM documents)",
        [],
        |r| r.get(0),
    )?;
    let documents_without_chunks: i64 = conn.query_row(
        "SELECT COUNT(*) FROM documents WHERE id NOT IN (SELECT DISTINCT document_id FROM chunks)",
        [],
        |r| r.get(0),
    )?;

    // Count chunks whose stored embedding fails to decode.
    let mut invalid_embeddings = 0i64;
    {
        let mut stmt = conn.prepare("SELECT embedding FROM chunks")?;
        let rows = stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))?;
        for row in rows {
            let bytes = row?;
            if crate::embeddings::Embedding::from_bytes(&bytes).is_err() {
                invalid_embeddings += 1;
            }
        }
    }

    let healthy =
        orphan_chunks == 0 && invalid_embeddings == 0 && documents_without_chunks == 0;

    Ok(IndexHealth {
        documents,
        chunks,
        invalid_embeddings,
        orphan_chunks,
        documents_without_chunks,
        healthy,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct RebuildResult {
    pub reembedded: i64,
    pub orphans_removed: i64,
}

/// Repair the index: drop orphan chunks and re-embed every chunk from its stored
/// text. No original files are needed.
pub fn rebuild_index(app_handle: &AppHandle) -> Result<RebuildResult> {
    let conn = crate::database::get_connection(app_handle)?;

    // Remove chunks whose document was deleted.
    let orphans_removed = conn.execute(
        "DELETE FROM chunks WHERE document_id NOT IN (SELECT id FROM documents)",
        [],
    )? as i64;

    // Read all (id, text) first so the statement borrow is released before we
    // start updating rows.
    let rows: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, chunk_text FROM chunks ORDER BY id")?;
        let mapped = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        mapped.collect::<std::result::Result<Vec<_>, _>>()?
    };

    let generator = crate::embeddings::EmbeddingGenerator::new()?;
    let mut reembedded = 0i64;
    for (id, text) in rows {
        let embedding = generator.generate_embedding(&text)?;
        conn.execute(
            "UPDATE chunks SET embedding = ?1 WHERE id = ?2",
            rusqlite::params![embedding.to_bytes(), id],
        )?;
        reembedded += 1;
    }

    Ok(RebuildResult {
        reembedded,
        orphans_removed,
    })
}
