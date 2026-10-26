use anyhow::{Result, Context};
use std::path::{Path, PathBuf};
use std::fs;
use ocrs::{OcrEngine, OcrEngineParams, ImageSource};
use image::DynamicImage;
use rten::Model;
use tauri::Emitter;

const DETECTION_MODEL_URL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten";
const RECOGNITION_MODEL_URL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten";

/// Get the directory where OCR models are cached
fn get_models_dir() -> Result<PathBuf> {
    let cache_dir = dirs::cache_dir()
        .context("Failed to get cache directory")?
        .join("enklayve")
        .join("ocr-models");

    fs::create_dir_all(&cache_dir)
        .context("Failed to create OCR models cache directory")?;

    Ok(cache_dir)
}

/// Download a model file if it doesn't exist in cache
async fn ensure_model_downloaded(url: &str, filename: &str) -> Result<PathBuf> {
    let models_dir = get_models_dir()?;
    let model_path = models_dir.join(filename);

    // If model already exists, return path
    if model_path.exists() {
        crate::logger::log_info(&format!("Using cached OCR model: {}", filename));
        return Ok(model_path);
    }

    // Download model
    crate::logger::log_info(&format!("Downloading OCR model: {} (first-time setup, ~10MB)", filename));

    let response = reqwest::get(url)
        .await
        .context("Failed to download OCR model")?;

    let bytes = response.bytes()
        .await
        .context("Failed to download OCR model bytes")?;

    fs::write(&model_path, &bytes)
        .context("Failed to save OCR model to cache")?;

    crate::logger::log_info(&format!("OCR model downloaded: {}", filename));

    Ok(model_path)
}

/// Check if OCR is available (always true with ocrs - pure Rust implementation)
pub fn is_tesseract_available() -> bool {
    true
}

/// Extract text from an image file (JPG, PNG) using OCR
pub async fn extract_text_from_image(image_path: &Path, app_handle: Option<&tauri::AppHandle>) -> Result<String> {
    crate::logger::log_info(&format!("Starting OCR processing for image: {:?}", image_path));

    // Emit progress event
    if let Some(app) = app_handle {
        let _ = app.emit("ocr-progress", serde_json::json!({
            "stage": "starting",
            "message": "Starting OCR processing...",
            "progress": 0
        }));
    }

    // Emit progress: downloading models
    if let Some(app) = app_handle {
        let _ = app.emit("ocr-progress", serde_json::json!({
            "stage": "downloading",
            "message": "Downloading OCR models...",
            "progress": 10
        }));
    }

    // Ensure models are downloaded
    let detection_model_path = ensure_model_downloaded(DETECTION_MODEL_URL, "text-detection.rten")
        .await
        .context("Failed to download text detection model")?;

    let recognition_model_path = ensure_model_downloaded(RECOGNITION_MODEL_URL, "text-recognition.rten")
        .await
        .context("Failed to download text recognition model")?;

    // Emit progress: loading models
    if let Some(app) = app_handle {
        let _ = app.emit("ocr-progress", serde_json::json!({
            "stage": "loading",
            "message": "Loading OCR models...",
            "progress": 30
        }));
    }

    // Load models
    crate::logger::log_info("Loading OCR models...");
    let detection_model = Model::load_file(&detection_model_path)
        .context("Failed to load detection model")?;

    let recognition_model = Model::load_file(&recognition_model_path)
        .context("Failed to load recognition model")?;

    // Create OCR engine
    crate::logger::log_info("Initializing OCR engine...");
    let engine = OcrEngine::new(OcrEngineParams {
        detection_model: Some(detection_model),
        recognition_model: Some(recognition_model),
        ..Default::default()
    })
    .context("Failed to initialize OCR engine")?;

    // Emit progress: processing image
    if let Some(app) = app_handle {
        let _ = app.emit("ocr-progress", serde_json::json!({
            "stage": "processing",
            "message": "Processing image...",
            "progress": 50
        }));
    }

    // Load image
    crate::logger::log_info("Loading image file...");
    let img = image::open(image_path)
        .context("Failed to load image file")?;

    // Convert to RGB8
    let rgb_image = img.to_rgb8();

    // Prepare image for OCR
    crate::logger::log_info("Preparing image for OCR...");
    let ocr_input = engine.prepare_input(ImageSource::from_bytes(
        rgb_image.as_raw(),
        rgb_image.dimensions(),
    )?)?;

    // Emit progress: detecting text
    if let Some(app) = app_handle {
        let _ = app.emit("ocr-progress", serde_json::json!({
            "stage": "detecting",
            "message": "Detecting text...",
            "progress": 70
        }));
    }

    // Run OCR
    crate::logger::log_info("Running text detection...");
    let word_rects = engine.detect_words(&ocr_input)?;

    crate::logger::log_info("Grouping words into lines...");
    let line_rects = engine.find_text_lines(&ocr_input, &word_rects);

    // Emit progress: recognizing text
    if let Some(app) = app_handle {
        let _ = app.emit("ocr-progress", serde_json::json!({
            "stage": "recognizing",
            "message": "Recognizing text...",
            "progress": 90
        }));
    }

    crate::logger::log_info("Recognizing text...");
    let line_texts = engine.recognize_text(&ocr_input, &line_rects)?;

    // Extract text from results
    let mut all_text = String::new();
    for line_opt in line_texts.iter() {
        if let Some(line) = line_opt {
            all_text.push_str(&line.to_string());
            all_text.push('\n');
        }
    }

    if all_text.trim().is_empty() {
        anyhow::bail!("OCR did not extract any text from the image. The image quality may be too low, or the image may not contain readable text.");
    }

    // Emit completion event
    if let Some(app) = app_handle {
        let _ = app.emit("ocr-progress", serde_json::json!({
            "stage": "complete",
            "message": "OCR complete!",
            "progress": 100
        }));
    }

    crate::logger::log_info(&format!("✅ OCR completed! Extracted {} characters from image", all_text.len()));

    Ok(all_text)
}

/// Extract text from a scanned PDF using OCR
/// This function converts PDF pages to images and runs OCR on them
pub async fn extract_text_from_scanned_pdf(pdf_path: &Path, app_handle: Option<&tauri::AppHandle>) -> Result<String> {
    crate::logger::log_info(&format!("Starting OCR processing for: {:?}", pdf_path));

    let emit = |stage: &str, message: String, progress: u32| {
        if let Some(app) = app_handle {
            let _ = app.emit("ocr-progress", serde_json::json!({
                "stage": stage, "message": message, "progress": progress,
            }));
        }
    };
    emit("starting", "Reading scanned document…".to_string(), 2);

    // Ensure models are downloaded
    let detection_model_path = ensure_model_downloaded(DETECTION_MODEL_URL, "text-detection.rten")
        .await
        .context("Failed to download text detection model")?;

    let recognition_model_path = ensure_model_downloaded(RECOGNITION_MODEL_URL, "text-recognition.rten")
        .await
        .context("Failed to download text recognition model")?;

    // Load models
    crate::logger::log_info("Loading OCR models...");
    let detection_model = Model::load_file(&detection_model_path)
        .context("Failed to load detection model")?;

    let recognition_model = Model::load_file(&recognition_model_path)
        .context("Failed to load recognition model")?;

    // Create OCR engine (shared across threads for parallel page recognition)
    crate::logger::log_info("Initializing OCR engine...");
    let engine = std::sync::Arc::new(
        OcrEngine::new(OcrEngineParams {
            detection_model: Some(detection_model),
            recognition_model: Some(recognition_model),
            ..Default::default()
        })
        .context("Failed to initialize OCR engine")?,
    );

    // Convert PDF pages to images (emits its own "rendering" progress per page)
    let images = pdf_to_images(pdf_path, app_handle)
        .context("Failed to convert PDF to images")?;

    let total = images.len();
    crate::logger::log_info(&format!("Processing {} page(s) with OCR...", total));

    // Recognize every page in parallel across CPU cores — recognition is the slow
    // half of OCR and each page is independent. collect() preserves page order;
    // the atomic counter drives the progress bar as pages finish.
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let done = std::sync::Arc::new(AtomicUsize::new(0));
    let page_texts = images
        .par_iter()
        .map(|img| -> Result<String> {
            let rgb_image = img.to_rgb8();
            let ocr_input = engine.prepare_input(ImageSource::from_bytes(
                rgb_image.as_raw(),
                rgb_image.dimensions(),
            )?)?;
            let word_rects = engine.detect_words(&ocr_input)?;
            let line_rects = engine.find_text_lines(&ocr_input, &word_rects);
            let line_texts = engine.recognize_text(&ocr_input, &line_rects)?;

            let mut s = String::new();
            for line_opt in line_texts.iter() {
                if let Some(line) = line_opt {
                    s.push_str(&line.to_string());
                    s.push('\n');
                }
            }

            let n = done.fetch_add(1, Ordering::SeqCst) + 1;
            let progress = 50 + ((n as f32 / total.max(1) as f32) * 45.0) as u32;
            if let Some(app) = app_handle {
                let _ = app.emit("ocr-progress", serde_json::json!({
                    "stage": "recognizing",
                    "message": format!("Read {} of {} pages…", n, total),
                    "progress": progress,
                }));
            }
            Ok(s)
        })
        .collect::<Result<Vec<String>>>()?;

    let all_text = page_texts.join("\n--- Page Break ---\n\n");

    if all_text.trim().is_empty() {
        anyhow::bail!("OCR did not extract any text from the PDF. The image quality may be too low, or the document may not contain readable text.");
    }

    emit("complete", "Scanned document read.".to_string(), 100);
    crate::logger::log_info(&format!("OCR completed! Extracted {} characters", all_text.len()));

    Ok(all_text)
}

/// Convert PDF pages to images for OCR processing
fn pdf_to_images(pdf_path: &Path, app_handle: Option<&tauri::AppHandle>) -> Result<Vec<DynamicImage>> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use rayon::prelude::*;

    crate::logger::log_info("Rendering PDF pages to images using hayro...");

    // Read PDF file bytes
    let pdf_bytes = std::fs::read(pdf_path)
        .context("Failed to read PDF file")?;

    // Parse PDF (hayro requires Arc<dyn AsRef<[u8]> + Send + Sync>)
    let pdf_data: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(pdf_bytes);
    let pdf = hayro::Pdf::new(pdf_data)
        .map_err(|e| anyhow::anyhow!("Failed to parse PDF: {:?}", e))?;

    let pages = pdf.pages();
    let page_count = pages.len();
    crate::logger::log_info(&format!("PDF has {} pages", page_count));

    if page_count == 0 {
        anyhow::bail!("PDF has no pages to process");
    }

    // Scale 2.0 ≈ 144 DPI: enough for clean OCR of typed text and numbers while
    // keeping render time down.
    let render_settings = hayro::RenderSettings {
        x_scale: 2.0,
        y_scale: 2.0,
        width: None,
        height: None,
    };

    let interpreter_settings = hayro::InterpreterSettings::default();

    // Rendering is the slow half of scanned-PDF OCR, and every page is
    // independent, so render them across all CPU cores. collect() keeps page
    // order; the counter drives progress as pages finish (not necessarily in
    // order, which is fine for a progress bar).
    let rendered = Arc::new(AtomicUsize::new(0));
    let images: Vec<DynamicImage> = pages
        .par_iter()
        .map(|page| {
            let pixmap = hayro::render(page, &interpreter_settings, &render_settings);
            let width = pixmap.width() as u32;
            let height = pixmap.height() as u32;
            let pixels = pixmap.data_as_u8_slice();
            let img_buffer = image::ImageBuffer::from_fn(width, height, |x, y| {
                let offset = ((y * width + x) * 4) as usize;
                if offset + 3 < pixels.len() {
                    image::Rgba([
                        pixels[offset],
                        pixels[offset + 1],
                        pixels[offset + 2],
                        pixels[offset + 3],
                    ])
                } else {
                    image::Rgba([255, 255, 255, 255]) // White fallback
                }
            });
            let n = rendered.fetch_add(1, Ordering::SeqCst) + 1;
            if let Some(app) = app_handle {
                let progress = 5 + ((n as f32 / page_count.max(1) as f32) * 45.0) as u32;
                let _ = app.emit("ocr-progress", serde_json::json!({
                    "stage": "rendering",
                    "message": format!("Rendering page {} of {}…", n, page_count),
                    "progress": progress,
                }));
            }
            DynamicImage::ImageRgba8(img_buffer)
        })
        .collect();

    crate::logger::log_info(&format!("Successfully rendered {} pages to images", images.len()));

    Ok(images)
}
