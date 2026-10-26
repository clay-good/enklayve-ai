# Tasks: Fix generation formatting and document ingestion

Status: implemented and unit-tested (2026-10-25). Full in-app verification with a
downloaded model is still pending (heavy native build; see proposal).

## 1. Generation formatting
- [x] 1.1 Add `response_format` module: strip `<think>` blocks + control tokens,
      preserve Markdown (`sanitize_final` + `StreamSanitizer`).
- [x] 1.2 Disable Qwen3 thinking for doc Q&A (`/no_think`) in both chat paths.
- [x] 1.3 Add a Markdown formatting instruction to the system prompt.
- [x] 1.4 Remove `clean_response` (it was destroying Markdown).
- [ ] 1.5 Manual in-app check: two comparable questions format consistently; no
      `<think>` or control tokens visible.

## 2. Streaming consistency
- [x] 2.1 Wrap the streaming callback with `StreamSanitizer` so the live view
      matches the final answer.
- [x] 2.2 Unit test: stream output equals `sanitize_final` for arbitrary chunk
      boundaries, including tags split token-by-token.
- [ ] 2.3 Manual in-app check: no flicker/snap on completion.

## 3. DOCX fallback
- [x] 3.1 On `docx-rs` failure or empty text, unzip and parse `word/document.xml`.
- [x] 3.2 Plain-language error when both paths fail.
- [x] 3.3 Unit tests for XML text extraction (paragraphs, entities, breaks/tabs,
      invalid zip).
- [ ] 3.4 Verify against the real file(s) that break in v0.1.6 (needs the files).

## 4. Deferred to Phase 3 (folder ingestion)
- [ ] Persisted "partial" document state and per-file batch resilience — today
      upload is single-file, so there is no batch to protect yet.
