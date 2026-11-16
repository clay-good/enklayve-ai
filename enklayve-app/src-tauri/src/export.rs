use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs;
use std::io::Write as IoWrite;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportMetadata {
    pub export_date: String,
    pub app_version: String,
    pub total_conversations: usize,
    pub conversation_ids: Vec<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConversationExportMetadata {
    pub conversation_id: i64,
    pub title: String,
    pub created_at: String,
    pub message_count: usize,
    pub model_used: Option<String>,
    pub documents_used: Vec<String>,
}

pub struct ExportManager {
    app_handle: tauri::AppHandle,
}

impl ExportManager {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self { app_handle }
    }

    /// Export all conversations to a single ZIP file
    pub async fn export_all_conversations(&self, destination_path: &Path) -> Result<PathBuf> {
        crate::logger::log_info("Starting export of all conversations...");

        let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let export_filename = format!("enklayve_conversations_{}.zip", timestamp);
        let export_path = destination_path.join(export_filename);

        let file = fs::File::create(&export_path)
            .context("Failed to create export ZIP file")?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let conn = crate::database::get_connection(&self.app_handle)?;

        let conversations = crate::conversations::list_conversations(&conn, None)?;
        crate::logger::log_info(&format!("Exporting {} conversations", conversations.len()));

        let mut conversation_ids = Vec::new();

        for (index, conv_summary) in conversations.iter().enumerate() {
            let conversation = crate::conversations::get_conversation(&conn, conv_summary.id)?;
            let messages = crate::conversations::get_conversation_messages(&conn, conv_summary.id)?;

            conversation_ids.push(conv_summary.id);

            let documents_used = self.get_documents_used_in_conversation(&messages)?;

            let metadata = ConversationExportMetadata {
                conversation_id: conv_summary.id,
                title: conversation.title.clone(),
                created_at: conversation.created_at.to_string(),
                message_count: messages.len(),
                model_used: None,
                documents_used,
            };

            let markdown_content = self.generate_markdown_with_metadata(&conversation, &messages, &metadata)?;
            let json_content = self.generate_json_with_metadata(&conversation, &messages, &metadata)?;

            let safe_title = self.sanitize_filename(&conversation.title);
            let markdown_filename = format!("conversations/{:03}_{}.md", index + 1, safe_title);
            let json_filename = format!("conversations/{:03}_{}.json", index + 1, safe_title);

            zip.start_file(&markdown_filename, options)?;
            zip.write_all(markdown_content.as_bytes())?;

            zip.start_file(&json_filename, options)?;
            zip.write_all(json_content.as_bytes())?;

            crate::logger::log_info(&format!("Exported conversation {}/{}", index + 1, conversations.len()));
        }

        let export_metadata = ExportMetadata {
            export_date: chrono::Local::now().to_rfc3339(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            total_conversations: conversations.len(),
            conversation_ids,
        };

        let metadata_json = serde_json::to_string_pretty(&export_metadata)?;
        zip.start_file("export_metadata.json", options)?;
        zip.write_all(metadata_json.as_bytes())?;

        zip.finish()?;

        crate::logger::log_info(&format!("Export complete: {:?}", export_path));
        Ok(export_path)
    }

    /// Export a single conversation with embedded source documents
    pub async fn export_conversation_with_sources(
        &self,
        conversation_id: i64,
        destination_path: &Path,
    ) -> Result<PathBuf> {
        crate::logger::log_info(&format!("Exporting conversation {} with sources...", conversation_id));

        let conn = crate::database::get_connection(&self.app_handle)?;
        let conversation = crate::conversations::get_conversation(&conn, conversation_id)?;
        let messages = crate::conversations::get_conversation_messages(&conn, conversation_id)?;

        let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let safe_title = self.sanitize_filename(&conversation.title);
        let export_filename = format!("{}_{}.zip", safe_title, timestamp);
        let export_path = destination_path.join(export_filename);

        let file = fs::File::create(&export_path)
            .context("Failed to create export ZIP file")?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let documents_used = self.get_documents_used_in_conversation(&messages)?;

        let metadata = ConversationExportMetadata {
            conversation_id,
            title: conversation.title.clone(),
            created_at: conversation.created_at.to_string(),
            message_count: messages.len(),
            model_used: None,
            documents_used: documents_used.clone(),
        };

        let markdown_content = self.generate_markdown_with_metadata(&conversation, &messages, &metadata)?;
        zip.start_file("conversation.md", options)?;
        zip.write_all(markdown_content.as_bytes())?;

        let json_content = self.generate_json_with_metadata(&conversation, &messages, &metadata)?;
        zip.start_file("conversation.json", options)?;
        zip.write_all(json_content.as_bytes())?;

        let metadata_json = serde_json::to_string_pretty(&metadata)?;
        zip.start_file("metadata.json", options)?;
        zip.write_all(metadata_json.as_bytes())?;

        let app_data_dir = self.app_handle.path().app_data_dir()
            .context("Failed to get app data directory")?;
        let documents_dir = app_data_dir.join("documents");

        if documents_dir.exists() {
            for doc_name in &documents_used {
                let doc_path = documents_dir.join(doc_name);
                if doc_path.exists() && doc_path.is_file() {
                    let zip_path = format!("source_documents/{}", doc_name);
                    zip.start_file(&zip_path, options)?;
                    let mut file_content = fs::File::open(&doc_path)?;
                    std::io::copy(&mut file_content, &mut zip)?;
                    crate::logger::log_info(&format!("Embedded source document: {}", doc_name));
                }
            }
        }

        zip.finish()?;

        crate::logger::log_info(&format!("Export with sources complete: {:?}", export_path));
        Ok(export_path)
    }

    /// Generate enhanced Markdown with metadata header
    fn generate_markdown_with_metadata(
        &self,
        conversation: &crate::conversations::Conversation,
        messages: &[crate::conversations::Message],
        metadata: &ConversationExportMetadata,
    ) -> Result<String> {
        let mut output = String::new();

        output.push_str(&format!("# {}\n\n", conversation.title));

        output.push_str("---\n");
        output.push_str(&format!("**Exported:** {}\n", chrono::Local::now().format("%B %d, %Y at %I:%M %p")));
        output.push_str(&format!("**Created:** {}\n", metadata.created_at));
        output.push_str(&format!("**Messages:** {}\n", metadata.message_count));
        if let Some(model) = &metadata.model_used {
            output.push_str(&format!("**Model:** {}\n", model));
        }
        if !metadata.documents_used.is_empty() {
            output.push_str(&format!("**Source Documents:** {}\n", metadata.documents_used.join(", ")));
        }
        output.push_str("---\n\n");

        for msg in messages {
            let role_label = match msg.role.as_str() {
                "user" => "You",
                "assistant" => "Assistant",
                _ => "System",
            };

            output.push_str(&format!("### {} ({})\n\n", role_label, msg.timestamp));
            output.push_str(&msg.content);
            output.push_str("\n\n");
        }

        Ok(output)
    }

    /// Generate enhanced JSON with metadata
    fn generate_json_with_metadata(
        &self,
        conversation: &crate::conversations::Conversation,
        messages: &[crate::conversations::Message],
        metadata: &ConversationExportMetadata,
    ) -> Result<String> {
        #[derive(Serialize)]
        struct ConversationExport<'a> {
            metadata: &'a ConversationExportMetadata,
            conversation: &'a crate::conversations::Conversation,
            messages: &'a [crate::conversations::Message],
        }

        let export = ConversationExport {
            metadata,
            conversation,
            messages,
        };

        Ok(serde_json::to_string_pretty(&export)?)
    }

    /// Extract document names referenced in messages
    fn get_documents_used_in_conversation(&self, messages: &[crate::conversations::Message]) -> Result<Vec<String>> {
        let mut documents = Vec::new();
        let citation_pattern = regex::Regex::new(r"\[([^\]]+\.(?:pdf|txt|docx|md))\]")?;

        for msg in messages {
            if msg.role == "assistant" {
                for cap in citation_pattern.captures_iter(&msg.content) {
                    if let Some(doc) = cap.get(1) {
                        let doc_name = doc.as_str().to_string();
                        if !documents.contains(&doc_name) {
                            documents.push(doc_name);
                        }
                    }
                }
            }
        }

        documents.sort();
        Ok(documents)
    }

    /// Sanitize filename to be filesystem-safe
    fn sanitize_filename(&self, name: &str) -> String {
        name.chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                _ => c,
            })
            .collect::<String>()
            .chars()
            .take(50)
            .collect()
    }
}
