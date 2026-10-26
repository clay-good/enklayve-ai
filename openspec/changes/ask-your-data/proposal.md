# Change: Ask your data, done well

## Why

Phase 3 turns retrieval from "always inject the top chunks" into trustworthy,
grounded answers, and makes ingestion and the index robust. Shipped in two
commits.

## What changes

### Retrieval honesty + citations (commit 3a)
- Relevance gate: `vector_search::top_semantic_similarity` gives the best cosine
  match; below `RELEVANCE_THRESHOLD` the documents are treated as not covering
  the question, so weak/irrelevant chunks are no longer forced into the prompt.
- Honest not-in-data: when documents exist but none are relevant, the model
  answers from general knowledge and says it did not find the answer in the
  user's documents. (Vector + keyword RRF fusion remains the reranking pass.)
- Clickable citations: the model is told to cite `[file.ext]`; the chat renderer
  turns those into links that open a passage viewer (`get_document_passages`).

### Folder ingestion + index repair (commit 3b)
- `upload_folder` recursively ingests every supported file (skipping hidden and
  junk dirs), records per-file failures without aborting the batch, and emits
  per-file progress. Frontend adds "+ Folder" with a live progress line.
- Index integrity + rebuild: `check_index_integrity` reports orphan chunks,
  undecodable embeddings, and docs without chunks; `rebuild_index` drops orphans
  and re-embeds from stored chunk text (no original files needed). Frontend adds
  a "Repair" action.

## Impact

- Affected specs: `ask-your-data`, `document-ingestion`, `resilience`.
- Affected code: `vector_search.rs`, `commands.rs`, `documents.rs`, new
  `index_health.rs`, `lib.rs`; frontend `MessageBubble`, `ChatWindow`,
  `DocumentPanel`, `App`, and their CSS.
- No schema change. The index is derived data, always rebuildable from
  `chunks.chunk_text`.

## Notes / follow-ups
- `RELEVANCE_THRESHOLD` (0.35) is a reasonable default to tune against real
  embeddings and corpora.
- Folder import surfaces its summary as a chat message; a dedicated toast/report
  surface could be nicer later.
