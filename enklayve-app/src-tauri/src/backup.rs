use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs;
use std::io::{Write as IoWrite, Read};
use zip::{ZipWriter, ZipArchive};
use zip::write::SimpleFileOptions;
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupManifest {
    pub version: String,
    pub backup_date: String,
    pub total_conversations: usize,
    pub total_documents: usize,
    pub total_chunks: usize,
    pub app_version: String,
}

pub struct BackupManager {
    app_handle: tauri::AppHandle,
}

impl BackupManager {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self { app_handle }
    }

    /// Create a full backup of all user data
    pub async fn create_backup(&self, destination_path: &Path) -> Result<PathBuf> {
        crate::logger::log_info("Starting full backup creation...");

        let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let backup_filename = format!("enklayve_backup_{}.zip", timestamp);
        let backup_path = destination_path.join(backup_filename);

        let file = fs::File::create(&backup_path)
            .context("Failed to create backup ZIP file")?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        // Get database path
        let db_path = crate::database::get_database_path(&self.app_handle)?;

        // Get database connection
        let conn = rusqlite::Connection::open(&db_path)
            .context("Failed to open database")?;

        // Count data for manifest
        let total_conversations: usize = conn.query_row(
            "SELECT COUNT(*) FROM conversations",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        let total_documents: usize = conn.query_row(
            "SELECT COUNT(*) FROM documents",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        let total_chunks: usize = conn.query_row(
            "SELECT COUNT(*) FROM chunks",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        crate::logger::log_info(&format!(
            "Backup includes: {} conversations, {} documents, {} chunks",
            total_conversations, total_documents, total_chunks
        ));

        // Flush the WAL into the main database file so the copied database.db is
        // complete and self-contained (WAL changes otherwise live in a side file).
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");

        // Export database
        zip.start_file("database.db", options)?;
        let mut db_file = fs::File::open(&db_path)?;
        std::io::copy(&mut db_file, &mut zip)?;
        crate::logger::log_info("Database backed up successfully");

        // Export documents directory
        let app_data_dir = self.app_handle.path().app_data_dir()
            .context("Failed to get app data directory")?;
        let documents_dir = app_data_dir.join("documents");

        if documents_dir.exists() {
            self.backup_directory(&mut zip, &documents_dir, "documents", options)?;
            crate::logger::log_info("Documents directory backed up successfully");
        }

        // Export settings as JSON
        let settings = crate::settings::load_settings(&conn)?;
        let settings_json = serde_json::to_string_pretty(&settings)?;
        zip.start_file("settings.json", options)?;
        zip.write_all(settings_json.as_bytes())?;
        crate::logger::log_info("Settings backed up successfully");

        // Create manifest
        let manifest = BackupManifest {
            version: "1.0".to_string(),
            backup_date: chrono::Local::now().to_rfc3339(),
            total_conversations,
            total_documents,
            total_chunks,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        };

        let manifest_json = serde_json::to_string_pretty(&manifest)?;
        zip.start_file("manifest.json", options)?;
        zip.write_all(manifest_json.as_bytes())?;
        crate::logger::log_info("Manifest created successfully");

        zip.finish()?;

        crate::logger::log_info(&format!("Backup created successfully: {:?}", backup_path));
        Ok(backup_path)
    }

    /// Recursively backup a directory to ZIP
    fn backup_directory(
        &self,
        zip: &mut ZipWriter<fs::File>,
        dir_path: &Path,
        prefix: &str,
        options: SimpleFileOptions,
    ) -> Result<()> {
        let entries = fs::read_dir(dir_path)
            .context(format!("Failed to read directory: {:?}", dir_path))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let zip_path = format!("{}/{}", prefix, name.to_string_lossy());

            if path.is_file() {
                zip.start_file(&zip_path, options)?;
                let mut file = fs::File::open(&path)?;
                std::io::copy(&mut file, zip)?;
            } else if path.is_dir() {
                self.backup_directory(zip, &path, &zip_path, options)?;
            }
        }

        Ok(())
    }

    /// Restore from backup ZIP file
    pub async fn restore_backup(&self, backup_path: &Path) -> Result<()> {
        crate::logger::log_info(&format!("Starting restore from backup: {:?}", backup_path));

        let file = fs::File::open(backup_path)
            .context("Failed to open backup file")?;
        let mut archive = zip::ZipArchive::new(file)
            .context("Failed to read backup ZIP")?;

        // Read and validate manifest
        let manifest_file = archive.by_name("manifest.json")
            .context("Backup manifest not found")?;
        let manifest: BackupManifest = serde_json::from_reader(manifest_file)
            .context("Failed to parse backup manifest")?;

        crate::logger::log_info(&format!(
            "Backup info: version={}, date={}, conversations={}, documents={}",
            manifest.version, manifest.backup_date, manifest.total_conversations, manifest.total_documents
        ));

        // Check version compatibility
        if manifest.version != "1.0" {
            return Err(anyhow::anyhow!("Unsupported backup version: {}", manifest.version));
        }

        // Get app data directory
        let app_data_dir = self.app_handle.path().app_data_dir()
            .context("Failed to get app data directory")?;
        fs::create_dir_all(&app_data_dir)?;

        // Restore database
        crate::logger::log_info("Restoring database...");
        let db_path = crate::database::get_database_path(&self.app_handle)?;

        // Backup existing database if it exists
        if db_path.exists() {
            let backup_db_path = db_path.with_extension("db.backup");
            fs::copy(&db_path, &backup_db_path)?;
            crate::logger::log_info(&format!("Existing database backed up to: {:?}", backup_db_path));
        }

        {
            let mut db_file = archive.by_name("database.db")
                .context("Database not found in backup")?;
            let mut output = fs::File::create(&db_path)?;
            std::io::copy(&mut db_file, &mut output)?;
            crate::logger::log_info("Database restored successfully");
        }

        // Restore documents directory
        let documents_dir = app_data_dir.join("documents");
        fs::create_dir_all(&documents_dir)?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let outpath = if let Some(path) = file.enclosed_name() {
                if path.starts_with("documents/") {
                    app_data_dir.join(path)
                } else {
                    continue;
                }
            } else {
                continue;
            };

            if file.name().ends_with('/') {
                fs::create_dir_all(&outpath)?;
            } else {
                if let Some(parent) = outpath.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut outfile = fs::File::create(&outpath)?;
                std::io::copy(&mut file, &mut outfile)?;
            }
        }

        crate::logger::log_info("Documents restored successfully");
        crate::logger::log_info("Backup restoration complete");

        Ok(())
    }

    /// List available backups in a directory
    pub fn list_backups(directory: &Path) -> Result<Vec<BackupInfo>> {
        let mut backups = Vec::new();

        if !directory.exists() {
            return Ok(backups);
        }

        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("zip") {
                if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                    if filename.starts_with("enklayve_backup_") {
                        if let Ok(metadata) = fs::metadata(&path) {
                            if let Ok(modified) = metadata.modified() {
                                let info = BackupInfo {
                                    path: path.clone(),
                                    filename: filename.to_string(),
                                    size_bytes: metadata.len(),
                                    created: modified,
                                };
                                backups.push(info);
                            }
                        }
                    }
                }
            }
        }

        backups.sort_by(|a, b| b.created.cmp(&a.created));

        Ok(backups)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub filename: String,
    pub size_bytes: u64,
    #[serde(with = "systemtime_serde")]
    pub created: std::time::SystemTime,
}

mod systemtime_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn serialize<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let duration = time.duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| std::time::Duration::from_secs(0));
        serializer.serialize_u64(duration.as_secs())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + std::time::Duration::from_secs(secs))
    }
}
