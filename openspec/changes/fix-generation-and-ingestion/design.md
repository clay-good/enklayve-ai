# Design notes

## What actually caused the bad formatting

ChatML is already applied in `commands.rs`, so the template was not the problem.
Reading the live path turned up three compounding causes:

1. `clean_response` stripped every Markdown affordance (bold, italics, bullet and
   numbered lists) from the final answer — and that de-Markdowned text was then
   rendered by `<ReactMarkdown>`, so it came out flat and inconsistent.
2. Qwen3 is a reasoning model; with thinking on it emits `<think>…</think>`,
   which was never stripped.
3. Streaming emitted raw tokens, so the live view (with think blocks and control
   tokens) never matched the cleaned final answer and visibly snapped on
   completion.

The fix is a `response_format` module that removes `<think>` blocks and control
tokens while leaving Markdown untouched, applied identically to the stream (a
stateful `StreamSanitizer`) and the final answer (`sanitize_final`); `/no_think`
plus a Markdown instruction in the system prompt; and `clean_response` deleted.
`StreamSanitizer` holds back any suffix that could begin a control token or
`<think>` tag, and a unit test proves the stream and final agree for arbitrary
chunk boundaries (including tags split token-by-token).

## DOCX fallback

`.docx` is a zip of XML. When `docx-rs` rejects a file, unzip it and read
`word/document.xml`, concatenating `<w:t>` text runs into paragraphs. This
recovers text from files with unusual styling/parts that trip the typed parser.
Keep `docx-rs` as the primary path (better structure); the unzip path is the
safety net. The same philosophy — primary extractor + raw fallback — is already
how PDF handles the text-layer/OCR split.

## Partial import

Add a document state `partial` alongside `ready`/`failed`. A partial document is
indexed on whatever text was recovered and flagged in the UI with the reason.
This keeps a single bad page or part from discarding an otherwise useful file and
aligns with the `resilience` spec.

## Out of scope

No new model runtime, no change to embeddings/storage/encryption, no new agent
tools. Those are separate roadmap phases.
