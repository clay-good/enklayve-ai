use crate::documents::DocumentMetadata;
use crate::models::{ModelInfo, ModelRecommendation};
use crate::downloads::{DownloadedModelInfo, DownloadInfo, ModelDownloader};
use crate::hardware::HardwareProfile;
use crate::encryption;
use crate::biometric;
use crate::encrypted_database;
use crate::database;
use crate::conversations;
use crate::settings;
use tauri::Emitter;

/// Simple greeting command for testing
#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! Welcome to Enklayve.", name)
}

/// Upload and process a document
#[tauri::command]
pub async fn upload_document(
    file_path: String,
    app_handle: tauri::AppHandle,
    model_cache: tauri::State<'_, crate::model_cache::ModelCache>,
) -> Result<DocumentMetadata, String> {
    let result = crate::documents::upload_document(file_path, &app_handle)
        .await
        .map_err(|e| e.to_string())?;

    model_cache.invalidate_prompt_cache();
    crate::logger::log_info("Prompt cache invalidated due to document upload");

    Ok(result)
}

/// List all uploaded documents
#[tauri::command]
pub async fn list_documents(app_handle: tauri::AppHandle) -> Result<Vec<DocumentMetadata>, String> {
    crate::documents::list_documents(&app_handle)
        .await
        .map_err(|e| e.to_string())
}

/// Delete a document and its chunks
#[tauri::command]
pub async fn delete_document(
    document_id: i64,
    app_handle: tauri::AppHandle,
    model_cache: tauri::State<'_, crate::model_cache::ModelCache>,
) -> Result<(), String> {
    crate::documents::delete_document(&app_handle, document_id)
        .await
        .map_err(|e| e.to_string())?;

    model_cache.invalidate_prompt_cache();
    crate::logger::log_info("Prompt cache invalidated due to document deletion");

    Ok(())
}

/// Get available models
#[tauri::command]
pub fn get_models() -> Vec<ModelInfo> {
    crate::models::get_available_models()
}

/// List downloaded models
#[tauri::command]
pub async fn list_downloaded_models(
    app_handle: tauri::AppHandle,
) -> Result<Vec<DownloadedModelInfo>, String> {
    crate::downloads::list_downloaded_models(&app_handle)
        .await
        .map_err(|e| e.to_string())
}

/// Get download info for a URL
#[tauri::command]
pub async fn get_download_info(url: String) -> Result<DownloadInfo, String> {
    let downloader = ModelDownloader::new().map_err(|e| e.to_string())?;
    downloader
        .get_download_info(&url)
        .await
        .map_err(|e| e.to_string())
}

/// Download a model
#[tauri::command]
pub async fn download_model(
    url: String,
    model_name: String,
    checksum: Option<String>,
    app_handle: tauri::AppHandle,
    window: tauri::Window,
) -> Result<String, String> {
    let downloader = ModelDownloader::new().map_err(|e| e.to_string())?;

    let path = downloader
        .download_model(&url, &model_name, checksum.as_deref(), &app_handle, move |progress| {
            // Emit progress event to frontend
            window.emit("download-progress", progress).ok();
        })
        .await
        .map_err(|e| e.to_string())?;

    Ok(path.to_string_lossy().to_string())
}

/// Delete a downloaded model
#[tauri::command]
pub async fn delete_model(app_handle: tauri::AppHandle, model_name: String) -> Result<(), String> {
    crate::downloads::delete_model(&app_handle, &model_name)
        .await
        .map_err(|e| e.to_string())
}

/// Always retrieve documents if they exist - let the model decide relevance
/// No hardcoded patterns or guessing about user intent
fn should_retrieve_documents(_query: &str, has_documents: bool) -> bool {
    has_documents
}

/// Query documents using RAG (Retrieval-Augmented Generation)
#[tauri::command]
pub async fn query_documents(
    question: String,
    model_path: Option<String>,
    conversation_id: Option<i64>,
    app_handle: tauri::AppHandle,
    model_cache: tauri::State<'_, crate::model_cache::ModelCache>,
) -> Result<String, String> {
    crate::logger::log_info(&format!("Query received: {}", question));
    crate::logger::log_info(&format!("Model path: {:?}", model_path));
    crate::logger::log_info(&format!("Conversation ID: {:?}", conversation_id));

    // Check if documents exist
    let documents = crate::documents::list_documents(&app_handle)
        .await
        .map_err(|e| e.to_string())?;
    let has_documents = !documents.is_empty();

    // Determine if retrieval is needed
    let should_retrieve = should_retrieve_documents(&question, has_documents);

    // Search for relevant chunks using hybrid search only if needed
    let search_results = if should_retrieve {
        crate::vector_search::hybrid_search(&question, &app_handle, 10)
            .await
            .map_err(|e| {
                crate::logger::log_error(&format!("Failed to search chunks: {}", e));
                e.to_string()
            })?
    } else {
        Vec::new()
    };

    crate::logger::log_info(&format!("Found {} relevant chunks from hybrid search", search_results.len()));

    // Use hybrid search results directly - no reranking, no filtering
    // Let the model see all relevant context and decide what's useful
    let max_chunks = 8;  // Generous limit for thorough answers
    let filtered_chunks: Vec<_> = search_results.iter().take(max_chunks).collect();

    crate::logger::log_info(&format!(
        "Using {} chunks for context",
        filtered_chunks.len()
    ));

    let context_chunks: Vec<String> = filtered_chunks
        .iter()
        .map(|r| {
            // Increased to ~1000 words for thorough document coverage
            // Qwen 7B has 8K context window - we have plenty of room
            let words: Vec<&str> = r.chunk_text.split_whitespace().collect();
            if words.len() > 1000 {
                words[..1000].join(" ") + "..."
            } else {
                r.chunk_text.clone()
            }
        })
        .collect();

    // Get conversation context if conversation_id provided (last 3 messages for continuity)
    let conversation_context = if let Some(conv_id) = conversation_id {
        let conn = crate::database::get_connection(&app_handle)
            .map_err(|e| e.to_string())?;

        // Get last 3 messages for context - balances continuity with context window limits
        crate::conversations::get_conversation_context(&conn, conv_id, 3)
            .unwrap_or_else(|e| {
                crate::logger::log_warn(&format!("Failed to get conversation context: {}", e));
                String::new()
            })
    } else {
        String::new()
    };

    // Get current date and time for context
    let now = chrono::Local::now();
    let current_date = now.format("%B %d, %Y").to_string(); // e.g., "November 21, 2025"
    let current_datetime = now.format("%B %d, %Y at %I:%M %p").to_string();

    // System prompt - honest about capabilities and knowledge cutoff
    let system_base = format!(
        "You are a helpful, knowledgeable AI assistant. Today is {}. Your knowledge was last updated in early 2024, so for questions about recent events, let the user know you may not have the latest information. Format your answers in clean, readable Markdown: short paragraphs, bulleted or numbered lists where they help, and fenced code blocks only for code. /no_think",
        current_date
    );

    // Create prompt using ChatML format - clean and natural
    let prompt = if context_chunks.is_empty() {
        // No documents - general knowledge query
        if conversation_context.is_empty() {
            format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                system_base, question
            )
        } else {
            format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\nConversation so far:\n{}\n\n{}<|im_end|>\n<|im_start|>assistant\n",
                system_base, conversation_context, question
            )
        }
    } else {
        // Documents available - RAG mode
        let mut docs_text = String::new();
        for (_i, (result, chunk_text)) in filtered_chunks.iter().zip(context_chunks.iter()).enumerate() {
            docs_text.push_str(&format!("[{}]\n{}\n\n", result.file_name, chunk_text));
        }

        let system_with_docs = format!(
            "{} You have access to the user's documents below. Use them to provide accurate, thorough answers.",
            system_base
        );

        if conversation_context.is_empty() {
            format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\nMy documents:\n\n{}\n{}<|im_end|>\n<|im_start|>assistant\n",
                system_with_docs, docs_text, question
            )
        } else {
            format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\nMy documents:\n\n{}\nConversation so far:\n{}\n\n{}<|im_end|>\n<|im_start|>assistant\n",
                system_with_docs, docs_text, conversation_context, question
            )
        }
    };

    // If model path provided, use actual LLM inference with caching
    if let Some(model_path_str) = model_path {
        let model_path = std::path::Path::new(&model_path_str);

        if !model_path.exists() {
            return Err(format!("Model file not found: {}", model_path_str));
        }

        // Load model into cache if not already loaded
        model_cache.get_or_load(&model_path_str)
            .map_err(|e| format!("Failed to load model: {}", e))?;

        // Generate response using cached model
        // Increased to 2048 tokens to prevent response cutoff (we have ~4600 tokens available in 8K context)
        let response = model_cache.generate(&prompt, 2048)
            .map_err(|e| format!("Failed to generate response: {}", e))?;

        // Clean response - strip control tokens and <think> blocks, keep Markdown
        let cleaned_response = crate::response_format::sanitize_final(&response);

        // Parse citations from response
        let parsed = crate::citations::parse_citations(&cleaned_response);
        crate::logger::log_info(&format!("Parsed {} citations from response", parsed.citations.len()));

        return Ok(cleaned_response);
    }

    // Fallback: return search results if no model provided
    if filtered_chunks.is_empty() {
        return Ok("No AI model is currently loaded. Please wait for the model to download, or check the application logs for errors.".to_string());
    }

    let mut response = format!(
        "Based on your documents, here are the most relevant passages:\n\n"
    );

    for (i, (result, chunk_text)) in filtered_chunks.iter().zip(context_chunks.iter()).enumerate() {
        response.push_str(&format!(
            "{}. From \"{}\" (similarity: {:.2}):\n{}\n\n",
            i + 1,
            result.file_name,
            result.similarity,
            chunk_text
        ));
    }

    response.push_str(&format!(
        "\nQuestion: {}\n\n",
        question
    ));

    response.push_str(
        "Note: No model selected. Download a model to get AI-generated answers. The above passages are the most relevant sections from your documents.\n"
    );

    Ok(response)
}

/// Detect hardware capabilities
#[tauri::command]
pub fn detect_hardware() -> Result<HardwareProfile, String> {
    HardwareProfile::detect().map_err(|e| e.to_string())
}

/// Get model recommendations based on hardware
#[tauri::command]
pub fn get_model_recommendations() -> Result<Vec<ModelRecommendation>, String> {
    let hardware = HardwareProfile::detect().map_err(|e| e.to_string())?;
    Ok(crate::models::get_recommended_models(&hardware))
}

/// Query documents using RAG with streaming response
#[tauri::command]
pub async fn query_documents_streaming(
    question: String,
    model_path: Option<String>,
    conversation_id: Option<i64>,
    app_handle: tauri::AppHandle,
    window: tauri::Window,
    model_cache: tauri::State<'_, crate::model_cache::ModelCache>,
) -> Result<String, String> {
    crate::logger::log_info(&format!("Streaming query received: {}", question));

    let documents = crate::documents::list_documents(&app_handle)
        .await
        .map_err(|e| e.to_string())?;
    let has_documents = !documents.is_empty();

    let should_retrieve = should_retrieve_documents(&question, has_documents);

    let search_results = if should_retrieve {
        crate::vector_search::hybrid_search(&question, &app_handle, 10)
            .await
            .map_err(|e| {
                crate::logger::log_error(&format!("Failed to search chunks: {}", e));
                e.to_string()
            })?
    } else {
        Vec::new()
    };

    crate::logger::log_info(&format!("Found {} relevant chunks from hybrid search", search_results.len()));

    // Use hybrid search results directly - no reranking, no filtering
    // Let the model see all relevant context and decide what's useful
    let max_chunks = 8;  // Generous limit for thorough answers
    let filtered_chunks: Vec<_> = search_results.iter().take(max_chunks).collect();

    crate::logger::log_info(&format!(
        "Using {} chunks for context",
        filtered_chunks.len()
    ));

    let context_chunks: Vec<String> = filtered_chunks
        .iter()
        .map(|r| {
            // Increased to ~1000 words for thorough document coverage
            // Qwen 7B has 8K context window - we have plenty of room
            let words: Vec<&str> = r.chunk_text.split_whitespace().collect();
            if words.len() > 1000 {
                words[..1000].join(" ") + "..."
            } else {
                r.chunk_text.clone()
            }
        })
        .collect();

    // Get conversation context if conversation_id provided (last 3 messages for continuity)
    let conversation_context = if let Some(conv_id) = conversation_id {
        let conn = crate::database::get_connection(&app_handle)
            .map_err(|e| e.to_string())?;

        // Get last 3 messages for context - balances continuity with context window limits
        crate::conversations::get_conversation_context(&conn, conv_id, 3)
            .unwrap_or_else(|e| {
                crate::logger::log_warn(&format!("Failed to get conversation context: {}", e));
                String::new()
            })
    } else {
        String::new()
    };

    // Get current date for context
    let now = chrono::Local::now();
    let current_date = now.format("%B %d, %Y").to_string();

    // System prompt - honest about capabilities and knowledge cutoff
    let system_base = format!(
        "You are a helpful, knowledgeable AI assistant. Today is {}. Your knowledge was last updated in early 2024, so for questions about recent events, let the user know you may not have the latest information. Format your answers in clean, readable Markdown: short paragraphs, bulleted or numbered lists where they help, and fenced code blocks only for code. /no_think",
        current_date
    );

    // Create prompt using ChatML format - clean and natural
    let prompt = if context_chunks.is_empty() {
        // No documents - general knowledge query
        if conversation_context.is_empty() {
            format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                system_base, question
            )
        } else {
            format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\nConversation so far:\n{}\n\n{}<|im_end|>\n<|im_start|>assistant\n",
                system_base, conversation_context, question
            )
        }
    } else {
        // Documents available - RAG mode
        let mut docs_text = String::new();
        for (_i, (result, chunk_text)) in filtered_chunks.iter().zip(context_chunks.iter()).enumerate() {
            docs_text.push_str(&format!("[{}]\n{}\n\n", result.file_name, chunk_text));
        }

        let system_with_docs = format!(
            "{} You have access to the user's documents below. Use them to provide accurate, thorough answers.",
            system_base
        );

        if conversation_context.is_empty() {
            format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\nMy documents:\n\n{}\n{}<|im_end|>\n<|im_start|>assistant\n",
                system_with_docs, docs_text, question
            )
        } else {
            format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\nMy documents:\n\n{}\nConversation so far:\n{}\n\n{}<|im_end|>\n<|im_start|>assistant\n",
                system_with_docs, docs_text, conversation_context, question
            )
        }
    };

    // If model path provided, use actual LLM inference with streaming
    if let Some(model_path_str) = model_path {
        // Load model into cache if not already loaded
        model_cache.get_or_load(&model_path_str)
            .map_err(|e| format!("Failed to load model: {}", e))?;

        // Generate response with buffered streaming
        // 2000 tokens for complete, thorough answers - Qwen can handle it
        // Sanitize the stream so the live view matches the final answer:
        // drop <think> reasoning and control tokens, preserve Markdown.
        let mut sanitizer = crate::response_format::StreamSanitizer::new();
        let response = model_cache.generate_streaming(&prompt, 2000, |token_batch| {
            let visible = sanitizer.push(token_batch);
            if !visible.is_empty() {
                window.emit("llm-token", visible).ok();
            }
            Ok(())
        })
        .map_err(|e| format!("Failed to generate response: {}", e))?;

        // Flush any text held back at the stream tail.
        let tail = sanitizer.flush();
        if !tail.is_empty() {
            window.emit("llm-token", tail).ok();
        }

        // Final, fully-cleaned answer replaces the streamed text in the UI.
        let cleaned_response = crate::response_format::sanitize_final(&response);
        window.emit("llm-complete", &cleaned_response).ok();

        return Ok(cleaned_response);
    }

    // Fallback: return search results if no model provided
    if filtered_chunks.is_empty() {
        return Ok("No AI model is currently loaded. Please wait for the model to download, or check the application logs for errors.".to_string());
    }

    let mut response = format!(
        "Based on your documents, here are the most relevant passages:\n\n"
    );

    for (i, (result, chunk_text)) in filtered_chunks.iter().zip(context_chunks.iter()).enumerate() {
        response.push_str(&format!(
            "{}. From \"{}\" (similarity: {:.2}):\n{}\n\n",
            i + 1,
            result.file_name,
            result.similarity,
            chunk_text
        ));
    }

    response.push_str(&format!(
        "\nQuestion: {}\n\n",
        question
    ));

    response.push_str(
        "Note: No model selected. Download a model to get AI-generated answers. The above passages are the most relevant sections from your documents.\n"
    );

    Ok(response)
}

/// Hash a password for secure storage
#[tauri::command]
pub fn hash_password(password: String) -> Result<String, String> {
    encryption::hash_password(&password)
        .map_err(|e| e.to_string())
}

/// Verify a password against a stored hash
#[tauri::command]
pub fn verify_password(password: String, password_hash: String) -> Result<bool, String> {
    encryption::verify_password(&password, &password_hash)
        .map_err(|e| e.to_string())
}

/// Encrypt data (for testing/utility purposes)
#[tauri::command]
pub fn encrypt_data(data: String, password: String) -> Result<Vec<u8>, String> {
    let salt = encryption::EncryptionKey::generate_salt();
    let key = encryption::EncryptionKey::from_password(&password, &salt)
        .map_err(|e| e.to_string())?;

    let mut result = salt.to_vec();
    let encrypted = encryption::encrypt(data.as_bytes(), &key)
        .map_err(|e| e.to_string())?;
    result.extend_from_slice(&encrypted);

    Ok(result)
}

/// Decrypt data (for testing/utility purposes)
#[tauri::command]
pub fn decrypt_data(encrypted_data: Vec<u8>, password: String) -> Result<String, String> {
    if encrypted_data.len() < 16 {
        return Err("Invalid encrypted data".to_string());
    }

    let salt: [u8; 16] = encrypted_data[..16].try_into()
        .map_err(|_| "Invalid salt".to_string())?;
    let key = encryption::EncryptionKey::from_password(&password, &salt)
        .map_err(|e| e.to_string())?;

    let decrypted = encryption::decrypt(&encrypted_data[16..], &key)
        .map_err(|e| e.to_string())?;

    String::from_utf8(decrypted)
        .map_err(|e| format!("Invalid UTF-8: {}", e))
}

/// Check if biometric authentication is available
#[tauri::command]
pub fn check_biometric_available() -> Result<biometric::BiometricCapability, String> {
    biometric::is_biometric_available()
        .map_err(|e| e.to_string())
}

/// Authenticate using biometric (Touch ID/Windows Hello)
#[tauri::command]
pub fn authenticate_biometric(reason: String) -> Result<bool, String> {
    crate::logger::log_info(&format!("Attempting biometric authentication: {}", reason));

    let result = biometric::authenticate_biometric(&reason)
        .map_err(|e| {
            crate::logger::log_error(&format!("Biometric authentication failed: {}", e));
            e.to_string()
        })?;

    if result {
        crate::logger::log_info("Biometric authentication successful");
    } else {
        crate::logger::log_info("Biometric authentication failed");
    }

    Ok(result)
}

/// Store data securely with biometric protection
#[tauri::command]
pub fn store_secure_data(key: String, data: Vec<u8>) -> Result<(), String> {
    crate::logger::log_info(&format!("Storing secure data for key: {}", key));

    biometric::store_secure(&key, &data)
        .map_err(|e| {
            crate::logger::log_error(&format!("Failed to store secure data: {}", e));
            e.to_string()
        })?;

    crate::logger::log_info(&format!("Successfully stored secure data for key: {}", key));
    Ok(())
}

/// Retrieve securely stored data (requires biometric authentication)
#[tauri::command]
pub fn retrieve_secure_data(key: String) -> Result<Vec<u8>, String> {
    crate::logger::log_info(&format!("Retrieving secure data for key: {}", key));

    let data = biometric::retrieve_secure(&key)
        .map_err(|e| {
            crate::logger::log_error(&format!("Failed to retrieve secure data: {}", e));
            e.to_string()
        })?;

    crate::logger::log_info(&format!("Successfully retrieved secure data for key: {}", key));
    Ok(data)
}

/// Get encryption statistics for database
#[tauri::command]
pub async fn get_encryption_stats(app_handle: tauri::AppHandle) -> Result<(usize, usize), String> {
    let db_path = database::get_database_path(&app_handle)
        .map_err(|e| e.to_string())?;

    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| e.to_string())?;

    encrypted_database::get_encryption_stats(&conn)
        .map_err(|e| e.to_string())
}

/// Enable database encryption (migrate existing data)
#[tauri::command]
pub async fn enable_database_encryption(
    app_handle: tauri::AppHandle,
    password: String,
) -> Result<usize, String> {
    // FIRST LOG - If you don't see this, the command is not being called from frontend
    crate::logger::log_info("🔐🔐🔐 ENABLE_DATABASE_ENCRYPTION COMMAND CALLED FROM FRONTEND 🔐🔐🔐");
    crate::logger::log_info(&format!("Password length: {} characters", password.len()));
    crate::logger::log_info("Starting conversation encryption...");

    let db_path = database::get_database_path(&app_handle)
        .map_err(|e| {
            crate::logger::log_error(&format!("Failed to get database path: {}", e));
            e.to_string()
        })?;

    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| {
            crate::logger::log_error(&format!("Failed to open database: {}", e));
            e.to_string()
        })?;

    // Initialize encryption support
    crate::logger::log_info("Initializing encryption support...");
    encrypted_database::initialize_encryption_support(&conn)
        .map_err(|e| {
            crate::logger::log_error(&format!("Failed to initialize encryption support: {}", e));
            e.to_string()
        })?;

    // Create encryption key
    crate::logger::log_info("Generating encryption key from password...");
    let salt = encryption::EncryptionKey::generate_salt();
    let key = encryption::EncryptionKey::from_password(&password, &salt)
        .map_err(|e| {
            crate::logger::log_error(&format!("Failed to generate encryption key: {}", e));
            e.to_string()
        })?;

    // Store salt securely (with biometric protection if available)
    crate::logger::log_info("Storing encryption salt securely...");
    biometric::store_secure("db_encryption_salt", &salt)
        .map_err(|e| {
            crate::logger::log_error(&format!("Failed to store salt: {}", e));
            e.to_string()
        })?;

    // Migrate conversation data to encrypted format
    crate::logger::log_info("Encrypting conversation messages...");
    let result = encrypted_database::migrate_to_encrypted(&conn, &key)
        .map_err(|e| {
            crate::logger::log_error(&format!("Failed to encrypt conversations: {}", e));
            e.to_string()
        })?;

    crate::logger::log_info(&format!("✅ Conversation encryption completed. Encrypted {} messages", result));
    crate::logger::log_info("⚠️  Note: Document chunks are NOT encrypted (planned for future update)");
    Ok(result)
}

/// Disable database encryption (migrate back to unencrypted)
#[tauri::command]
pub async fn disable_database_encryption(
    app_handle: tauri::AppHandle,
    password: String,
) -> Result<usize, String> {
    crate::logger::log_info("Starting database decryption...");

    let db_path = database::get_database_path(&app_handle)
        .map_err(|e| {
            crate::logger::log_error(&format!("Failed to get database path: {}", e));
            e.to_string()
        })?;

    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| {
            crate::logger::log_error(&format!("Failed to open database: {}", e));
            e.to_string()
        })?;

    // Retrieve salt
    crate::logger::log_info("Retrieving encryption salt...");
    let salt_bytes = biometric::retrieve_secure("db_encryption_salt")
        .map_err(|e| {
            crate::logger::log_error(&format!("Failed to retrieve salt: {}", e));
            e.to_string()
        })?;

    let salt: [u8; 16] = salt_bytes.try_into()
        .map_err(|_| {
            crate::logger::log_error("Invalid salt format");
            "Invalid salt".to_string()
        })?;

    // Create encryption key
    crate::logger::log_info("Deriving encryption key from password...");
    let key = encryption::EncryptionKey::from_password(&password, &salt)
        .map_err(|e| {
            crate::logger::log_error(&format!("Failed to derive key: {}", e));
            e.to_string()
        })?;

    // Migrate data to unencrypted format
    crate::logger::log_info("Migrating database to unencrypted format...");
    let result = encrypted_database::migrate_to_unencrypted(&conn, &key)
        .map_err(|e| {
            crate::logger::log_error(&format!("Failed to migrate to unencrypted format: {}", e));
            e.to_string()
        })?;

    crate::logger::log_info(&format!("Database decryption completed successfully. Decrypted {} chunks", result));
    Ok(result)
}

// ============================================================================
// SECURITY & AUTHENTICATION COMMANDS
// ============================================================================

/// Get security configuration (safe to expose - no password hash)
#[tauri::command]
pub async fn get_security_config(
    app_handle: tauri::AppHandle,
) -> Result<crate::onboarding::SecurityConfig, String> {
    crate::onboarding::get_security_config(&app_handle)
        .map_err(|e| e.to_string())
}

/// Setup security with password during onboarding or settings
#[tauri::command(rename_all = "camelCase")]
pub async fn setup_security(
    app_handle: tauri::AppHandle,
    password: String,
    enable_biometric: bool,
) -> Result<(), String> {
    crate::logger::log_info("Setting up security...");

    crate::onboarding::setup_security(&app_handle, &password, enable_biometric)
        .map_err(|e| {
            crate::logger::log_error(&format!("Security setup failed: {}", e));
            e.to_string()
        })?;

    crate::logger::log_info("Security setup completed successfully");
    Ok(())
}

/// Verify password for unlock screen
#[tauri::command]
pub async fn verify_unlock_password(
    app_handle: tauri::AppHandle,
    password: String,
) -> Result<bool, String> {
    crate::onboarding::verify_unlock_password(&app_handle, &password)
        .map_err(|e| e.to_string())
}

/// Unlock with biometric (Touch ID / Windows Hello)
#[tauri::command]
pub async fn unlock_with_biometric(
    app_handle: tauri::AppHandle,
) -> Result<bool, String> {
    crate::logger::log_info("Attempting biometric unlock...");

    // First check if biometric is available and enabled
    let config = crate::onboarding::get_security_config(&app_handle)
        .map_err(|e| e.to_string())?;

    if !config.biometric_enabled {
        return Err("Biometric authentication is not enabled".to_string());
    }

    if !config.biometric_available {
        return Err("Biometric authentication is not available on this device".to_string());
    }

    // Authenticate with biometric
    let auth_result = biometric::authenticate_biometric("Unlock Enklayve")
        .map_err(|e| {
            crate::logger::log_error(&format!("Biometric auth failed: {}", e));
            e.to_string()
        })?;

    if !auth_result {
        return Ok(false);
    }

    // Biometric passed - the password is stored in keychain for biometric unlock
    // We don't need to return it, just verify it's accessible
    match biometric::retrieve_secure("enklayve_master_password") {
        Ok(_) => {
            crate::logger::log_info("Biometric unlock successful");
            Ok(true)
        }
        Err(e) => {
            crate::logger::log_error(&format!("Failed to retrieve password after biometric: {}", e));
            Err("Biometric authentication succeeded but could not retrieve credentials".to_string())
        }
    }
}

/// Disable security (requires current password)
#[tauri::command(rename_all = "camelCase")]
pub async fn disable_security(
    app_handle: tauri::AppHandle,
    current_password: String,
) -> Result<(), String> {
    crate::logger::log_info("Disabling security...");

    crate::onboarding::disable_security(&app_handle, &current_password)
        .map_err(|e| {
            crate::logger::log_error(&format!("Failed to disable security: {}", e));
            e.to_string()
        })?;

    crate::logger::log_info("Security disabled successfully");
    Ok(())
}

/// Change password (requires current password)
#[tauri::command(rename_all = "camelCase")]
pub async fn change_password(
    app_handle: tauri::AppHandle,
    current_password: String,
    new_password: String,
) -> Result<(), String> {
    crate::logger::log_info("Changing password...");

    crate::onboarding::change_password(&app_handle, &current_password, &new_password)
        .map_err(|e| {
            crate::logger::log_error(&format!("Password change failed: {}", e));
            e.to_string()
        })?;

    crate::logger::log_info("Password changed successfully");
    Ok(())
}

/// Skip security setup during onboarding
#[tauri::command]
pub async fn skip_security_setup(
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    crate::logger::log_info("User skipped security setup during onboarding");
    // Nothing to do - security_enabled remains false by default
    Ok(())
}

/// Toggle biometric authentication
#[tauri::command(rename_all = "camelCase")]
pub async fn toggle_biometric(
    app_handle: tauri::AppHandle,
    current_password: String,
    enable: bool,
) -> Result<(), String> {
    crate::onboarding::toggle_biometric(&app_handle, &current_password, enable)
        .map_err(|e| {
            crate::logger::log_error(&format!("Failed to toggle biometric: {}", e));
            e.to_string()
        })
}

// ============================================================================
// CONVERSATION HISTORY COMMANDS
// ============================================================================

/// Create a new conversation
#[tauri::command]
pub async fn create_conversation(
    app_handle: tauri::AppHandle,
    title: Option<String>,
) -> Result<i64, String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    conversations::create_conversation(&conn, title.as_deref())
        .map_err(|e| e.to_string())
}

/// List all conversations
#[tauri::command]
pub async fn list_conversations(
    app_handle: tauri::AppHandle,
    limit: Option<i32>,
) -> Result<Vec<conversations::ConversationSummary>, String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    conversations::list_conversations(&conn, limit)
        .map_err(|e| e.to_string())
}

/// Get a specific conversation
#[tauri::command]
pub async fn get_conversation(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> Result<conversations::Conversation, String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    conversations::get_conversation(&conn, conversation_id)
        .map_err(|e| e.to_string())
}

/// Get all messages in a conversation
#[tauri::command]
pub async fn get_conversation_messages(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> Result<Vec<conversations::Message>, String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    conversations::get_conversation_messages(&conn, conversation_id)
        .map_err(|e| e.to_string())
}

/// Add a message to a conversation
#[tauri::command]
pub async fn add_message(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    role: String,
    content: String,
    tokens: Option<i32>,
) -> Result<i64, String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    conversations::add_message(&conn, conversation_id, &role, &content, tokens)
        .map_err(|e| e.to_string())
}

/// Delete a conversation
#[tauri::command]
pub async fn delete_conversation(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> Result<(), String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    conversations::delete_conversation(&conn, conversation_id)
        .map_err(|e| e.to_string())
}

/// Delete a message
#[tauri::command]
pub async fn delete_message(
    app_handle: tauri::AppHandle,
    message_id: i64,
) -> Result<(), String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    conversations::delete_message(&conn, message_id)
        .map_err(|e| e.to_string())
}

/// Update conversation title
#[tauri::command]
pub async fn update_conversation_title(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    title: String,
) -> Result<(), String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    conversations::update_conversation_title(&conn, conversation_id, &title)
        .map_err(|e| e.to_string())
}

/// Search conversations
#[tauri::command]
pub async fn search_conversations(
    app_handle: tauri::AppHandle,
    query: String,
    limit: i32,
) -> Result<Vec<conversations::ConversationSummary>, String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    conversations::search_conversations(&conn, &query, limit)
        .map_err(|e| e.to_string())
}

/// Export conversation to Markdown
#[tauri::command]
pub async fn export_conversation_markdown(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> Result<String, String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    conversations::export_conversation_markdown(&conn, conversation_id)
        .map_err(|e| e.to_string())
}

/// Export conversation to JSON
#[tauri::command]
pub async fn export_conversation_json(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> Result<String, String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    conversations::export_conversation_json(&conn, conversation_id)
        .map_err(|e| e.to_string())
}

/// Export conversation to plain text
#[tauri::command]
pub async fn export_conversation_text(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> Result<String, String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    conversations::export_conversation_text(&conn, conversation_id)
        .map_err(|e| e.to_string())
}

// ============================================================================
// SETTINGS COMMANDS
// ============================================================================

/// Get application settings
#[tauri::command]
pub async fn get_settings(
    app_handle: tauri::AppHandle,
) -> Result<settings::AppSettings, String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    settings::load_settings(&conn)
        .map_err(|e| e.to_string())
}

/// Save application settings
#[tauri::command]
pub async fn save_settings(
    app_handle: tauri::AppHandle,
    app_settings: settings::AppSettings,
) -> Result<(), String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    settings::save_settings(&conn, &app_settings)
        .map_err(|e| e.to_string())
}

/// Reset settings to defaults
#[tauri::command]
pub async fn reset_settings(
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    settings::reset_settings(&conn)
        .map_err(|e| e.to_string())
}

/// Export settings to JSON
#[tauri::command]
pub async fn export_settings(
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    let app_settings = settings::load_settings(&conn)
        .map_err(|e| e.to_string())?;

    settings::export_settings_json(&app_settings)
        .map_err(|e| e.to_string())
}

/// Import settings from JSON
#[tauri::command]
pub async fn import_settings(
    app_handle: tauri::AppHandle,
    json: String,
) -> Result<(), String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    let app_settings = settings::import_settings_json(&json)
        .map_err(|e| e.to_string())?;

    settings::save_settings(&conn, &app_settings)
        .map_err(|e| e.to_string())
}

/// Get the current display mode
#[tauri::command]
pub async fn get_display_mode(
    app_handle: tauri::AppHandle,
) -> Result<settings::DisplayMode, String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    settings::get_display_mode(&conn)
        .map_err(|e| e.to_string())
}

/// Set the display mode
#[tauri::command]
pub async fn set_display_mode(
    app_handle: tauri::AppHandle,
    mode: settings::DisplayMode,
) -> Result<(), String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    settings::set_display_mode(&conn, mode)
        .map_err(|e| e.to_string())
}

/// Apply hardware-based auto-tuning to settings
#[tauri::command]
pub async fn apply_auto_tuning(
    app_handle: tauri::AppHandle,
) -> Result<settings::AppSettings, String> {
    let conn = database::get_connection(&app_handle)
        .map_err(|e| e.to_string())?;

    let hardware = HardwareProfile::detect()
        .map_err(|e| e.to_string())?;

    settings::load_settings_with_auto_tuning(&conn, &hardware)
        .map_err(|e| e.to_string())
}

// ============================================================================
// EXPORT COMMANDS
// ============================================================================

/// Export all conversations to a single ZIP file
#[tauri::command]
pub async fn export_all_conversations(
    app_handle: tauri::AppHandle,
    destination_path: String,
) -> Result<String, String> {
    let export_manager = crate::export::ExportManager::new(app_handle);
    let destination = std::path::Path::new(&destination_path);

    let export_path = export_manager.export_all_conversations(destination)
        .await
        .map_err(|e| e.to_string())?;

    Ok(export_path.to_string_lossy().to_string())
}

/// Export a conversation with embedded source documents
#[tauri::command]
pub async fn export_conversation_with_sources(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    destination_path: String,
) -> Result<String, String> {
    let export_manager = crate::export::ExportManager::new(app_handle);
    let destination = std::path::Path::new(&destination_path);

    let export_path = export_manager.export_conversation_with_sources(conversation_id, destination)
        .await
        .map_err(|e| e.to_string())?;

    Ok(export_path.to_string_lossy().to_string())
}

// ============================================================================
// BACKUP COMMANDS
// ============================================================================

/// Create a full backup of all user data
#[tauri::command]
pub async fn create_backup(
    app_handle: tauri::AppHandle,
    destination_path: String,
) -> Result<String, String> {
    let backup_manager = crate::backup::BackupManager::new(app_handle);
    let destination = std::path::Path::new(&destination_path);

    let backup_path = backup_manager.create_backup(destination)
        .await
        .map_err(|e| e.to_string())?;

    Ok(backup_path.to_string_lossy().to_string())
}

/// Restore from a backup file
#[tauri::command]
pub async fn restore_backup(
    app_handle: tauri::AppHandle,
    backup_path: String,
) -> Result<(), String> {
    let backup_manager = crate::backup::BackupManager::new(app_handle);
    let path = std::path::Path::new(&backup_path);

    backup_manager.restore_backup(path)
        .await
        .map_err(|e| e.to_string())
}

/// List available backups in a directory
#[tauri::command]
pub fn list_backups(directory_path: String) -> Result<Vec<crate::backup::BackupInfo>, String> {
    let directory = std::path::Path::new(&directory_path);

    crate::backup::BackupManager::list_backups(directory)
        .map_err(|e| e.to_string())
}

// ============================================================================
// LOGGING COMMANDS
// ============================================================================

/// Log a message from the frontend to the application logs
#[tauri::command]
pub fn log_from_frontend(message: String) {
    crate::logger::log_info(&format!("[FRONTEND] {}", message));
}

/// Get the log file directory path
#[tauri::command]
pub fn get_log_path(app_handle: tauri::AppHandle) -> Result<String, String> {
    crate::logger::get_log_path(&app_handle)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

// ============================================================================
// ONBOARDING COMMANDS
// ============================================================================

/// Check if this is the first run of the application
#[tauri::command]
pub fn check_first_run(app_handle: tauri::AppHandle) -> Result<crate::onboarding::OnboardingState, String> {
    crate::onboarding::get_onboarding_state(&app_handle)
        .map_err(|e| e.to_string())
}

/// Mark onboarding as completed
#[tauri::command]
pub fn complete_onboarding(app_handle: tauri::AppHandle) -> Result<(), String> {
    crate::onboarding::mark_onboarding_completed(&app_handle)
        .map_err(|e| e.to_string())
}

/// Mark recommended model as downloaded
#[tauri::command]
pub fn mark_model_downloaded(app_handle: tauri::AppHandle) -> Result<(), String> {
    crate::onboarding::mark_model_downloaded(&app_handle)
        .map_err(|e| e.to_string())
}

/// Reset onboarding state (for testing)
#[tauri::command]
pub fn reset_onboarding(app_handle: tauri::AppHandle) -> Result<(), String> {
    crate::onboarding::reset_onboarding(&app_handle)
        .map_err(|e| e.to_string())
}

/// Get the best model for the current hardware
#[tauri::command]
pub fn get_best_model(app_handle: tauri::AppHandle) -> Result<crate::model_selection::BestModelSelection, String> {
    let hardware = HardwareProfile::detect()
        .map_err(|e| e.to_string())?;

    let best_model = crate::model_selection::get_best_model_for_hardware(&hardware);

    Ok(best_model)
}

/// Get hardware summary in user-friendly format
#[tauri::command]
pub fn get_hardware_summary() -> Result<String, String> {
    let hardware = HardwareProfile::detect()
        .map_err(|e| e.to_string())?;

    Ok(hardware.get_hardware_summary())
}

/// Preload model in background
#[tauri::command]
pub fn preload_model(
    model_path: String,
    model_cache: tauri::State<'_, crate::model_cache::ModelCache>,
) -> Result<(), String> {
    model_cache.preload_model(model_path);
    Ok(())
}

/// Get preload status
#[tauri::command]
pub fn get_preload_status(
    model_cache: tauri::State<'_, crate::model_cache::ModelCache>,
) -> Result<String, String> {
    let status = model_cache.get_preload_status();
    Ok(format!("{:?}", status))
}

/// Cancel preload
#[tauri::command]
pub fn cancel_preload(
    model_cache: tauri::State<'_, crate::model_cache::ModelCache>,
) -> Result<(), String> {
    model_cache.cancel_preload();
    Ok(())
}

/// Invalidate prompt cache (call when documents change)
#[tauri::command]
pub fn invalidate_prompt_cache(
    model_cache: tauri::State<'_, crate::model_cache::ModelCache>,
) -> Result<(), String> {
    model_cache.invalidate_prompt_cache();
    Ok(())
}

/// Get prompt cache statistics
#[tauri::command]
pub fn get_prompt_cache_stats(
    model_cache: tauri::State<'_, crate::model_cache::ModelCache>,
) -> Result<serde_json::Value, String> {
    let (has_cache, hits, hit_rate) = model_cache.get_prompt_cache_stats();
    Ok(serde_json::json!({
        "enabled": has_cache,
        "hits": hits,
        "hit_rate": hit_rate
    }))
}

/// Stop ongoing generation
#[tauri::command]
pub fn stop_generation(
    model_cache: tauri::State<'_, crate::model_cache::ModelCache>,
) -> Result<(), String> {
    model_cache.stop_generation();
    Ok(())
}
