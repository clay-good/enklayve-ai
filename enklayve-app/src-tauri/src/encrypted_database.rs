//! Legacy per-message encryption (app versions up to 0.1.7).
//!
//! Those versions encrypted existing messages once when security was turned
//! on, then showed them as a placeholder forever and stored new data in
//! plaintext. Whole-database encryption (`vault`) replaces that. This module
//! keeps the columns those versions added and decrypts their messages back so
//! they can be moved into the encrypted vault.

use anyhow::Result;
use rusqlite::Connection;
use crate::encryption::{EncryptionKey, decrypt};

/// Add the legacy encryption columns if missing (queries still select them).
pub fn initialize_encryption_support(conn: &Connection) -> Result<()> {
    let has = |table: &str, col: &str| -> Result<bool> {
        let n: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM pragma_table_info('{}') WHERE name = ?1", table),
            [col],
            |row| row.get(0),
        )?;
        Ok(n > 0)
    };

    if !has("messages", "is_encrypted")? {
        conn.execute("ALTER TABLE messages ADD COLUMN is_encrypted BOOLEAN NOT NULL DEFAULT 0", [])?;
    }
    if !has("messages", "encrypted_content")? {
        conn.execute("ALTER TABLE messages ADD COLUMN encrypted_content BLOB", [])?;
    }
    if !has("chunks", "is_encrypted")? {
        conn.execute("ALTER TABLE chunks ADD COLUMN is_encrypted BOOLEAN NOT NULL DEFAULT 0", [])?;
    }
    Ok(())
}

/// Decrypt messages encrypted by the legacy scheme back into `content`.
/// Returns how many were restored.
pub fn restore_legacy_messages(conn: &Connection, key: &EncryptionKey) -> Result<usize> {
    let rows: Vec<(i64, Vec<u8>)> = conn
        .prepare("SELECT id, encrypted_content FROM messages WHERE is_encrypted = 1 AND encrypted_content IS NOT NULL")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;

    let mut restored = 0;
    for (id, blob) in rows {
        let text = String::from_utf8(decrypt(&blob, key)?)?;
        conn.execute(
            "UPDATE messages SET content = ?1, encrypted_content = NULL, is_encrypted = 0 WHERE id = ?2",
            rusqlite::params![text, id],
        )?;
        restored += 1;
    }
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encryption::encrypt;

    #[test]
    fn legacy_messages_are_restored() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE messages (id INTEGER PRIMARY KEY, content TEXT NOT NULL);
             CREATE TABLE chunks (id INTEGER PRIMARY KEY);",
        )
        .unwrap();
        initialize_encryption_support(&conn).unwrap();
        initialize_encryption_support(&conn).unwrap(); // idempotent

        let salt = EncryptionKey::generate_salt();
        let key = EncryptionKey::from_password("pw", &salt).unwrap();
        let blob = encrypt(b"an old secret", &key).unwrap();
        conn.execute(
            "INSERT INTO messages (content, is_encrypted, encrypted_content) VALUES ('[ENCRYPTED]', 1, ?1)",
            [blob],
        )
        .unwrap();
        conn.execute("INSERT INTO messages (content) VALUES ('plain')", []).unwrap();

        assert_eq!(restore_legacy_messages(&conn, &key).unwrap(), 1);
        let (content, enc): (String, i64) = conn
            .query_row("SELECT content, is_encrypted FROM messages WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(content, "an old secret");
        assert_eq!(enc, 0);
    }
}
