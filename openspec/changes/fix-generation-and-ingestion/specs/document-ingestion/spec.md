# Document Ingestion (delta)

## MODIFIED Requirements

### Requirement: The app SHALL extract text from every supported format with fallbacks

Each format SHALL have a primary extractor and at least one fallback before it is
declared a failure. DOCX SHALL fall back to parsing `word/document.xml` directly
when the typed parser rejects the file.

#### Scenario: DOCX that the primary parser rejects
- **WHEN** `docx-rs` fails on a valid-but-unusual .docx
- **THEN** the app unzips it and extracts text from `word/document.xml`
- **AND** ingestion succeeds and the fallback path is logged

## ADDED Requirements

### Requirement: Ingestion SHALL support partial import instead of failing the file

When a file is only partially readable, the app SHALL index the recoverable text
and mark the document "partial" with a reason, rather than dropping it.

#### Scenario: Partially corrupt file
- **WHEN** a file is only partially readable
- **THEN** the recoverable text is indexed and the document is flagged partial

### Requirement: A failed file SHALL NOT abort a batch

#### Scenario: One bad file in a large batch
- **WHEN** one file in a folder import fails
- **THEN** the rest complete and the failure is shown with a retry option
