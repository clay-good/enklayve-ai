use anyhow::Result;
use rusqlite::Connection;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Initialize the SQLite database with required tables
pub async fn init_database(app_handle: &AppHandle) -> Result<()> {
    let db_path = get_database_path(app_handle)?;

    // Ensure parent directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(&db_path)?;

    // Crash-resilient, concurrent-friendly connection settings.
    apply_pragmas(&conn)?;

    // Create documents table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS documents (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_name TEXT NOT NULL,
            file_path TEXT NOT NULL,
            file_type TEXT NOT NULL,
            upload_date INTEGER NOT NULL,
            size_bytes INTEGER NOT NULL,
            title TEXT,
            author TEXT,
            creation_date INTEGER,
            page_count INTEGER,
            word_count INTEGER,
            conversation_id INTEGER
        )",
        [],
    )?;

    let mut stmt = conn.prepare("PRAGMA table_info(documents)")?;
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;

    if !columns.contains(&"title".to_string()) {
        conn.execute("ALTER TABLE documents ADD COLUMN title TEXT", [])?;
    }
    if !columns.contains(&"author".to_string()) {
        conn.execute("ALTER TABLE documents ADD COLUMN author TEXT", [])?;
    }
    if !columns.contains(&"creation_date".to_string()) {
        conn.execute("ALTER TABLE documents ADD COLUMN creation_date INTEGER", [])?;
    }
    if !columns.contains(&"page_count".to_string()) {
        conn.execute("ALTER TABLE documents ADD COLUMN page_count INTEGER", [])?;
    }
    if !columns.contains(&"word_count".to_string()) {
        conn.execute("ALTER TABLE documents ADD COLUMN word_count INTEGER", [])?;
    }
    if !columns.contains(&"conversation_id".to_string()) {
        conn.execute("ALTER TABLE documents ADD COLUMN conversation_id INTEGER", [])?;
    }

    // Create chunks table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS chunks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            document_id INTEGER NOT NULL,
            chunk_text TEXT NOT NULL,
            chunk_index INTEGER NOT NULL,
            page_number INTEGER,
            embedding BLOB,
            FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE CASCADE
        )",
        [],
    )?;

    // Create index for faster chunk lookups
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_chunks_document
         ON chunks(document_id)",
        [],
    )?;

    // Index documents by the conversation they belong to (per-chat scoping)
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_documents_conversation
         ON documents(conversation_id)",
        [],
    )?;

    // Create FTS5 virtual table for full-text search
    conn.execute(
        "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
            chunk_text,
            document_id UNINDEXED,
            content='chunks',
            content_rowid='id'
        )",
        [],
    )?;

    // Create triggers to keep FTS5 table synchronized with chunks table
    conn.execute(
        "CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
            INSERT INTO chunks_fts(rowid, chunk_text, document_id)
            VALUES (new.id, new.chunk_text, new.document_id);
        END",
        [],
    )?;

    conn.execute(
        "CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
            DELETE FROM chunks_fts WHERE rowid = old.id;
        END",
        [],
    )?;

    conn.execute(
        "CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
            UPDATE chunks_fts SET chunk_text = new.chunk_text, document_id = new.document_id
            WHERE rowid = new.id;
        END",
        [],
    )?;

    // Populate FTS5 table for existing chunks if not already populated
    let fts_count: i64 = conn.query_row("SELECT COUNT(*) FROM chunks_fts", [], |row| row.get(0))?;
    let chunks_count: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;

    if fts_count == 0 && chunks_count > 0 {
        conn.execute(
            "INSERT INTO chunks_fts(rowid, chunk_text, document_id)
             SELECT id, chunk_text, document_id FROM chunks",
            [],
        )?;
    }

    // Create models table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS downloaded_models (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            model_name TEXT NOT NULL UNIQUE,
            file_path TEXT NOT NULL,
            download_date INTEGER NOT NULL,
            size_bytes INTEGER NOT NULL,
            checksum TEXT NOT NULL,
            verified INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )?;

    // Initialize conversation tables
    crate::conversations::init_conversation_tables(&conn)?;

    // Initialize settings table
    crate::settings::init_settings_table(&conn)?;

    // Initialize encryption database support
    crate::encrypted_database::initialize_encryption_support(&conn)?;

    // Initialize onboarding table
    crate::onboarding::init_onboarding_table(&conn)?;

    println!("Database initialized at: {}", db_path.display());

    Ok(())
}

/// Get the path to the database file
pub fn get_database_path(app_handle: &AppHandle) -> Result<PathBuf> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get app data directory: {}", e))?;

    Ok(app_data_dir.join("enklayve.db"))
}

/// Get a database connection
/// Connection settings applied to every SQLite connection: foreign keys, plus
/// WAL journaling for crash-safe atomic commits and better read/write concurrency,
/// and a busy timeout so brief lock contention retries instead of failing.
fn apply_pragmas(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(())
}

pub fn get_connection(app_handle: &AppHandle) -> Result<Connection> {
    let db_path = get_database_path(app_handle)?;
    let conn = Connection::open(db_path)?;
    apply_pragmas(&conn)?;
    Ok(conn)
}

#[cfg(test)]
mod pragma_tests {
    use super::*;

    #[test]
    fn wal_mode_is_enabled() {
        let dir = std::env::temp_dir().join(format!(
            "enklayve_wal_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("test.db");
        let conn = Connection::open(&db).unwrap();
        apply_pragmas(&conn).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
