use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use calamine::{Reader, open_workbook_auto};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMetadata {
    pub id: i64,
    pub file_name: String,
    pub file_path: String,
    pub file_type: String,
    pub upload_date: i64,
    pub size_bytes: i64,
    pub chunks_count: usize,
    pub title: Option<String>,
    pub author: Option<String>,
    pub creation_date: Option<i64>,
    pub page_count: Option<i64>,
    pub word_count: Option<i64>,
    pub conversation_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub metadata: DocumentMetadata,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentChunk {
    pub id: i64,
    pub document_id: i64,
    pub chunk_text: String,
    pub chunk_index: i64,
    pub page_number: Option<i64>,
}

/// Upload and process a document
pub async fn upload_document(file_path: String, conversation_id: i64, app_handle: &AppHandle) -> Result<DocumentMetadata> {
    crate::logger::log_info(&format!("Starting document upload: {}", file_path));

    let path = Path::new(&file_path);

    if !path.exists() {
        crate::logger::log_error(&format!("File does not exist: {}", file_path));
        anyhow::bail!("File does not exist: {}", file_path);
    }

    // Security: Canonicalize path to prevent path traversal attacks
    let canonical_path = path.canonicalize()
        .context("Invalid file path or insufficient permissions")?;

    // Verify the path is a file (not a directory or symlink to something dangerous)
    if !canonical_path.is_file() {
        anyhow::bail!("Path must be a file, not a directory or special file");
    }

    // Validate file is within user's home directory or app data directory for security
    let home_dir = std::env::var("HOME").unwrap_or_default();
    let app_data_dir = app_handle.path().app_data_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let path_str = canonical_path.to_string_lossy();
    let is_safe_location = path_str.starts_with(&home_dir) ||
                           path_str.starts_with(&app_data_dir) ||
                           path_str.starts_with("/tmp") || // Allow tmp for testing
                           path_str.starts_with("/var/tmp");

    if !is_safe_location {
        crate::logger::log_error(&format!("Access denied: file path outside allowed directories: {}", path_str));
        anyhow::bail!("Access denied: file must be in your home directory or app data directory");
    }

    // Detect file type
    let file_type = detect_file_type(path)?;

    // Extract text based on file type
    let content = extract_text(path, &file_type, app_handle).await?;

    // Chunk the document
    let chunks = chunk_text(&content, 800, 200)?;

    // Validate that we have content to process
    if chunks.is_empty() {
        crate::logger::log_error(&format!("Document has no extractable content: {}", file_path));
        anyhow::bail!("Document appears to be empty or contains no extractable text. Please ensure the file contains readable content.");
    }

    // Limit number of chunks to prevent excessive memory/database usage
    const MAX_CHUNKS: usize = 2000; // ~1.6 MB of text content
    if chunks.len() > MAX_CHUNKS {
        anyhow::bail!(
            "Document has too many chunks ({} chunks). Maximum allowed is {} chunks. Consider splitting the document or reducing its size.",
            chunks.len(),
            MAX_CHUNKS
        );
    }

    // Get file metadata and validate size
    let metadata = std::fs::metadata(path)?;
    let size_bytes = metadata.len();

    // Limit file size to 100MB to prevent memory exhaustion
    const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024; // 100 MB
    if size_bytes > MAX_FILE_SIZE {
        anyhow::bail!(
            "File size ({:.2} MB) exceeds maximum allowed size of 100 MB. Please upload a smaller file.",
            size_bytes as f64 / (1024.0 * 1024.0)
        );
    }

    let size_bytes_i64 = size_bytes as i64;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Get current timestamp
    let upload_date = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs() as i64;

    use unicode_segmentation::UnicodeSegmentation;
    let word_count = content.unicode_words().count() as i64;

    let title = extract_document_title(&content, &file_name);

    let (author, creation_date) = extract_document_properties(path, &file_type);

    let page_count = estimate_page_count(&content, &file_type);

    // Generate embeddings BEFORE database transaction to avoid holding transaction during slow operation
    crate::logger::log_info(&format!("Generating embeddings for {} chunks using parallel processing...", chunks.len()));
    let embedding_generator = crate::embeddings::EmbeddingGenerator::shared()?;

    // Generate all embeddings in parallel batches
    let embeddings = if chunks.len() > 100 {
        // For large documents, use parallel processing with progress tracking
        crate::logger::log_info("Using parallel batch processing for large document (100+ chunks)");
        embedding_generator.generate_embeddings_parallel(&chunks, |processed, total| {
            if processed % 50 == 0 || processed == total {
                crate::logger::log_info(&format!("Embedding progress: {}/{} chunks", processed, total));
            }
        })?
    } else {
        // For smaller documents, use simple parallel processing
        embedding_generator.generate_embeddings_parallel_simple(&chunks)?
    };

    // Store in database within a transaction for atomicity
    let conn = crate::database::get_connection(app_handle)?;

    // Begin transaction
    conn.execute("BEGIN IMMEDIATE", [])?;

    let result: Result<i64> = (|| {
        conn.execute(
            "INSERT INTO documents (file_name, file_path, file_type, upload_date, size_bytes, title, author, creation_date, page_count, word_count, conversation_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![file_name, file_path, file_type, upload_date, size_bytes_i64, title, author, creation_date, page_count, word_count, conversation_id],
        )?;

        let document_id = conn.last_insert_rowid();

        // Store chunks with embeddings
        crate::logger::log_info("Storing chunks and embeddings in database...");
        for (index, (chunk, embedding)) in chunks.iter().zip(embeddings.iter()).enumerate() {
            let embedding_bytes = embedding.to_bytes();

            conn.execute(
                "INSERT INTO chunks (document_id, chunk_text, chunk_index, page_number, embedding)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![document_id, chunk, index as i64, None::<i64>, embedding_bytes],
            )?;
        }

        Ok(document_id)
    })();

    let document_id = match result {
        Ok(id) => {
            conn.execute("COMMIT", [])?;
            crate::logger::log_info("All embeddings generated and stored successfully");
            id
        }
        Err(e) => {
            conn.execute("ROLLBACK", []).ok(); // Rollback on error
            crate::logger::log_error(&format!("Failed to store document, rolled back transaction: {}", e));
            return Err(e);
        }
    };

    let metadata = DocumentMetadata {
        id: document_id,
        file_name: file_name.clone(),
        file_path,
        file_type,
        upload_date,
        size_bytes: size_bytes_i64,
        chunks_count: chunks.len(),
        title,
        author,
        creation_date,
        page_count,
        word_count: Some(word_count),
        conversation_id: Some(conversation_id),
    };

    crate::logger::log_info(&format!("Document uploaded successfully: {} ({} chunks)", file_name, chunks.len()));

    Ok(metadata)
}

/// List all documents
pub async fn list_documents(conversation_id: i64, app_handle: &AppHandle) -> Result<Vec<DocumentMetadata>> {
    let conn = crate::database::get_connection(app_handle)?;

    let mut stmt = conn.prepare(
        "SELECT d.id, d.file_name, d.file_path, d.file_type, d.upload_date, d.size_bytes,
                COUNT(c.id) as chunks_count, d.title, d.author, d.creation_date, d.page_count, d.word_count, d.conversation_id
         FROM documents d
         LEFT JOIN chunks c ON d.id = c.document_id
         WHERE d.conversation_id = ?1
         GROUP BY d.id
         ORDER BY d.upload_date DESC"
    )?;

    let documents = stmt.query_map([conversation_id], |row| {
        Ok(DocumentMetadata {
            id: row.get(0)?,
            file_name: row.get(1)?,
            file_path: row.get(2)?,
            file_type: row.get(3)?,
            upload_date: row.get(4)?,
            size_bytes: row.get(5)?,
            chunks_count: row.get::<_, i64>(6)? as usize,
            title: row.get(7).ok(),
            author: row.get(8).ok(),
            creation_date: row.get(9).ok(),
            page_count: row.get(10).ok(),
            word_count: row.get(11).ok(),
            conversation_id: row.get(12).ok(),
        })
    })?;

    let mut result = Vec::new();
    for doc in documents {
        result.push(doc?);
    }

    Ok(result)
}

/// Detect file type from extension
fn detect_file_type(path: &Path) -> Result<String> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .context("Failed to get file extension")?
        .to_lowercase();

    match extension.as_str() {
        "pdf" => Ok("pdf".to_string()),
        "docx" => Ok("docx".to_string()),
        "txt" => Ok("txt".to_string()),
        "md" => Ok("markdown".to_string()),
        "jpg" | "jpeg" => Ok("jpeg".to_string()),
        "png" => Ok("png".to_string()),
        "html" | "htm" => Ok("html".to_string()),
        "mhtml" | "mht" => Ok("mhtml".to_string()),
        "csv" => Ok("csv".to_string()),
        "xlsx" | "xls" => Ok("xlsx".to_string()),
        "rs" => Ok("code_rust".to_string()),
        "py" => Ok("code_python".to_string()),
        "js" | "jsx" => Ok("code_javascript".to_string()),
        "ts" | "tsx" => Ok("code_typescript".to_string()),
        "go" => Ok("code_go".to_string()),
        "java" => Ok("code_java".to_string()),
        "cpp" | "cc" | "cxx" | "c" | "h" | "hpp" => Ok("code_cpp".to_string()),
        "rb" => Ok("code_ruby".to_string()),
        "php" => Ok("code_php".to_string()),
        "swift" => Ok("code_swift".to_string()),
        "kt" | "kts" => Ok("code_kotlin".to_string()),
        "sh" | "bash" => Ok("code_shell".to_string()),
        "yaml" | "yml" => Ok("code_yaml".to_string()),
        "json" => Ok("code_json".to_string()),
        "xml" => Ok("code_xml".to_string()),
        "sql" => Ok("code_sql".to_string()),
        _ => anyhow::bail!("Unsupported file type: {}", extension),
    }
}

/// Extract text from document based on file type
async fn extract_text(path: &Path, file_type: &str, app_handle: &tauri::AppHandle) -> Result<String> {
    match file_type {
        "pdf" => extract_pdf_text(path).await,
        "docx" => extract_docx_text(path),
        "txt" | "markdown" => extract_plain_text(path),
        "jpeg" | "png" => extract_image_text(path, app_handle).await,
        "html" | "mhtml" => extract_html_text(path),
        "csv" | "xlsx" => extract_spreadsheet_text(path),
        t if t.starts_with("code_") => extract_code_text(path, file_type),
        _ => anyhow::bail!("Unsupported file type: {}", file_type),
    }
}

/// Extract text from image using OCR
async fn extract_image_text(path: &Path, app_handle: &tauri::AppHandle) -> Result<String> {
    crate::logger::log_info(&format!("Extracting text from image: {:?}", path));

    // Use OCR to extract text from image
    match crate::ocr::extract_text_from_image(path, Some(app_handle)).await {
        Ok(text) => {
            crate::logger::log_info(&format!("✅ OCR successful! Extracted {} characters from image", text.len()));
            Ok(text)
        }
        Err(ocr_error) => {
            crate::logger::log_error(&format!("OCR failed: {}", ocr_error));
            anyhow::bail!(
                "Failed to extract text from image.\n\n\
                OCR Error: {}\n\n\
                Make sure the image contains readable text and is of good quality.",
                ocr_error
            );
        }
    }
}

/// Extract text from PDF (with automatic OCR fallback for scanned PDFs and table detection)
async fn extract_pdf_text(path: &Path) -> Result<String> {
    crate::logger::log_info(&format!("Extracting text from PDF: {:?}", path));

    let bytes = std::fs::read(path)?;
    crate::logger::log_info(&format!("PDF file size: {} bytes", bytes.len()));

    let mut text = pdf_extract::extract_text_from_mem(&bytes)
        .context("Failed to extract text from PDF")?;

    crate::logger::log_info(&format!("Extracted {} characters from PDF using standard extraction", text.len()));

    if text.trim().is_empty() {
        crate::logger::log_warn(&format!("Standard PDF extraction resulted in empty text. Attempting OCR..."));

        match crate::ocr::extract_text_from_scanned_pdf(path).await {
            Ok(ocr_text) => {
                crate::logger::log_info(&format!("OCR successful! Extracted {} characters", ocr_text.len()));
                return Ok(ocr_text);
            }
            Err(ocr_error) => {
                crate::logger::log_error(&format!("OCR failed: {}", ocr_error));
                anyhow::bail!(
                    "PDF extraction resulted in empty text.\n\n\
                    This PDF appears to be scanned/image-based.\n\n\
                    OCR Error: {}\n\n\
                    File size: {} bytes",
                    ocr_error,
                    bytes.len()
                );
            }
        }
    }

    if let Ok(table_text) = extract_pdf_tables(path) {
        if !table_text.is_empty() {
            text.push_str("\n\n");
            text.push_str("Extracted Tables:\n");
            text.push_str(&"=".repeat(20));
            text.push_str("\n\n");
            text.push_str(&table_text);
            crate::logger::log_info("Successfully extracted tables from PDF");
        }
    }

    Ok(text)
}

/// Attempt to extract tables from PDF using lopdf
fn extract_pdf_tables(path: &Path) -> Result<String> {
    use lopdf::Document;

    let doc = Document::load(path)
        .context("Failed to load PDF for table extraction")?;

    let mut tables_text = String::new();
    let page_numbers = doc.get_pages();

    for (page_num, _page_id) in page_numbers.iter().enumerate() {
        if let Ok(text) = doc.extract_text(&[page_num as u32 + 1]) {
            let lines: Vec<&str> = text.lines().collect();

            let mut potential_table_lines = Vec::new();
            let mut in_table = false;

            for line in lines {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let has_multiple_spaces = trimmed.matches("  ").count() >= 2;
                let has_pipe = trimmed.contains('|');
                let has_tab = trimmed.contains('\t');

                if has_multiple_spaces || has_pipe || has_tab {
                    if !in_table {
                        in_table = true;
                        if !tables_text.is_empty() {
                            tables_text.push_str("\n\n");
                        }
                        tables_text.push_str(&format!("Table from page {}:\n", page_num + 1));
                    }
                    potential_table_lines.push(trimmed);
                } else if in_table && potential_table_lines.len() >= 2 {
                    for table_line in &potential_table_lines {
                        tables_text.push_str(table_line);
                        tables_text.push('\n');
                    }
                    potential_table_lines.clear();
                    in_table = false;
                }
            }

            if in_table && potential_table_lines.len() >= 2 {
                for table_line in &potential_table_lines {
                    tables_text.push_str(table_line);
                    tables_text.push('\n');
                }
            }
        }
    }

    Ok(tables_text)
}

/// Extract text from DOCX, with a raw-XML fallback for files docx-rs rejects.
///
/// A .docx is a zip; when the typed parser fails or yields nothing, we read
/// `word/document.xml` directly. This recovers text from valid-but-unusual
/// documents that broke ingestion before.
fn extract_docx_text(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("Could not read the file: {}", path.display()))?;

    // Primary path: the typed parser preserves structure best.
    match extract_docx_via_docx_rs(&bytes) {
        Ok(text) if !text.trim().is_empty() => return Ok(text),
        Ok(_) => crate::logger::log_warn("DOCX parsed but held no text; trying raw-XML fallback"),
        Err(e) => crate::logger::log_warn(&format!("docx-rs failed ({}); trying raw-XML fallback", e)),
    }

    // Fallback path: read word/document.xml out of the zip directly.
    match extract_docx_via_xml(&bytes) {
        Ok(text) if !text.trim().is_empty() => {
            crate::logger::log_info("Recovered DOCX text via raw-XML fallback");
            Ok(text)
        }
        Ok(_) => anyhow::bail!(
            "This Word document contains no readable text. If it is mostly images or \
             scanned pages, try exporting it to PDF and uploading that instead."
        ),
        Err(e) => anyhow::bail!(
            "Could not read this Word document. It may be corrupt or password-protected. \
             (Details: {})",
            e
        ),
    }
}

/// Primary DOCX extractor using the typed `docx-rs` parser.
fn extract_docx_via_docx_rs(bytes: &[u8]) -> Result<String> {
    let docx = docx_rs::read_docx(bytes).map_err(|e| anyhow::anyhow!("{:?}", e))?;

    let mut text = String::new();
    for child in docx.document.children {
        if let docx_rs::DocumentChild::Paragraph(para) = child {
            for para_child in para.children {
                if let docx_rs::ParagraphChild::Run(run) = para_child {
                    for run_child in run.children {
                        if let docx_rs::RunChild::Text(t) = run_child {
                            text.push_str(&t.text);
                        }
                    }
                }
            }
            text.push('\n');
        }
    }
    Ok(text)
}

/// Fallback DOCX extractor: unzip and read `word/document.xml`.
fn extract_docx_via_xml(bytes: &[u8]) -> Result<String> {
    use std::io::{Cursor, Read};

    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .context("not a valid .docx (zip) file")?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .context("missing word/document.xml")?
        .read_to_string(&mut xml)
        .context("could not read word/document.xml")?;

    Ok(text_from_document_xml(&xml))
}

/// Pull readable text out of a WordprocessingML `document.xml`, preserving
/// paragraph (`w:p`), line-break (`w:br`), and tab (`w:tab`) boundaries.
fn text_from_document_xml(xml: &str) -> String {
    // The first alternative is anchored so it matches <w:t> / <w:t ...> but not
    // <w:tab/>, which must be handled as a tab, not a text run.
    let re = regex::Regex::new(
        r"(?s)<w:t(?:\s[^>]*)?>(.*?)</w:t>|<w:tab[^>]*/?>|<w:br[^>]*/?>|</w:p>",
    )
    .expect("valid regex");

    let mut out = String::new();
    for caps in re.captures_iter(xml) {
        if let Some(t) = caps.get(1) {
            out.push_str(&unescape_xml(t.as_str()));
        } else {
            let matched = caps.get(0).map(|m| m.as_str()).unwrap_or("");
            if matched.starts_with("<w:tab") {
                out.push('\t');
            } else {
                out.push('\n'); // <w:br/> or </w:p>
            }
        }
    }
    out
}

/// Unescape the five predefined XML entities.
fn unescape_xml(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// File extensions Enklayve can ingest (lowercase, without the dot).
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "pdf", "docx", "txt", "md", "csv", "xlsx", "xls", "html", "htm", "mhtml", "mht",
    "jpg", "jpeg", "png",
];

/// Is this file extension one we can ingest?
pub fn is_supported_extension(ext: &str) -> bool {
    SUPPORTED_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// Recursively collect ingestible files under `dir`, skipping hidden entries and
/// common junk directories. Returns a sorted list for stable ordering.
pub fn collect_supported_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    let mut stack: Vec<std::path::PathBuf> = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();

            if name.starts_with('.') {
                continue; // hidden files/dirs
            }

            if path.is_dir() {
                if matches!(name.as_ref(), "node_modules" | "target" | "venv" | "__pycache__") {
                    continue;
                }
                stack.push(path);
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if is_supported_extension(ext) {
                    out.push(path);
                }
            }
        }
    }

    out.sort();
    out
}

#[cfg(test)]
mod docx_tests {
    use super::*;

    #[test]
    fn extracts_paragraphs() {
        let xml = r#"<w:body><w:p><w:r><w:t>Hello</w:t></w:r></w:p><w:p><w:r><w:t xml:space="preserve"> world</w:t></w:r></w:p></w:body>"#;
        assert_eq!(text_from_document_xml(xml), "Hello\n world\n");
    }

    #[test]
    fn unescapes_entities() {
        let xml = r#"<w:p><w:r><w:t>a &amp; b &lt;tag&gt;</w:t></w:r></w:p>"#;
        assert_eq!(text_from_document_xml(xml), "a & b <tag>\n");
    }

    #[test]
    fn handles_breaks_and_tabs() {
        let xml = r#"<w:p><w:r><w:t>a</w:t><w:tab/><w:t>b</w:t><w:br/><w:t>c</w:t></w:r></w:p>"#;
        assert_eq!(text_from_document_xml(xml), "a\tb\nc\n");
    }

    #[test]
    fn invalid_zip_is_an_error_not_a_panic() {
        assert!(extract_docx_via_xml(b"not a zip").is_err());
    }

    #[test]
    fn supported_extension_is_case_insensitive() {
        assert!(is_supported_extension("PDF"));
        assert!(is_supported_extension("docx"));
        assert!(!is_supported_extension("exe"));
    }

    #[test]
    fn collect_skips_unsupported_hidden_and_junk() {
        use std::fs;
        let base = std::env::temp_dir().join(format!(
            "enklayve_collect_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(base.join("node_modules")).unwrap();
        fs::create_dir_all(base.join("sub")).unwrap();
        fs::write(base.join("a.txt"), b"hi").unwrap();
        fs::write(base.join("b.pdf"), b"%PDF").unwrap();
        fs::write(base.join("c.xyz"), b"x").unwrap(); // unsupported
        fs::write(base.join(".secret.txt"), b"x").unwrap(); // hidden
        fs::write(base.join("sub").join("d.md"), b"x").unwrap();
        fs::write(base.join("node_modules").join("e.txt"), b"x").unwrap(); // junk dir

        let found = collect_supported_files(&base);
        let names: Vec<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert!(names.contains(&"a.txt".to_string()));
        assert!(names.contains(&"b.pdf".to_string()));
        assert!(names.contains(&"d.md".to_string()));
        assert!(!names.iter().any(|n| n == "c.xyz"));
        assert!(!names.iter().any(|n| n == ".secret.txt"));
        assert!(!names.iter().any(|n| n == "e.txt"));
        assert_eq!(found.len(), 3);

        let _ = fs::remove_dir_all(&base);
    }
}

/// Extract text from plain text file
fn extract_plain_text(path: &Path) -> Result<String> {
    Ok(std::fs::read_to_string(path)?)
}

/// Extract text from HTML or MHTML file
fn extract_html_text(path: &Path) -> Result<String> {
    crate::logger::log_info(&format!("Extracting text from HTML: {:?}", path));

    let html_content = std::fs::read_to_string(path)
        .context("Failed to read HTML file")?;

    let text = html2text::from_read(html_content.as_bytes(), 120);

    crate::logger::log_info(&format!("Extracted {} characters from HTML", text.len()));

    Ok(text)
}

/// Extract text from spreadsheet (CSV or XLSX)
fn extract_spreadsheet_text(path: &Path) -> Result<String> {
    crate::logger::log_info(&format!("Extracting text from spreadsheet: {:?}", path));

    let mut workbook = open_workbook_auto(path)
        .context("Failed to open spreadsheet")?;

    let mut output = String::new();

    let sheet_names = workbook.sheet_names().to_vec();

    for sheet_name in sheet_names {
        if let Ok(range) = workbook.worksheet_range(&sheet_name) {
            if output.len() > 0 {
                output.push_str("\n\n");
            }
            output.push_str(&format!("Sheet: {}\n", sheet_name));
            output.push_str(&format!("{}\n\n", "=".repeat(sheet_name.len() + 7)));

            let rows: Vec<Vec<_>> = range.rows().map(|r| r.to_vec()).collect();

            if rows.is_empty() {
                continue;
            }

            let mut col_widths: Vec<usize> = vec![0; rows[0].len()];
            for row in &rows {
                for (col_idx, cell) in row.iter().enumerate() {
                    let cell_str = format!("{}", cell);
                    if cell_str.len() > col_widths[col_idx] {
                        col_widths[col_idx] = std::cmp::min(cell_str.len(), 30);
                    }
                }
            }

            for row in rows {
                let formatted_row: Vec<String> = row
                    .iter()
                    .enumerate()
                    .map(|(idx, cell)| {
                        let cell_str = format!("{}", cell);
                        let truncated = if cell_str.len() > 30 {
                            format!("{}...", &cell_str[..27])
                        } else {
                            cell_str
                        };
                        format!("{:width$}", truncated, width = col_widths[idx])
                    })
                    .collect();

                output.push_str("| ");
                output.push_str(&formatted_row.join(" | "));
                output.push_str(" |\n");
            }
        }
    }

    crate::logger::log_info(&format!("Extracted {} characters from spreadsheet", output.len()));

    Ok(output)
}

/// Extract text from code files
fn extract_code_text(path: &Path, file_type: &str) -> Result<String> {
    crate::logger::log_info(&format!("Extracting text from code file: {:?}", path));

    let code_content = std::fs::read_to_string(path)
        .context("Failed to read code file")?;

    let language = file_type.strip_prefix("code_").unwrap_or("text");

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let mut output = String::new();
    output.push_str(&format!("File: {}\n", file_name));
    output.push_str(&format!("Language: {}\n\n", language));
    output.push_str("```");
    output.push_str(language);
    output.push('\n');
    output.push_str(&code_content);
    if !code_content.ends_with('\n') {
        output.push('\n');
    }
    output.push_str("```\n");

    crate::logger::log_info(&format!("Extracted {} characters from code file", output.len()));

    Ok(output)
}

/// Chunk text into semantic segments with intelligent boundary detection
fn chunk_text(text: &str, chunk_size: usize, overlap: usize) -> Result<Vec<String>> {
    use unicode_segmentation::UnicodeSegmentation;

    // Validate parameters to prevent infinite loops
    if chunk_size == 0 {
        anyhow::bail!("chunk_size must be greater than 0");
    }
    if overlap >= chunk_size {
        anyhow::bail!("overlap must be less than chunk_size (overlap: {}, chunk_size: {})", overlap, chunk_size);
    }
    if chunk_size > 10000 {
        anyhow::bail!("chunk_size too large (max 10000 words)");
    }

    if text.trim().is_empty() {
        return Ok(vec![]);
    }

    let paragraphs: Vec<&str> = text.split("\n\n").collect();
    let mut chunks = Vec::new();
    let mut current_chunk = String::new();
    let mut current_word_count = 0;
    let mut last_heading = String::new();

    for paragraph in paragraphs {
        let trimmed = paragraph.trim();
        if trimmed.is_empty() {
            continue;
        }

        let is_heading = trimmed.starts_with('#')
            || (trimmed.len() < 100 && trimmed.lines().count() == 1 && !trimmed.ends_with('.'));

        if is_heading {
            last_heading = trimmed.to_string();
        }

        let para_words: Vec<&str> = trimmed.unicode_words().collect();
        let para_word_count = para_words.len();

        if current_word_count + para_word_count > chunk_size && current_word_count > 0 {
            chunks.push(current_chunk.clone());

            let overlap_words = if current_word_count > overlap {
                let words: Vec<&str> = current_chunk.unicode_words().collect();
                let start_idx = words.len().saturating_sub(overlap);
                words[start_idx..].join(" ")
            } else {
                current_chunk.clone()
            };

            current_chunk = String::new();
            if !last_heading.is_empty() && !overlap_words.contains(&last_heading) {
                current_chunk.push_str(&last_heading);
                current_chunk.push_str("\n\n");
            }
            current_chunk.push_str(&overlap_words);
            current_chunk.push_str("\n\n");
            current_word_count = current_chunk.unicode_words().count();
        }

        if !current_chunk.is_empty() && !current_chunk.ends_with("\n\n") {
            current_chunk.push_str("\n\n");
        }
        current_chunk.push_str(trimmed);
        current_word_count += para_word_count;
    }

    if !current_chunk.trim().is_empty() {
        chunks.push(current_chunk);
    }

    if chunks.is_empty() {
        let words: Vec<&str> = text.unicode_words().collect();
        if !words.is_empty() {
            let mut start = 0;
            while start < words.len() {
                let end = std::cmp::min(start + chunk_size, words.len());
                let chunk = words[start..end].join(" ");
                chunks.push(chunk);

                if end >= words.len() {
                    break;
                }
                start += chunk_size - overlap;
            }
        }
    }

    Ok(chunks)
}

/// Extract document title from content or file name
fn extract_document_title(content: &str, file_name: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().take(20).collect();

    for line in lines {
        let trimmed = line.trim();

        if trimmed.starts_with("# ") {
            return Some(trimmed.trim_start_matches("# ").to_string());
        }

        if !trimmed.is_empty()
            && trimmed.len() < 200
            && !trimmed.starts_with("```")
            && !trimmed.contains("http")
        {
            return Some(trimmed.to_string());
        }
    }

    let name_without_ext = file_name.rsplit_once('.').map(|(n, _)| n).unwrap_or(file_name);
    Some(name_without_ext.to_string())
}

/// Extract document properties from file metadata
fn extract_document_properties(path: &Path, file_type: &str) -> (Option<String>, Option<i64>) {
    if file_type == "pdf" {
        if let Ok(pdf_metadata) = extract_pdf_metadata(path) {
            return pdf_metadata;
        }
    }

    let author: Option<String> = None;
    let creation_date = if let Ok(metadata) = std::fs::metadata(path) {
        if let Ok(created) = metadata.created() {
            if let Ok(duration) = created.duration_since(SystemTime::UNIX_EPOCH) {
                Some(duration.as_secs() as i64)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    (author, creation_date)
}

/// Extract metadata from PDF
fn extract_pdf_metadata(path: &Path) -> Result<(Option<String>, Option<i64>)> {
    use lopdf::Document;

    let doc = Document::load(path)?;
    let mut author: Option<String> = None;
    let mut creation_date: Option<i64> = None;

    if let Ok(info) = doc.trailer.get(b"Info") {
        if let Ok(info_dict) = info.as_dict() {
            if let Ok(author_obj) = info_dict.get(b"Author") {
                if let Ok(author_str) = author_obj.as_str() {
                    author = Some(String::from_utf8_lossy(author_str).to_string());
                }
            }
        }
    }

    Ok((author, creation_date))
}

/// Estimate page count based on content and file type
fn estimate_page_count(content: &str, file_type: &str) -> Option<i64> {
    if file_type == "pdf" {
        return None;
    }

    use unicode_segmentation::UnicodeSegmentation;
    let word_count = content.unicode_words().count();

    let words_per_page = 500;
    let estimated_pages = (word_count as f64 / words_per_page as f64).ceil() as i64;

    Some(estimated_pages.max(1))
}

/// Delete a document and its chunks
pub async fn delete_document(app_handle: &AppHandle, document_id: i64) -> Result<()> {
    let conn = crate::database::get_connection(app_handle)?;
    conn.execute("DELETE FROM documents WHERE id = ?", [document_id])?;
    Ok(())
}
