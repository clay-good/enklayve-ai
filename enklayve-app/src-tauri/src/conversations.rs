use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// A conversation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub conversation_id: i64,
    pub role: String, // "user" or "assistant"
    pub content: String,
    pub timestamp: i64,
    pub tokens: Option<i32>,
}

/// A conversation thread
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: i64,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: i32,
    pub model_name: Option<String>,
}

/// Conversation summary for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub id: i64,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: i32,
    pub last_message: Option<String>,
}

/// Initialize conversation tables
pub fn init_conversation_tables(conn: &Connection) -> Result<()> {
    // Create conversations table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS conversations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            model_name TEXT,
            archived INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )?;

    // Migrate existing tables: add the archived column if it is missing.
    let conv_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(conversations)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !conv_cols.contains(&"archived".to_string()) {
        conn.execute(
            "ALTER TABLE conversations ADD COLUMN archived INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }

    // Create messages table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            conversation_id INTEGER NOT NULL,
            role TEXT NOT NULL CHECK(role IN ('user', 'assistant')),
            content TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            tokens INTEGER,
            FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
        )",
        [],
    )?;

    // Create index for faster message lookups
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_conversation
         ON messages(conversation_id, timestamp)",
        [],
    )?;

    Ok(())
}

/// Create a new conversation
pub fn create_conversation(conn: &Connection, title: Option<&str>) -> Result<i64> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs() as i64;

    let title = title.unwrap_or("New Conversation");

    conn.execute(
        "INSERT INTO conversations (title, created_at, updated_at)
         VALUES (?1, ?2, ?3)",
        rusqlite::params![title, now, now],
    )?;

    Ok(conn.last_insert_rowid())
}

/// Add a message to a conversation
pub fn add_message(
    conn: &Connection,
    conversation_id: i64,
    role: &str,
    content: &str,
    tokens: Option<i32>,
) -> Result<i64> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs() as i64;

    // Use transaction to ensure atomicity of message insert + conversation update
    conn.execute("BEGIN IMMEDIATE", [])?;

    let result: Result<i64> = (|| {
        conn.execute(
            "INSERT INTO messages (conversation_id, role, content, timestamp, tokens)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![conversation_id, role, content, now, tokens],
        )?;

        let message_id = conn.last_insert_rowid();

        // Update conversation's updated_at timestamp
        conn.execute(
            "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, conversation_id],
        )?;

        Ok(message_id)
    })();

    match result {
        Ok(id) => {
            conn.execute("COMMIT", [])?;
            Ok(id)
        }
        Err(e) => {
            conn.execute("ROLLBACK", []).ok();
            Err(e)
        }
    }
}

/// Get all messages in a conversation
pub fn get_conversation_messages(
    conn: &Connection,
    conversation_id: i64,
) -> Result<Vec<Message>> {
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, role, content, timestamp, tokens, is_encrypted, encrypted_content
         FROM messages
         WHERE conversation_id = ?1
         ORDER BY timestamp ASC",
    )?;

    let messages = stmt
        .query_map([conversation_id], |row| {
            let is_encrypted: Option<bool> = row.get(6).ok();
            let content = if is_encrypted.unwrap_or(false) {
                // If encrypted, show placeholder (actual decryption requires password)
                "[ENCRYPTED - Enter password to decrypt]".to_string()
            } else {
                row.get(3)?
            };

            Ok(Message {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                role: row.get(2)?,
                content,
                timestamp: row.get(4)?,
                tokens: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(messages)
}

/// Get conversation by ID
pub fn get_conversation(conn: &Connection, conversation_id: i64) -> Result<Conversation> {
    let mut stmt = conn.prepare(
        "SELECT id, title, created_at, updated_at, model_name,
                (SELECT COUNT(*) FROM messages WHERE conversation_id = c.id) as message_count
         FROM conversations c
         WHERE id = ?1",
    )?;

    let conversation = stmt.query_row([conversation_id], |row| {
        Ok(Conversation {
            id: row.get(0)?,
            title: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            model_name: row.get(4)?,
            message_count: row.get(5)?,
        })
    })?;

    Ok(conversation)
}

/// List all conversations
pub fn list_conversations(conn: &Connection, limit: Option<i32>) -> Result<Vec<ConversationSummary>> {
    let limit = limit.unwrap_or(100);

    let mut stmt = conn.prepare(
        "SELECT c.id, c.title, c.created_at, c.updated_at,
                (SELECT COUNT(*) FROM messages WHERE conversation_id = c.id) as message_count,
                (SELECT content FROM messages WHERE conversation_id = c.id ORDER BY timestamp DESC LIMIT 1) as last_message
         FROM conversations c
         WHERE c.archived = 0
         ORDER BY c.updated_at DESC
         LIMIT ?1",
    )?;

    let conversations = stmt
        .query_map([limit], |row| {
            Ok(ConversationSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                message_count: row.get(4)?,
                last_message: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(conversations)
}

/// List archived conversations (most recently updated first).
pub fn list_archived_conversations(conn: &Connection) -> Result<Vec<ConversationSummary>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.title, c.created_at, c.updated_at,
                (SELECT COUNT(*) FROM messages WHERE conversation_id = c.id) as message_count,
                (SELECT content FROM messages WHERE conversation_id = c.id ORDER BY timestamp DESC LIMIT 1) as last_message
         FROM conversations c
         WHERE c.archived = 1
         ORDER BY c.updated_at DESC",
    )?;

    let conversations = stmt
        .query_map([], |row| {
            Ok(ConversationSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                message_count: row.get(4)?,
                last_message: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(conversations)
}

/// Archive or unarchive a conversation.
pub fn set_conversation_archived(conn: &Connection, id: i64, archived: bool) -> Result<()> {
    conn.execute(
        "UPDATE conversations SET archived = ?1 WHERE id = ?2",
        rusqlite::params![if archived { 1 } else { 0 }, id],
    )?;
    Ok(())
}

/// Update conversation title
pub fn update_conversation_title(
    conn: &Connection,
    conversation_id: i64,
    title: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE conversations SET title = ?1 WHERE id = ?2",
        rusqlite::params![title, conversation_id],
    )?;

    Ok(())
}

/// Update conversation model
pub fn update_conversation_model(
    conn: &Connection,
    conversation_id: i64,
    model_name: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE conversations SET model_name = ?1 WHERE id = ?2",
        rusqlite::params![model_name, conversation_id],
    )?;

    Ok(())
}

/// Delete a conversation and all its messages
pub fn delete_conversation(conn: &Connection, conversation_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM conversations WHERE id = ?1",
        [conversation_id],
    )?;

    Ok(())
}

/// Delete a specific message
pub fn delete_message(conn: &Connection, message_id: i64) -> Result<()> {
    conn.execute("DELETE FROM messages WHERE id = ?1", [message_id])?;

    Ok(())
}

/// Get conversation context (last N messages formatted for prompt)
pub fn get_conversation_context(
    conn: &Connection,
    conversation_id: i64,
    max_messages: usize,
) -> Result<String> {
    let messages = get_conversation_messages(conn, conversation_id)?;

    let recent_messages: Vec<_> = messages
        .iter()
        .rev()
        .take(max_messages)
        .rev()
        .collect();

    let mut context = String::new();
    for msg in recent_messages {
        context.push_str(&format!("{}: {}\n\n", msg.role, msg.content));
    }

    Ok(context)
}

/// Get total token count for a conversation
pub fn get_conversation_token_count(conn: &Connection, conversation_id: i64) -> Result<i32> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(SUM(tokens), 0) FROM messages WHERE conversation_id = ?1",
    )?;

    let total: i32 = stmt.query_row([conversation_id], |row| row.get(0))?;

    Ok(total)
}

/// Search conversations by content
pub fn search_conversations(
    conn: &Connection,
    query: &str,
    limit: i32,
) -> Result<Vec<ConversationSummary>> {
    let search_pattern = format!("%{}%", query);

    let mut stmt = conn.prepare(
        "SELECT DISTINCT c.id, c.title, c.created_at, c.updated_at,
                (SELECT COUNT(*) FROM messages WHERE conversation_id = c.id) as message_count,
                (SELECT content FROM messages WHERE conversation_id = c.id ORDER BY timestamp DESC LIMIT 1) as last_message
         FROM conversations c
         INNER JOIN messages m ON c.id = m.conversation_id
         WHERE (c.title LIKE ?1 OR m.content LIKE ?1) AND c.archived = 0
         ORDER BY c.updated_at DESC
         LIMIT ?2",
    )?;

    let conversations = stmt
        .query_map(rusqlite::params![search_pattern, limit], |row| {
            Ok(ConversationSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                message_count: row.get(4)?,
                last_message: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(conversations)
}

/// Export conversation to markdown format
pub fn export_conversation_markdown(
    conn: &Connection,
    conversation_id: i64,
) -> Result<String> {
    let conversation = get_conversation(conn, conversation_id)?;
    let messages = get_conversation_messages(conn, conversation_id)?;

    let mut markdown = String::new();

    // Header
    markdown.push_str(&format!("# {}\n\n", conversation.title));
    markdown.push_str(&format!(
        "**Created:** {}\n",
        format_timestamp(conversation.created_at)
    ));
    if let Some(model) = &conversation.model_name {
        markdown.push_str(&format!("**Model:** {}\n", model));
    }
    markdown.push_str(&format!("**Messages:** {}\n\n", conversation.message_count));
    markdown.push_str("---\n\n");

    // Messages
    for msg in messages {
        let role_header = if msg.role == "user" {
            "**User**"
        } else {
            "**Assistant**"
        };

        markdown.push_str(&format!(
            "{} *({})*\n\n",
            role_header,
            format_timestamp(msg.timestamp)
        ));
        markdown.push_str(&msg.content);
        markdown.push_str("\n\n---\n\n");
    }

    Ok(markdown)
}

/// Export conversation to JSON format
pub fn export_conversation_json(
    conn: &Connection,
    conversation_id: i64,
) -> Result<String> {
    let conversation = get_conversation(conn, conversation_id)?;
    let messages = get_conversation_messages(conn, conversation_id)?;

    let export = serde_json::json!({
        "conversation": {
            "id": conversation.id,
            "title": conversation.title,
            "created_at": conversation.created_at,
            "updated_at": conversation.updated_at,
            "model_name": conversation.model_name,
            "message_count": conversation.message_count,
        },
        "messages": messages,
    });

    Ok(serde_json::to_string_pretty(&export)?)
}

/// Export conversation to plain text format
pub fn export_conversation_text(
    conn: &Connection,
    conversation_id: i64,
) -> Result<String> {
    let conversation = get_conversation(conn, conversation_id)?;
    let messages = get_conversation_messages(conn, conversation_id)?;

    let mut text = String::new();

    // Header
    text.push_str(&format!("Conversation: {}\n", conversation.title));
    text.push_str(&format!("Created: {}\n", format_timestamp(conversation.created_at)));
    if let Some(model) = &conversation.model_name {
        text.push_str(&format!("Model: {}\n", model));
    }
    text.push_str(&format!("Messages: {}\n", conversation.message_count));
    text.push_str(&"=".repeat(80));
    text.push_str("\n\n");

    // Messages
    for msg in messages {
        text.push_str(&format!(
            "{} ({})\n",
            msg.role.to_uppercase(),
            format_timestamp(msg.timestamp)
        ));
        text.push_str(&"-".repeat(80));
        text.push_str("\n");
        text.push_str(&msg.content);
        text.push_str("\n\n");
    }

    Ok(text)
}

/// Helper function to format Unix timestamp
fn format_timestamp(timestamp: i64) -> String {
    use std::time::{Duration, UNIX_EPOCH};

    let datetime = UNIX_EPOCH + Duration::from_secs(timestamp as u64);

    // Simple formatting (you could use chrono crate for better formatting)
    format!("{:?}", datetime)
}

/// Auto-generate conversation title from first message
pub fn auto_generate_title(content: &str) -> String {
    let words: Vec<&str> = content.split_whitespace().collect();
    if words.is_empty() {
        return "New Conversation".to_string();
    }

    let truncated_by_word_count = words.len() > 6;
    let preview: String = words.iter().take(6).cloned().collect::<Vec<_>>().join(" ");

    if preview.len() > 50 {
        format!("{}...", &preview[..47])
    } else if truncated_by_word_count {
        format!("{}...", preview)
    } else {
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_db() -> Result<Connection> {
        let conn = Connection::open_in_memory()?;
        init_conversation_tables(&conn)?;
        // Production adds these message columns via the encryption migration
        // (encrypted_database::initialize_encryption_support), which
        // get_conversation_messages reads. Mirror that here so the test schema
        // matches production.
        conn.execute(
            "ALTER TABLE messages ADD COLUMN is_encrypted BOOLEAN NOT NULL DEFAULT 0",
            [],
        )?;
        conn.execute("ALTER TABLE messages ADD COLUMN encrypted_content BLOB", [])?;
        Ok(conn)
    }

    #[test]
    fn test_create_conversation() {
        let conn = create_test_db().unwrap();
        let conv_id = create_conversation(&conn, Some("Test Conversation")).unwrap();
        assert!(conv_id > 0);

        let conversation = get_conversation(&conn, conv_id).unwrap();
        assert_eq!(conversation.title, "Test Conversation");
        assert_eq!(conversation.message_count, 0);
    }

    #[test]
    fn test_add_messages() {
        let conn = create_test_db().unwrap();
        let conv_id = create_conversation(&conn, None).unwrap();

        add_message(&conn, conv_id, "user", "Hello!", None).unwrap();
        add_message(&conn, conv_id, "assistant", "Hi there!", Some(10)).unwrap();

        let messages = get_conversation_messages(&conn, conv_id).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
    }

    #[test]
    fn test_conversation_context() {
        let conn = create_test_db().unwrap();
        let conv_id = create_conversation(&conn, None).unwrap();

        add_message(&conn, conv_id, "user", "Question 1", None).unwrap();
        add_message(&conn, conv_id, "assistant", "Answer 1", None).unwrap();
        add_message(&conn, conv_id, "user", "Question 2", None).unwrap();

        let context = get_conversation_context(&conn, conv_id, 2).unwrap();
        assert!(context.contains("Answer 1"));
        assert!(context.contains("Question 2"));
    }

    #[test]
    fn test_delete_conversation() {
        let conn = create_test_db().unwrap();
        let conv_id = create_conversation(&conn, None).unwrap();
        add_message(&conn, conv_id, "user", "Test", None).unwrap();

        delete_conversation(&conn, conv_id).unwrap();

        let result = get_conversation(&conn, conv_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_export_formats() {
        let conn = create_test_db().unwrap();
        let conv_id = create_conversation(&conn, Some("Export Test")).unwrap();
        add_message(&conn, conv_id, "user", "Hello", None).unwrap();
        add_message(&conn, conv_id, "assistant", "Hi", None).unwrap();

        let markdown = export_conversation_markdown(&conn, conv_id).unwrap();
        assert!(markdown.contains("# Export Test"));
        assert!(markdown.contains("**User**"));

        let json = export_conversation_json(&conn, conv_id).unwrap();
        assert!(json.contains("Export Test"));

        let text = export_conversation_text(&conn, conv_id).unwrap();
        assert!(text.contains("Conversation: Export Test"));
    }

    #[test]
    fn test_auto_title_generation() {
        let title1 = auto_generate_title("This is a very long question about machine learning and artificial intelligence");
        assert!(title1.len() <= 50);
        assert!(title1.ends_with("..."));

        let title2 = auto_generate_title("Short question");
        assert_eq!(title2, "Short question");

        let title3 = auto_generate_title("");
        assert_eq!(title3, "New Conversation");
    }
}
