# Change: Fix generation formatting and document ingestion

## Why

Two defects in v0.1.6 block everything else in the vision:

1. **Chat output is "weirdly formatted and not uniform."** Root cause: prompts
   are tokenized as a raw string (`inference.rs::generate_simple` calls
   `str_to_token(prompt, AddBos::Always)` with no chat template). Without the
   model's chat template and a system prompt, a Qwen-class model produces
   inconsistent markdown and can leak or mis-handle special tokens.

2. **DOCX and other formats break.** `documents.rs::extract_text` dispatches to a
   single extractor per format with thin fallbacks, so a valid-but-unusual
   `.docx` that `docx-rs` rejects fails the whole import.

Neither is a redesign; both are correctness fixes that the rest of the roadmap
depends on. This change also establishes the baseline resilience rule (no
user-facing panics) for these paths.

## What changes

- Add a chat-template layer that wraps system prompt + history + retrieved
  context in the active model's template before tokenization.
- Add a default formatting system prompt for uniform markdown answers.
- Harden the markdown renderer for streaming (stable incremental render, no raw
  special tokens, graceful handling of unclosed code fences).
- Add a DOCX fallback: unzip and parse `word/document.xml` when `docx-rs` fails.
- Make every extractor return a plain-language error and support "partial import"
  instead of failing the batch.

## Impact

- Affected specs: `chat-experience`, `document-ingestion`, `resilience`.
- Affected code: `src-tauri/src/inference.rs`, `src-tauri/src/documents.rs`,
  `src/components/Chat/MessageBubble.tsx`, model metadata (chat template per
  model in `model_selection.rs` / `models.rs`).
- No data migration. No change to the encryption or storage layer.
