# Document Ingestion

## Purpose

Turn whatever files a user drops in — a single file or a whole folder — into
clean, searchable text, reliably. Parsing failures are the most common real-world
breakage (DOCX and malformed PDFs today), so ingestion must be robust, must fall
back gracefully, and must never fail silently or corrupt the index.

## Requirements

### Requirement: The app SHALL ingest a folder or individual files via drag-and-drop

The user SHALL be able to drop a folder; the app SHALL recursively discover
supported files, skip unsupported ones with a visible note, and index the rest.

#### Scenario: Drop a folder
- **WHEN** the user drops a folder containing PDFs, DOCX, and unsupported files
- **THEN** supported files are queued and indexed
- **AND** unsupported files are listed as skipped, not treated as errors

### Requirement: The app SHALL extract text from every supported format with fallbacks

Supported: PDF (incl. scanned via OCR), DOCX, TXT, Markdown, HTML, XLSX/XLS, CSV,
and common images (OCR). Each format SHALL have a primary extractor and at least
one fallback path before it is declared a failure.

#### Scenario: DOCX that the primary parser rejects
- **WHEN** `docx-rs` fails on a valid-but-unusual .docx (the v0.1.6 breakage)
- **THEN** the app falls back to unzipping the .docx and parsing
  `word/document.xml` directly for text
- **AND** if text is recovered, ingestion succeeds and the fallback is logged

#### Scenario: PDF with no extractable text layer
- **WHEN** a PDF yields empty text from the standard extractor
- **THEN** the app automatically renders pages and runs OCR
- **AND** reports page-level progress for large scanned PDFs

#### Scenario: Partially corrupt file
- **WHEN** a file is corrupt or only partially readable
- **THEN** the app extracts what it can, indexes the recoverable portion, and
  flags the document as "partially imported" with the reason — it does not drop
  the whole file or crash the batch

### Requirement: The app SHALL normalize extracted text to clean, chunkable content

Extracted text SHALL be normalized (whitespace, encoding, de-hyphenation,
table-to-text) before chunking so retrieval quality is consistent across formats.

#### Scenario: Spreadsheet to text
- **WHEN** an XLSX is ingested
- **THEN** sheets and rows are rendered as readable, delimited text that
  preserves headers, so the model can answer questions about the data

### Requirement: Ingestion SHALL report status per file and never block the UI

Each file SHALL surface a clear state: queued, parsing, indexing, ready, skipped,
partial, or failed — with a plain-language reason on anything less than ready.

#### Scenario: One bad file in a large batch
- **WHEN** indexing 200 files and one fails
- **THEN** the other 199 complete, the failure is shown with its reason, and the
  user can retry just that file
