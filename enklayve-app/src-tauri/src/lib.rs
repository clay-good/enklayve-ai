// Core modules
mod logger;
mod database;
mod documents;
mod models;
mod inference;
mod response_format;
mod index_health;
mod web;
mod fs_sandbox;
mod downloads;
mod embeddings;
mod vector_search;
mod hardware;
mod encryption;
mod biometric;
mod encrypted_database;
mod conversations;
mod settings;
mod model_cache;
mod ocr;
mod onboarding;
mod model_selection;
mod reranker;
mod citations;
mod backup;
mod export;

// Tauri commands
mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize model cache
    let model_cache = model_cache::ModelCache::new();

    tauri::Builder::default()
        .manage(model_cache)
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::upload_document,
            commands::list_documents,
            commands::delete_document,
            commands::get_models,
            commands::list_downloaded_models,
            commands::get_download_info,
            commands::download_model,
            commands::delete_model,
            commands::query_documents,
            commands::query_documents_streaming,
            commands::detect_hardware,
            commands::get_model_recommendations,
            commands::hash_password,
            commands::verify_password,
            commands::encrypt_data,
            commands::decrypt_data,
            commands::check_biometric_available,
            commands::authenticate_biometric,
            commands::store_secure_data,
            commands::retrieve_secure_data,
            commands::get_encryption_stats,
            commands::enable_database_encryption,
            commands::disable_database_encryption,
            commands::get_security_config,
            commands::setup_security,
            commands::verify_unlock_password,
            commands::unlock_with_biometric,
            commands::disable_security,
            commands::change_password,
            commands::skip_security_setup,
            commands::toggle_biometric,
            commands::create_conversation,
            commands::list_conversations,
            commands::get_conversation,
            commands::get_conversation_messages,
            commands::add_message,
            commands::delete_conversation,
            commands::delete_message,
            commands::update_conversation_title,
            commands::search_conversations,
            commands::search_archived_conversations,
            commands::export_conversation_markdown,
            commands::export_conversation_json,
            commands::export_conversation_text,
            commands::get_settings,
            commands::save_settings,
            commands::reset_settings,
            commands::export_settings,
            commands::import_settings,
            commands::get_display_mode,
            commands::set_display_mode,
            commands::apply_auto_tuning,
            commands::export_all_conversations,
            commands::export_conversation_with_sources,
            commands::create_backup,
            commands::restore_backup,
            commands::list_backups,
            commands::log_from_frontend,
            commands::get_log_path,
            commands::check_first_run,
            commands::complete_onboarding,
            commands::mark_model_downloaded,
            commands::reset_onboarding,
            commands::get_best_model,
            commands::get_active_model,
            commands::set_active_model,
            commands::list_model_options,
            commands::get_document_passages,
            commands::upload_folder,
            commands::check_index_integrity,
            commands::rebuild_index,
            commands::run_startup_recovery,
            commands::generate_chat_title,
            commands::list_archived_conversations,
            commands::set_conversation_archived,
            commands::get_web_access,
            commands::set_web_access,
            commands::get_working_folder,
            commands::set_working_folder,
            commands::save_to_working_folder,
            commands::get_hardware_summary,
            commands::preload_model,
            commands::get_preload_status,
            commands::cancel_preload,
            commands::invalidate_prompt_cache,
            commands::get_prompt_cache_stats,
            commands::stop_generation,
        ])
        .setup(|app| {
            // Initialize logger first
            let app_handle = app.handle().clone();
            if let Err(e) = logger::init_logger(&app_handle) {
                eprintln!("Failed to initialize logger: {}", e);
            }
            logger::log_info("Enklayve application starting...");

            // Initialize database on startup
            let app_handle_db = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                logger::log_info("Initializing database...");
                if let Err(e) = database::init_database(&app_handle_db).await {
                    logger::log_error(&format!("Failed to initialize database: {}", e));
                    eprintln!("Failed to initialize database: {}", e);
                } else {
                    logger::log_info("Database initialized successfully");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
