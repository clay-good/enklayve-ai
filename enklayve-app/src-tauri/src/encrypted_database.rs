use anyhow::{Result, Context as AnyhowContext};
use rusqlite::Connection;
use crate::encryption::{EncryptionKey, encrypt, decrypt};

/// Settings for database encryption
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EncryptionSettings {
    pub enabled: bool,
    pub salt: Option<[u8; 16]>,
}

impl Default for EncryptionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            salt: None,
        }
    }
}

/// Store encrypted chunk in database
pub fn store_encrypted_chunk(
    conn: &Connection,
    document_id: i64,
    chunk_text: &str,
    embedding: &[f32],
    chunk_index: i32,
    encryption_key: Option<&EncryptionKey>,
) -> Result<()> {
    let (encrypted_text, encrypted_embedding) = if let Some(key) = encryption_key {
        // Encrypt chunk text
        let text_bytes = chunk_text.as_bytes();
        let encrypted_text = encrypt(text_bytes, key)
            .context("Failed to encrypt chunk text")?;

        // Encrypt embedding
        let embedding_bytes = bincode::serialize(embedding)
            .context("Failed to serialize embedding")?;
        let encrypted_embedding = encrypt(&embedding_bytes, key)
            .context("Failed to encrypt embedding")?;

        (encrypted_text, encrypted_embedding)
    } else {
        // Store unencrypted (for backward compatibility)
        let text_bytes = chunk_text.as_bytes().to_vec();
        let embedding_bytes = bincode::serialize(embedding)
            .context("Failed to serialize embedding")?;

        (text_bytes, embedding_bytes)
    };

    conn.execute(
        "INSERT INTO chunks (document_id, chunk_text, embedding, chunk_index, is_encrypted)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            document_id,
            encrypted_text,
            encrypted_embedding,
            chunk_index,
            encryption_key.is_some()
        ],
    )?;

    Ok(())
}

/// Retrieve and decrypt chunk from database
pub fn retrieve_encrypted_chunk(
    conn: &Connection,
    chunk_id: i64,
    encryption_key: Option<&EncryptionKey>,
) -> Result<(String, Vec<f32>)> {
    let mut stmt = conn.prepare(
        "SELECT chunk_text, embedding, is_encrypted FROM chunks WHERE id = ?1"
    )?;

    let (encrypted_text, encrypted_embedding, is_encrypted): (Vec<u8>, Vec<u8>, bool) =
        stmt.query_row([chunk_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?
            ))
        })?;

    let (chunk_text, embedding) = if is_encrypted {
        let key = encryption_key
            .ok_or_else(|| anyhow::anyhow!("Chunk is encrypted but no key provided"))?;

        // Decrypt chunk text
        let text_bytes = decrypt(&encrypted_text, key)
            .context("Failed to decrypt chunk text")?;
        let chunk_text = String::from_utf8(text_bytes)
            .context("Invalid UTF-8 in decrypted chunk")?;

        // Decrypt embedding
        let embedding_bytes = decrypt(&encrypted_embedding, key)
            .context("Failed to decrypt embedding")?;
        let embedding: Vec<f32> = bincode::deserialize(&embedding_bytes)
            .context("Failed to deserialize embedding")?;

        (chunk_text, embedding)
    } else {
        // Unencrypted data
        let chunk_text = String::from_utf8(encrypted_text)
            .context("Invalid UTF-8 in chunk")?;
        let embedding: Vec<f32> = bincode::deserialize(&encrypted_embedding)
            .context("Failed to deserialize embedding")?;

        (chunk_text, embedding)
    };

    Ok((chunk_text, embedding))
}

/// Get all chunks for a document (with decryption)
pub fn get_document_chunks_decrypted(
    conn: &Connection,
    document_id: i64,
    encryption_key: Option<&EncryptionKey>,
) -> Result<Vec<(i64, String, Vec<f32>, i32)>> {
    let mut stmt = conn.prepare(
        "SELECT id, chunk_text, embedding, chunk_index, is_encrypted
         FROM chunks
         WHERE document_id = ?1
         ORDER BY chunk_index"
    )?;

    let chunks = stmt.query_map([document_id], |row| {
        let chunk_id: i64 = row.get(0)?;
        let encrypted_text: Vec<u8> = row.get(1)?;
        let encrypted_embedding: Vec<u8> = row.get(2)?;
        let chunk_index: i32 = row.get(3)?;
        let is_encrypted: bool = row.get(4)?;

        Ok((chunk_id, encrypted_text, encrypted_embedding, chunk_index, is_encrypted))
    })?;

    let mut result = Vec::new();

    for chunk in chunks {
        let (chunk_id, encrypted_text, encrypted_embedding, chunk_index, is_encrypted) = chunk?;

        let (chunk_text, embedding) = if is_encrypted {
            let key = encryption_key
                .ok_or_else(|| anyhow::anyhow!("Chunk is encrypted but no key provided"))?;

            // Decrypt chunk text
            let text_bytes = decrypt(&encrypted_text, key)
                .context("Failed to decrypt chunk text")?;
            let chunk_text = String::from_utf8(text_bytes)
                .context("Invalid UTF-8 in decrypted chunk")?;

            // Decrypt embedding
            let embedding_bytes = decrypt(&encrypted_embedding, key)
                .context("Failed to decrypt embedding")?;
            let embedding: Vec<f32> = bincode::deserialize(&embedding_bytes)
                .context("Failed to deserialize embedding")?;

            (chunk_text, embedding)
        } else {
            // Unencrypted data
            let chunk_text = String::from_utf8(encrypted_text)
                .context("Invalid UTF-8 in chunk")?;
            let embedding: Vec<f32> = bincode::deserialize(&encrypted_embedding)
                .context("Failed to deserialize embedding")?;

            (chunk_text, embedding)
        };

        result.push((chunk_id, chunk_text, embedding, chunk_index));
    }

    Ok(result)
}

/// Initialize encryption for database (add encryption columns if missing)
pub fn initialize_encryption_support(conn: &Connection) -> Result<()> {
    // Check if is_encrypted column exists in messages table
    let is_encrypted_exists: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name='is_encrypted'")?
        .query_row([], |row| row.get(0))
        .map(|count: i32| count > 0)?;

    if !is_encrypted_exists {
        // Add is_encrypted column to messages table with default value false
        conn.execute(
            "ALTER TABLE messages ADD COLUMN is_encrypted BOOLEAN NOT NULL DEFAULT 0",
            [],
        )?;

        println!("Added is_encrypted column to messages table");
    }

    // Check if encrypted_content column exists
    let encrypted_content_exists: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name='encrypted_content'")?
        .query_row([], |row| row.get(0))
        .map(|count: i32| count > 0)?;

    if !encrypted_content_exists {
        // Add encrypted_content column to store encrypted message data
        conn.execute(
            "ALTER TABLE messages ADD COLUMN encrypted_content BLOB",
            [],
        )?;

        println!("Added encrypted_content column to messages table");
    }

    // Check if is_encrypted column exists in chunks table
    let chunks_is_encrypted_exists: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('chunks') WHERE name='is_encrypted'")?
        .query_row([], |row| row.get(0))
        .map(|count: i32| count > 0)?;

    if !chunks_is_encrypted_exists {
        // Add is_encrypted column to chunks table with default value false
        conn.execute(
            "ALTER TABLE chunks ADD COLUMN is_encrypted BOOLEAN NOT NULL DEFAULT 0",
            [],
        )?;

        println!("Added is_encrypted column to chunks table");
    }

    Ok(())
}

/// Migrate existing conversation messages to encrypted format
pub fn migrate_to_encrypted(
    conn: &Connection,
    encryption_key: &EncryptionKey,
) -> Result<usize> {
    // Get all unencrypted messages
    let mut stmt = conn.prepare(
        "SELECT id, content
         FROM messages
         WHERE is_encrypted = 0"
    )?;

    let messages: Vec<(i64, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let total = messages.len();

    // Encrypt each message
    for (message_id, content) in messages {
        // Convert string to bytes
        let content_bytes = content.as_bytes();

        // Encrypt content
        let encrypted_content = encrypt(content_bytes, encryption_key)
            .context("Failed to encrypt message content")?;

        // Store encrypted content in encrypted_content column, clear plain content, mark as encrypted
        conn.execute(
            "UPDATE messages
             SET encrypted_content = ?1, content = '[ENCRYPTED]', is_encrypted = 1
             WHERE id = ?2",
            rusqlite::params![encrypted_content, message_id],
        )?;
    }

    Ok(total)
}

/// Decrypt all encrypted chunks (for disabling encryption)
pub fn migrate_to_unencrypted(
    conn: &Connection,
    encryption_key: &EncryptionKey,
) -> Result<usize> {
    // Get all encrypted chunks
    let mut stmt = conn.prepare(
        "SELECT id, chunk_text, embedding
         FROM chunks
         WHERE is_encrypted = 1"
    )?;

    let chunks: Vec<(i64, Vec<u8>, Vec<u8>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let total = chunks.len();

    // Decrypt each chunk
    for (chunk_id, encrypted_text, encrypted_embedding) in chunks {
        // Decrypt text
        let text_bytes = decrypt(&encrypted_text, encryption_key)
            .context("Failed to decrypt chunk text")?;

        // Decrypt embedding
        let embedding_bytes = decrypt(&encrypted_embedding, encryption_key)
            .context("Failed to decrypt embedding")?;

        // Update chunk
        conn.execute(
            "UPDATE chunks
             SET chunk_text = ?1, embedding = ?2, is_encrypted = 0
             WHERE id = ?3",
            rusqlite::params![text_bytes, embedding_bytes, chunk_id],
        )?;
    }

    Ok(total)
}

/// Get encryption statistics
pub fn get_encryption_stats(conn: &Connection) -> Result<(usize, usize)> {
    let total: usize = conn
        .prepare("SELECT COUNT(*) FROM chunks")?
        .query_row([], |row| row.get(0))?;

    let encrypted: usize = conn
        .prepare("SELECT COUNT(*) FROM chunks WHERE is_encrypted = 1")?
        .query_row([], |row| row.get(0))?;

    Ok((total, encrypted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn create_test_db() -> Result<Connection> {
        let conn = Connection::open_in_memory()?;

        // Create tables
        conn.execute(
            "CREATE TABLE documents (
                id INTEGER PRIMARY KEY,
                file_name TEXT NOT NULL,
                file_path TEXT NOT NULL,
                file_type TEXT NOT NULL,
                uploaded_at INTEGER NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE chunks (
                id INTEGER PRIMARY KEY,
                document_id INTEGER NOT NULL,
                chunk_text BLOB NOT NULL,
                embedding BLOB NOT NULL,
                chunk_index INTEGER NOT NULL,
                is_encrypted BOOLEAN NOT NULL DEFAULT 0,
                FOREIGN KEY (document_id) REFERENCES documents(id)
            )",
            [],
        )?;

        // Insert test document
        conn.execute(
            "INSERT INTO documents (file_name, file_path, file_type, uploaded_at)
             VALUES ('test.txt', '/test.txt', 'txt', 0)",
            [],
        )?;

        Ok(conn)
    }

    #[test]
    fn test_encrypted_storage() {
        let conn = create_test_db().unwrap();
        let password = "test_password";
        let salt = EncryptionKey::generate_salt();
        let key = EncryptionKey::from_password(password, &salt).unwrap();

        // Store encrypted chunk
        let chunk_text = "This is a secret document";
        let embedding = vec![0.1, 0.2, 0.3, 0.4];

        store_encrypted_chunk(&conn, 1, chunk_text, &embedding, 0, Some(&key)).unwrap();

        // Retrieve and decrypt
        let (decrypted_text, decrypted_embedding) =
            retrieve_encrypted_chunk(&conn, 1, Some(&key)).unwrap();

        assert_eq!(decrypted_text, chunk_text);
        assert_eq!(decrypted_embedding, embedding);
    }

    #[test]
    fn test_wrong_key_fails() {
        let conn = create_test_db().unwrap();
        let salt = EncryptionKey::generate_salt();
        let key1 = EncryptionKey::from_password("password1", &salt).unwrap();
        let key2 = EncryptionKey::from_password("password2", &salt).unwrap();

        // Store with key1
        let chunk_text = "Secret data";
        let embedding = vec![0.1, 0.2];

        store_encrypted_chunk(&conn, 1, chunk_text, &embedding, 0, Some(&key1)).unwrap();

        // Try to retrieve with key2 (should fail)
        let result = retrieve_encrypted_chunk(&conn, 1, Some(&key2));
        assert!(result.is_err());
    }

    #[test]
    fn test_migration() {
        let conn = create_test_db().unwrap();
        let salt = EncryptionKey::generate_salt();
        let key = EncryptionKey::from_password("migration_key", &salt).unwrap();

        // --- migrate_to_encrypted operates on the messages table ---
        conn.execute(
            "CREATE TABLE messages (
                id INTEGER PRIMARY KEY,
                content TEXT NOT NULL,
                is_encrypted BOOLEAN NOT NULL DEFAULT 0,
                encrypted_content BLOB
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (content) VALUES ('a secret message')",
            [],
        )
        .unwrap();

        let migrated = migrate_to_encrypted(&conn, &key).unwrap();
        assert_eq!(migrated, 1);

        let is_encrypted: i64 = conn
            .query_row("SELECT is_encrypted FROM messages WHERE id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(is_encrypted, 1);

        // --- migrate_to_unencrypted operates on the chunks table ---
        let chunk_text = "Encrypted data";
        let embedding = vec![1.0, 2.0, 3.0];
        store_encrypted_chunk(&conn, 1, chunk_text, &embedding, 0, Some(&key)).unwrap();

        let (total, encrypted) = get_encryption_stats(&conn).unwrap();
        assert_eq!(total, 1);
        assert_eq!(encrypted, 1);

        let migrated = migrate_to_unencrypted(&conn, &key).unwrap();
        assert_eq!(migrated, 1);

        let (total, encrypted) = get_encryption_stats(&conn).unwrap();
        assert_eq!(total, 1);
        assert_eq!(encrypted, 0);

        // The chunk is now readable without a key.
        let (decrypted_text, decrypted_embedding) =
            retrieve_encrypted_chunk(&conn, 1, None).unwrap();
        assert_eq!(decrypted_text, chunk_text);
        assert_eq!(decrypted_embedding, embedding);
    }
}
