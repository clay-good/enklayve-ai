# Change: Fix generation formatting and document ingestion

## Why

Two defects in v0.1.6 block everything else in the vision. (Note: the live chat
path is `commands.rs` + `model_cache.rs`; `inference.rs` is a secondary copy not
used by chat. The ChatML template is already applied, so that was *not* the
cause — the real causes were found by reading that live path.)

1. **Chat output is "weirdly formatted and not uniform."** Three compounding causes:
   - `commands.rs::clean_response` strips **all** Markdown (bold, italics, bullet
     and numbered lists) from the final answer — which is then handed to
     `<ReactMarkdown>`, producing flat, destructured text.
   - These are Qwen3 **reasoning** models with thinking left on, so
     `<think>…</think>` blocks are generated and never stripped.
   - The streaming path emits raw tokens (think blocks + control tokens), so the
     live stream looks different from the final answer, which then snaps to the
     de-Markdowned version. Stream and final never agreed.

2. **DOCX and other formats break.** `documents.rs::extract_text` dispatches to a
   single extractor per format with thin fallbacks, so a valid-but-unusual
   `.docx` that `docx-rs` rejects fails the whole import.

Neither is a redesign; both are correctness fixes that the rest of the roadmap
depends on. This change also establishes the baseline resilience rule (no
user-facing panics) for these paths.

## What changes

- Add a `response_format` module that strips `<think>` blocks and control tokens
  while **preserving Markdown**, with a stateful stream sanitizer proven (by
  unit test) to produce the same visible text as the final-pass cleaner.
- Replace `clean_response` so it no longer destroys Markdown.
- Disable Qwen3 thinking for document Q&A (`/no_think`) and add a Markdown
  formatting instruction to the system prompt (both chat paths).
- Sanitize the streaming callback so the live view matches the final answer.
- Add a DOCX raw-XML fallback (unzip and parse `word/document.xml`) when
  `docx-rs` fails or yields nothing, with plain-language errors.

Deferred to Phase 3 (folder ingestion): "partial import" as a persisted document
state and batch-level resilience — today upload is single-file, so there is no
batch to protect yet.

## Impact

- Affected specs: `chat-experience`, `document-ingestion`, `resilience`.
- Affected code: `src-tauri/src/commands.rs`, `src-tauri/src/documents.rs`, new
  `src-tauri/src/response_format.rs`. The frontend needed no change — it already
  renders Markdown; the fix was to stop destroying it server-side.
- No data migration. No change to the encryption or storage layer.
