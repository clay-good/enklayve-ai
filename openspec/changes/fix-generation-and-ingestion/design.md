# Design notes

## Chat template

The correctness fix is to stop feeding raw strings to the tokenizer. Each model
family needs its template. Rather than depend on llama.cpp's built-in template
handling alone, carry an explicit template in model metadata so behavior is
predictable and testable across the models we ship.

Qwen3 (our default family) uses ChatML:

```
<|im_start|>system
{system}<|im_end|>
<|im_start|>user
{context}\n\n{user}<|im_end|>
<|im_start|>assistant
```

`build_prompt` assembles system + prior turns + retrieved context + current user
message, then renders the active model's template. Generation must stop on the
model's end-of-turn id (`<|im_end|>`), and any trailing special tokens are
stripped before the text reaches the UI. This single change is expected to
resolve the "weird, non-uniform" formatting, because the model will now be run
the way it was trained.

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
