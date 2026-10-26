use anyhow::{Result, Context};
use reqwest::Client;
use sha2::{Sha256, Digest};
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use futures::StreamExt;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DownloadProgress {
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub percentage: f32,
    pub speed_mbps: f32,
    pub estimated_seconds_remaining: u64,
    pub speed_mb_per_sec: f32,
}

pub struct ModelDownloader {
    client: Client,
}

impl ModelDownloader {
    pub fn new() -> Result<Self> {
        // Model files are multi-gigabyte, so we must NOT set an overall request
        // timeout (that would cap the whole download and abort large models).
        // Instead bound the connect phase and detect stalled transfers with a
        // per-read inactivity timeout, which allows arbitrarily long downloads
        // as long as bytes keep arriving.
        let client = Client::builder()
            .user_agent(concat!("Enklayve/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(30))
            .read_timeout(std::time::Duration::from_secs(120))
            .build()?;

        Ok(Self { client })
    }

    pub async fn download_model(
        &self,
        url: &str,
        model_name: &str,
        expected_checksum: Option<&str>,
        app_handle: &AppHandle,
        progress_callback: impl Fn(DownloadProgress) + Send + Sync + 'static,
    ) -> Result<PathBuf> {
        use tokio::fs::OpenOptions;
        use reqwest::header::{RANGE, CONTENT_RANGE};

        let models_dir = get_models_directory(app_handle)?;
        std::fs::create_dir_all(&models_dir)?;

        let file_path = models_dir.join(model_name);

        // Already have a complete, valid model? Use it.
        if file_path.exists() {
            if is_valid_model_file(&file_path) {
                crate::logger::log_info(&format!(
                    "Valid model file already exists: {} ({} MB)",
                    model_name,
                    std::fs::metadata(&file_path)?.len() / 1_000_000
                ));
                return Ok(file_path);
            }
            crate::logger::log_warn(&format!(
                "Found corrupted model file {}, re-downloading",
                model_name
            ));
            let _ = std::fs::remove_file(&file_path);
        }

        // Partial downloads accumulate in a .part file so an interrupted
        // download can resume instead of starting over.
        let part_path = models_dir.join(format!("{}.part", model_name));

        // A .part must still look like a GGUF from byte 0; otherwise it is a
        // stale error page or garbage, and we start fresh.
        let mut resume_from: u64 = match std::fs::metadata(&part_path) {
            Ok(m) if m.len() > 0 && starts_with_gguf_magic(&part_path) => m.len(),
            Ok(_) => {
                let _ = std::fs::remove_file(&part_path);
                0
            }
            Err(_) => 0,
        };

        // Convert a HuggingFace repo URL into a direct download URL.
        let download_url = if url.contains("huggingface.co") && !url.contains("/resolve/") {
            format!("{}/resolve/main/{}", url.trim_end_matches('/'), model_name)
        } else {
            url.to_string()
        };

        crate::logger::log_info(&format!(
            "Downloading {} from {} (resuming at {} bytes)",
            model_name, download_url, resume_from
        ));

        let mut request = self.client.get(&download_url);
        if resume_from > 0 {
            request = request.header(RANGE, format!("bytes={}-", resume_from));
        }
        let response = request.send().await.context("Failed to initiate download")?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("Download failed with status: {}", status);
        }

        // Decide the total size and whether the server honored our resume.
        let (total_size, append) = if resume_from > 0 && status.as_u16() == 206 {
            // Partial content: content-length is the REMAINING bytes. Prefer the
            // authoritative total from Content-Range when present.
            let total = response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.rsplit('/').next())
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or_else(|| resume_from + response.content_length().unwrap_or(0));
            (total, true)
        } else {
            // Server ignored the range (or there was nothing to resume): restart.
            if resume_from > 0 {
                crate::logger::log_warn("Server ignored resume request; restarting download");
                resume_from = 0;
            }
            (response.content_length().unwrap_or(0), false)
        };

        let mut file = if append {
            OpenOptions::new()
                .append(true)
                .open(&part_path)
                .await
                .context("Failed to open partial file for resume")?
        } else {
            File::create(&part_path)
                .await
                .context("Failed to create download file")?
        };

        // Stream to disk with throttled progress updates.
        let download_result: Result<u64> = async {
            let mut stream = response.bytes_stream();
            let mut downloaded: u64 = resume_from;
            let start_bytes = resume_from;
            let start_time = std::time::Instant::now();
            let mut last_update = start_time;

            while let Some(chunk) = stream.next().await {
                let chunk = chunk.context("Failed to read chunk")?;
                file.write_all(&chunk).await.context("Failed to write chunk")?;
                downloaded += chunk.len() as u64;

                let now = std::time::Instant::now();
                if now.duration_since(last_update).as_millis() > 500 {
                    let elapsed = now.duration_since(start_time).as_secs_f32();
                    let speed_mb_per_sec = if elapsed > 0.0 {
                        ((downloaded - start_bytes) as f32 / 1_048_576.0) / elapsed
                    } else {
                        0.0
                    };
                    let speed_mbps = speed_mb_per_sec * 8.0;
                    let percentage = if total_size > 0 {
                        (downloaded as f32 / total_size as f32) * 100.0
                    } else {
                        0.0
                    };
                    let bytes_remaining = total_size.saturating_sub(downloaded);
                    let estimated_seconds_remaining = if speed_mb_per_sec > 0.0 {
                        (bytes_remaining as f32 / 1_048_576.0 / speed_mb_per_sec) as u64
                    } else {
                        0
                    };

                    progress_callback(DownloadProgress {
                        total_bytes: total_size,
                        downloaded_bytes: downloaded,
                        percentage,
                        speed_mbps,
                        estimated_seconds_remaining,
                        speed_mb_per_sec,
                    });

                    last_update = now;
                }
            }

            file.flush().await?;
            Ok(downloaded)
        }.await;

        // On a mid-stream error, KEEP the .part file so the next attempt resumes.
        let downloaded = match download_result {
            Ok(bytes) => bytes,
            Err(e) => {
                crate::logger::log_warn(&format!(
                    "Download interrupted ({}); keeping partial file to resume later",
                    e
                ));
                let _ = file.flush().await;
                return Err(e);
            }
        };

        // A dropped connection can end the stream early without erroring. Keep
        // the partial file and report a resumable error.
        if total_size > 0 && downloaded != total_size {
            crate::logger::log_warn(&format!(
                "Download incomplete: {} of {} bytes; will resume on next attempt",
                downloaded, total_size
            ));
            anyhow::bail!(
                "Download was interrupted ({} of {} bytes). It will resume where it \
                 left off when you try again.",
                downloaded, total_size
            );
        }

        // Completed: validate the finished file before promoting it.
        if !is_valid_model_file(&part_path) {
            crate::logger::log_error("Downloaded file failed the integrity check");
            let _ = std::fs::remove_file(&part_path);
            anyhow::bail!("The downloaded model was corrupted. Please try again.");
        }

        // Optional cryptographic verification when a checksum is known.
        if let Some(expected) = expected_checksum.filter(|c| !c.is_empty()) {
            match self.verify_checksum(&part_path, expected).await {
                Ok(true) => crate::logger::log_info("Model checksum verified"),
                Ok(false) => {
                    let _ = std::fs::remove_file(&part_path);
                    anyhow::bail!(
                        "The downloaded model failed checksum verification. Please try again."
                    );
                }
                Err(e) => crate::logger::log_warn(&format!(
                    "Could not verify checksum ({}); relying on the integrity check",
                    e
                )),
            }
        }

        // Promote the completed .part to its final name.
        tokio::fs::rename(&part_path, &file_path)
            .await
            .context("Failed to finalize the downloaded model")?;

        crate::logger::log_info(&format!(
            "Download completed successfully: {} ({} MB)",
            model_name,
            std::fs::metadata(&file_path)?.len() / 1_000_000
        ));

        progress_callback(DownloadProgress {
            total_bytes: total_size,
            downloaded_bytes: downloaded,
            percentage: 100.0,
            speed_mbps: 0.0,
            estimated_seconds_remaining: 0,
            speed_mb_per_sec: 0.0,
        });

        Ok(file_path)
    }

    pub async fn verify_checksum(
        &self,
        file_path: &Path,
        expected_checksum: &str,
    ) -> Result<bool> {
        let mut file = File::open(file_path).await?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 8192];

        loop {
            let n = tokio::io::AsyncReadExt::read(&mut file, &mut buffer).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }

        let result = hasher.finalize();
        let computed = hex::encode(result);

        Ok(computed.eq_ignore_ascii_case(expected_checksum))
    }

    pub async fn get_download_info(&self, url: &str) -> Result<DownloadInfo> {
        let response = self.client.head(url).send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to get download info: {}", response.status());
        }

        let size = response
            .content_length()
            .context("Failed to get content length")?;

        Ok(DownloadInfo {
            size_bytes: size,
            size_mb: (size as f64 / 1_048_576.0),
            size_gb: (size as f64 / 1_073_741_824.0),
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DownloadInfo {
    pub size_bytes: u64,
    pub size_mb: f64,
    pub size_gb: f64,
}

pub fn get_models_directory(app_handle: &AppHandle) -> Result<PathBuf> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get app data directory: {}", e))?;

    Ok(app_data_dir.join("models"))
}

pub async fn list_downloaded_models(app_handle: &AppHandle) -> Result<Vec<DownloadedModelInfo>> {
    let models_dir = get_models_directory(app_handle)?;
    crate::logger::log_info(&format!("Checking for models in: {}", models_dir.display()));

    if !models_dir.exists() {
        crate::logger::log_warn("Models directory does not exist");
        return Ok(Vec::new());
    }

    let mut models = Vec::new();
    let mut entries = tokio::fs::read_dir(&models_dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        crate::logger::log_info(&format!("Found file: {}", path.display()));

        if path.is_file() {
            if let Some(extension) = path.extension() {
                crate::logger::log_info(&format!("File extension: {:?}", extension));
                if extension == "gguf" {
                    // Validate model file is not corrupted (size > 100 MB)
                    if !is_valid_model_file(&path) {
                        crate::logger::log_warn(&format!(
                            "Skipping invalid/corrupted model file: {} (size: 0 or too small)",
                            path.display()
                        ));
                        continue;
                    }

                    let metadata = entry.metadata().await?;
                    let file_name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();

                    crate::logger::log_info(&format!("Adding model to list: {} (size: {} bytes)", file_name, metadata.len()));

                    models.push(DownloadedModelInfo {
                        name: file_name.clone(),
                        path: path.to_string_lossy().to_string(),
                        size_bytes: metadata.len(),
                        size_gb: metadata.len() as f64 / 1_073_741_824.0,
                    });
                }
            }
        }
    }

    crate::logger::log_info(&format!("Total models found: {}", models.len()));
    Ok(models)
}

/// Does the file begin with the GGUF magic bytes? Used to decide whether a
/// leftover `.part` file is a real partial model worth resuming.
fn starts_with_gguf_magic(path: &Path) -> bool {
    use std::io::Read;
    match std::fs::File::open(path) {
        Ok(mut f) => {
            let mut magic = [0u8; 4];
            f.read_exact(&mut magic).is_ok() && &magic == b"GGUF"
        }
        Err(_) => false,
    }
}

/// Validate that a model file is not corrupted.
/// A valid file must be at least 100 MB (smallest quantized model) and begin
/// with the GGUF magic bytes, which rejects HTML/JSON error responses that were
/// mistakenly saved with a `.gguf` name.
pub fn is_valid_model_file(file_path: &Path) -> bool {
    if !file_path.exists() {
        return false;
    }

    const MIN_MODEL_SIZE: u64 = 100_000_000; // 100 MB
    match std::fs::metadata(file_path) {
        Ok(metadata) if metadata.len() >= MIN_MODEL_SIZE => {}
        _ => return false,
    }

    // GGUF files start with the ASCII magic "GGUF".
    use std::io::Read;
    const GGUF_MAGIC: &[u8; 4] = b"GGUF";
    match std::fs::File::open(file_path) {
        Ok(mut file) => {
            let mut magic = [0u8; 4];
            file.read_exact(&mut magic).is_ok() && &magic == GGUF_MAGIC
        }
        Err(_) => false,
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DownloadedModelInfo {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub size_gb: f64,
}

pub async fn delete_model(app_handle: &AppHandle, model_name: &str) -> Result<()> {
    let models_dir = get_models_directory(app_handle)?;
    let model_path = models_dir.join(model_name);

    if model_path.exists() {
        tokio::fs::remove_file(model_path).await?;
    }

    Ok(())
}
