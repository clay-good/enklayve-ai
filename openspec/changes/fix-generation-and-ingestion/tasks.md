# Tasks: Fix generation formatting and document ingestion

## 1. Chat template + system prompt
- [ ] 1.1 Add a chat template per supported model to model metadata (name,
      special tokens, template string). Default to the Qwen3 ChatML template.
- [ ] 1.2 Add `build_prompt(system, history, retrieved_context, user_msg)` that
      renders the active model's template and returns the token-ready string.
- [ ] 1.3 Route `generate_simple` and `generate_streaming` through `build_prompt`;
      stop generation on the model's end-of-turn token, strip any trailing
      special tokens from output.
- [ ] 1.4 Add a default formatting system prompt (concise, structured markdown).
- [ ] 1.5 Verify: identical-style answers for two comparable questions; no
      special tokens visible in output.

## 2. Streaming markdown rendering
- [ ] 2.1 In `MessageBubble`, render streamed markdown without reflow/flicker.
- [ ] 2.2 Handle an unclosed code fence mid-stream gracefully.
- [ ] 2.3 Never display raw special tokens if any slip through.

## 3. DOCX fallback
- [ ] 3.1 On `docx-rs` failure, unzip the `.docx` and extract text from
      `word/document.xml` (strip tags, join paragraphs).
- [ ] 3.2 Log which path succeeded; mark as full or partial import.
- [ ] 3.3 Verify against the real file(s) that break in v0.1.6.

## 4. Ingestion resilience
- [ ] 4.1 Every extractor returns a plain-language error (no panic, no raw
      anyhow chain to the UI).
- [ ] 4.2 Introduce a "partial" document state; index recoverable text instead of
      dropping the file.
- [ ] 4.3 One failed file in a batch does not abort the batch; expose per-file
      retry.

## 5. Regression coverage
- [ ] 5.1 Add fixture files (normal + malformed DOCX, text-less PDF, corrupt
      file) and tests for the fallback and partial-import paths.
- [ ] 5.2 Add a generation test asserting the prompt is template-wrapped.
