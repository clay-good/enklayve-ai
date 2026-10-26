# Tasks: Ask your data, done well

Status: implemented and unit-tested (2026-10-25). Retrieval quality and the
passage viewer need in-app verification with a real model and corpus.

## 1. Retrieval honesty (commit 3a)
- [x] 1.1 `top_semantic_similarity` relevance signal.
- [x] 1.2 Relevance gate in both chat paths; drop weak context.
- [x] 1.3 Honest "not in your documents" preface when docs exist but aren't relevant.
- [ ] 1.4 Tune `RELEVANCE_THRESHOLD` against real embeddings/corpora.

## 2. Clickable citations (commit 3a)
- [x] 2.1 Instruct the model to cite `[file.ext]`.
- [x] 2.2 `get_document_passages(file_name)` command.
- [x] 2.3 Render citations as clickable links + passage viewer modal.
- [ ] 2.4 In-app check that citations open the right passages.

## 3. Folder ingestion (commit 3b)
- [x] 3.1 `collect_supported_files` recursive walk (skip hidden/junk); unit-tested.
- [x] 3.2 `upload_folder` command: per-file failures recorded, batch never aborts.
- [x] 3.3 Per-file progress events; frontend "+ Folder" with progress line + summary.
- [ ] 3.4 In-app check on a large mixed folder.

## 4. Index integrity + rebuild (commit 3b)
- [x] 4.1 `check_index_integrity`: orphans, undecodable embeddings, docs w/o chunks.
- [x] 4.2 `rebuild_index`: drop orphans, re-embed from stored text.
- [x] 4.3 Frontend "Repair" action with a result message.
- [ ] 4.4 Startup auto-check that offers repair (currently manual) — Phase 5 polish.
